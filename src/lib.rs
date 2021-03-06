//! # Lightning Element Control Protocol
//! This crate is used to remotely control LED elements over a variety of
//! transmission mechanisms.
//! Elements are controlled via messages that are serialized in to a byte-stream
//! and deserialized at the receiver which then controls the lights using GPIO pins.
//! 
//! ## Current implemented control mechanisms
//! - Bluetooth Low Energy with **bluetooth** feature.
//! - RFM69HCW packet radio with **ham** feature.

pub mod controller;
pub mod color;

#[cfg(feature = "bluetooth")]
pub mod bluetooth;

#[cfg(feature = "bluetooth")]
use rustable;

#[cfg(test)]
pub mod tests;

use ham::{PacketReceiver, PacketSender};
use std::sync::mpsc;
use std::time::{Duration, Instant};


/// `LedMsg` is the message format used control LEDs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LedMsg {
	/// The current time in microseconds from an arbitrary point in time.
    pub cur_time: u32,
	/// Which one of the 255 possible elements is being controlled.
    pub element: u8,
	/// The color to be set to. The u8 values are mapped to an actual
	/// RGBA [Color`] using a [`ColorMap`].
	///
	/// [`Color`]: ./color/struct.Color.html
	/// [`ColorMap`]: ./color/struct.ColorMap.html
    pub color: u8,
	/// Controls what the LED does.
    pub cmd: Command,
}
impl Default for LedMsg {
    #[inline]
    fn default() -> Self {
        LedMsg {
            cur_time: 0,
            element: 0,
            color: 0,
            cmd: Command::Null,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Command {
    Null,
    Flat(u8),
    PulseLinear(u8),
    PulseQuadratic(u8),
    FlatStack(u8),
}

#[derive(Debug)]
pub enum Error {
    BadInput(String),
    Unrecoverable(String),
    Ham(ham::Error),
    Timeout(String),
    Misc(String),
    #[cfg(feature = "bluetooth")]
    Bluetooth(rustable::Error),
}
impl From<ham::Error> for Error {
    fn from(err: ham::Error) -> Self {
        match err {
            ham::Error::Timeout(e) => Error::Timeout(format!("{:?}", e)),
            e => Error::Ham(e),
        }
    }
}
impl From<rustable::Error> for Error {
    fn from(err: rustable::Error) -> Self {
        match err {
            rustable::Error::Timeout => Error::Timeout("BLE timeout".to_string()),
            _ => Error::Bluetooth(err),
        }
    }
}
fn slice_to_u32<F>(buf: &[u8], bytes: usize, or_else: F) -> Result<u32, Error>
where
    F: FnOnce() -> Error,
{
    let bytes = bytes.min(4);
    let &u0 = buf.get(bytes - 1).ok_or_else(or_else)?;
    let mut ret = u0 as u32;
    let bytes = bytes - 1;
    for i in 0..bytes {
        ret += (buf[i] as u32) << ((bytes - i) * 8);
    }
    Ok(ret)
}
impl LedMsg {
    pub const MAX_LEN: usize = 1 + 2 + 4 + 1; // flags + color/elment + time + cmd_value
    fn deserialize(buf: &[u8]) -> Result<Vec<LedMsg>, Error> {
        let mut ret = Vec::new();
        if buf.len() == 0 {
            return Ok(Vec::new());
        } else if buf.len() < 4 {
            return Err(Error::BadInput(
                "Buffer is too short and doesn't contain time.".to_string(),
            ));
        }
        let time = ((buf[0] as u32) << 24)
            + ((buf[1] as u32) << 16)
            + ((buf[2] as u32) << 8)
            + buf[3] as u32;
        let mut i = 4;
        while i < buf.len() {
            let extra_bytes = || {
                Error::BadInput(format!(
                    "There was an extra {} bytes at the end of the buffer.",
                    buf.len() - i
                ))
            };
            if i + 2 >= buf.len() {
                return Err(extra_bytes());
            }
            let (cur_time, extra0) = match buf[i] >> 5 {
                0x00 => (time, 0),
                0x01 => (
                    time.wrapping_add(slice_to_u32(&buf[i + 3..], 1, extra_bytes)?),
                    1,
                ),
                0x02 => (
                    time.wrapping_add(slice_to_u32(&buf[i + 3..], 2, extra_bytes)?),
                    2,
                ),
                0x03 => (
                    time.wrapping_add(slice_to_u32(&buf[i + 3..], 3, extra_bytes)?),
                    3,
                ),
                0x04 => (slice_to_u32(&buf[i + 3..], 4, extra_bytes)?, 4),
                0x05 => (
                    time.wrapping_sub(slice_to_u32(&buf[i + 3..], 1, extra_bytes)?),
                    1,
                ),
                0x06 => (
                    time.wrapping_sub(slice_to_u32(&buf[i + 3..], 2, extra_bytes)?),
                    2,
                ),
                0x07 => (
                    time.wrapping_sub(slice_to_u32(&buf[i + 3..], 3, extra_bytes)?),
                    3,
                ),
                _ => unreachable!(),
            };
            let (cmd, extra1) = match (buf[i] >> 2) & 0x07 {
                0x00 => (Command::Null, 0),
                0x01 => (
                    Command::Flat(*buf.get(i + 3 + extra0).ok_or_else(extra_bytes)?),
                    1,
                ),
                0x02 => (
                    Command::PulseLinear(*buf.get(i + 3 + extra0).ok_or_else(extra_bytes)?),
                    1,
                ),
                0x03 => (
                    Command::PulseQuadratic(*buf.get(i + 3 + extra0).ok_or_else(extra_bytes)?),
                    1,
                ),
                0x04 => (
                    Command::FlatStack(*buf.get(i + 3 + extra0).ok_or_else(extra_bytes)?),
                    1,
                ),
                v => {
                    return Err(Error::BadInput(format!(
                        "Unknown command was given: {:#04X}",
                        v
                    )))
                }
            };
            let msg = LedMsg {
                cur_time,
                element: buf[i + 1],
                color: buf[i + 2],
                cmd,
            };
            ret.push(msg);
            i += 3 + extra0 + extra1;
        }
        Ok(ret)
    }
    fn serialize(msgs: &[LedMsg], ret: &mut [u8], time: Option<u32>) -> (usize, usize) {
        assert!(ret.len() >= 7);
		let time = match time {
			Some(t) => t,
			None => msgs[0].cur_time
		};
        ret[0..4].copy_from_slice(&time.to_be_bytes());
        let mut i = 4;
        for (j, msg) in msgs.iter().enumerate() {
            let mut buf = [0; 8];
            let diff = msg.cur_time.wrapping_sub(time);
            let (flag0, extra0) = if diff == 0 {
                ((0x0 << 5), 0)
            } else if diff < 256 {
                buf[3] = diff as u8;
                ((0x1 << 5), 1)
            } else if diff < 65536 {
                buf[3..5].copy_from_slice(&diff.to_be_bytes()[2..4]);
                ((0x02 << 5), 2)
            } else if diff < 16777216 {
                buf[3..6].copy_from_slice(&diff.to_be_bytes()[1..4]);
                ((0x03 << 5), 3)
            } else if diff < 2147483648 || diff >= 2147483648 + 16777216 {
                buf[3..7].copy_from_slice(&msg.cur_time.to_be_bytes());
                ((0x04 << 5), 4)
            } else if diff < 2147483648 + 256 {
                buf[3] = diff as u8;
                ((0x05 << 5), 1)
            } else if diff < 2147483648 + 65536 {
                buf[3..5].copy_from_slice(&diff.to_be_bytes()[2..4]);
                ((0x06 << 5), 2)
            } else {
                // implied: if diff < 2147483648 + 16777216, because of exhaustion
                buf[3..6].copy_from_slice(&diff.to_be_bytes()[1..4]);
                ((0x07 << 5), 3)
            };
            let (flag1, extra1) = match msg.cmd {
                Command::Null => (0x00 << 2, 0),
                Command::Flat(v) => {
                    buf[3 + extra0] = v;
                    (0x01 << 2, 1)
                }
                Command::PulseLinear(v) => {
                    buf[3 + extra0] = v;
                    (0x02 << 2, 1)
                }
                Command::PulseQuadratic(v) => {
                    buf[3 + extra0] = v;
                    (0x03 << 2, 1)
                }
                Command::FlatStack(v) => {
                    buf[3 + extra0] = v;
                    (0x04 << 2, 1)
                }
            };
            let msg_len = extra0 + extra1 + 3;
            if i + msg_len <= ret.len() {
                // we have enough room in the buffer so append
                buf[0] = flag0 | flag1;
                buf[1] = msg.element;
                buf[2] = msg.color;
                ret[i..i + msg_len].copy_from_slice(&buf[..msg_len]);
                i += msg_len; // iterate for the next buffer
            } else {
                // no room in buffer so discard last msg and return
                return (i, j);
            }
            if i + 3 > ret.len() {
                // we wont have any room for next message
                return (i, j + 1);
            }
        }
        (i, msgs.len())
    }
}
pub trait Receiver {
    fn cur_time(&self) -> u32;
    fn recv_to(&mut self, timeout: Duration) -> Result<Vec<LedMsg>, Error>;
    #[inline]
    fn try_recv(&mut self) -> Result<Vec<LedMsg>, Error> {
        self.recv_to(Duration::from_secs(0))
    }
    fn recv(&mut self) -> Result<Vec<LedMsg>, Error>;
    fn try_iter(&mut self) -> TryIter<'_, Self>
    where
        Self: Sized,
    {
        TryIter { recv: self }
    }
}
pub struct TryIter<'a, T: Receiver> {
    recv: &'a mut T,
}
impl<T: Receiver> Iterator for TryIter<'_, T> {
    type Item = Vec<LedMsg>;
    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.recv.try_recv().ok()
    }
}
/*
#[cfg(feature = "ham-xpt")]
pub struct HamReceiver<T: PacketReceiver> {
    ham: T,
}

impl<T: PacketReceiver> HamReceiver<T> {
    pub fn new<P>(pr: P) -> Result<Self, Error>
    where
        P: IntoPacketReceiver<Recv = T>,
    {
        let ham = pr.into_packet_receiver()?;
        Ok(HamReceiver { ham })
    }
}
*/

impl<T: PacketReceiver> Receiver for T {
    #[inline]
    fn cur_time(&self) -> u32 {
        self.cur_time()
    }
    #[inline]
    fn recv_to(&mut self, timeout: Duration) -> Result<Vec<LedMsg>, Error> {
        let msg = self.recv_pkt_to(timeout)?;
        LedMsg::deserialize(&msg)
    }
    #[inline]
    fn recv(&mut self) -> Result<Vec<LedMsg>, Error> {
        let msg = self.recv_pkt()?;
        LedMsg::deserialize(&msg)
    }
}

pub trait Sender {
    fn send(&mut self, msgs: &[LedMsg]) -> Result<(), Error>;
}
/*
pub struct HamSender<T: PacketSender> {
    ham: T,
}
*/

impl<T: PacketSender> Sender for T {
    fn send(&mut self, msgs: &[LedMsg]) -> Result<(), Error> {
        let first_msg_time = if let Some(msg) = msgs.get(0) {
            msg.cur_time
        } else {
            return Ok(());
        };
        let start = Instant::now();
        let mut i = 0;
        let mtu = self.mtu();

        while i < msgs.len() {
            let mut buf = vec![0; mtu];
            let (bytes, procs) = LedMsg::serialize(&msgs[i..], &mut buf, None);
            i += procs;
            buf.resize(bytes, 0);
            self.send_packet(
                &buf,
                first_msg_time
                    .wrapping_add(Instant::now().duration_since(start).as_micros() as u32),
            )?;
        }
        Ok(())
    }
}

pub struct LocalReceiver {
    last_inst: Instant,
    last_time: u32,
    recv: mpsc::Receiver<(Vec<LedMsg>, Instant, u32)>,
}

impl Receiver for LocalReceiver {
    #[inline]
    fn cur_time(&self) -> u32 {
        (Instant::now().duration_since(self.last_inst).as_micros() + self.last_time as u128) as u32
    }
    fn recv(&mut self) -> Result<Vec<LedMsg>, Error> {
        let msgs = self
            .recv
            .recv()
            .map_err(|_| Error::Unrecoverable("Sender has disconnected.".to_string()))?;
        self.last_inst = msgs.1;
        self.last_time = msgs.2;
        Ok(msgs.0)
    }
    fn recv_to(&mut self, timeout: Duration) -> Result<Vec<LedMsg>, Error> {
        let msgs = self.recv.recv_timeout(timeout).map_err(|e| match e {
            mpsc::RecvTimeoutError::Timeout => {
                Error::Timeout("LocalReceiver: recv timeout".to_string())
            }
            mpsc::RecvTimeoutError::Disconnected => {
                Error::Unrecoverable("LocalReceiver: senders disconnected".to_string())
            }
        })?;
        self.last_inst = msgs.1;
        self.last_time = msgs.2;
        Ok(msgs.0)
    }
}
pub struct LocalSender {
    sender: mpsc::SyncSender<(Vec<LedMsg>, Instant, u32)>,
}
impl Sender for LocalSender {
    #[inline]
    fn send(&mut self, msgs: &[LedMsg]) -> Result<(), Error> {
        let first_msg_time = if let Some(msg) = msgs.get(0) {
            msg.cur_time
        } else {
            return Ok(());
        };
        self.sender
            .send((Vec::from(msgs), Instant::now(), first_msg_time))
            .map_err(|_| Error::Unrecoverable("LocalSender: receiver disconnected".to_string()))
    }
}

pub fn channel(size: usize) -> (LocalSender, LocalReceiver) {
    let (sender, recv) = mpsc::sync_channel(size);
    let last_inst = Instant::now();
    (
        LocalSender { sender },
        LocalReceiver {
            last_inst,
            last_time: 0,
            recv,
        },
    )
}
