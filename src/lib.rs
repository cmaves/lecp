//! # Lightning Element Control Protocol
//! This crate is used to remotely control LED elements over a variety of
//! transmission mechanisms.
//! Elements are controlled via messages that are serialized in to a byte-stream
//! and deserialized at the receiver which then controls the lights using GPIO pins.
//!
//! ## Current implemented control mechanisms
//! - Bluetooth Low Energy with **bluetooth** feature.
//! - RFM69HCW packet radio with **ham** feature.

pub mod color;
pub mod controller;

#[cfg(feature = "bluetooth")]
pub mod bluetooth;

#[cfg(test)]
pub mod tests;

// use ham::{PacketReceiver, PacketSender};
use std::convert::TryFrom;
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// `LedMsg` is the message format used control LEDs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LedMsg {
    /// The current time in microseconds from an arbitrary point in time.
    pub time: u64,
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
            time: 0,
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
    #[cfg(feature = "bluetooth")]
    BtTiming(btutils::timing::Error),
    #[cfg(feature = "bluetooth")]
    BtMsg(btutils::messaging::Error),
    NotConnected,
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
        Error::Bluetooth(err)
    }
}
impl From<btutils::timing::Error> for Error {
    fn from(err: btutils::timing::Error) -> Self {
        Error::BtTiming(err)
    }
}
impl From<btutils::messaging::Error> for Error {
    fn from(err: btutils::messaging::Error) -> Self {
        Error::BtMsg(err)
    }
}
const U32_MAX: u64 = std::u32::MAX as u64;

impl LedMsg {
    pub const MAX_LEN: usize = 1 + 2 + 4 + 1; // flags + color/elment + time + cmd_value
    fn deserialize(buf: &[u8], cur_time: u64) -> Result<Vec<LedMsg>, Error> {
        let mut ret = Vec::new();
        if buf.is_empty() {
            return Ok(Vec::new());
        } else if buf.len() < 4 {
            return Err(Error::BadInput(
                "Buffer is too short and doesn't contain time.".to_string(),
            ));
        }
        let mut time_buf = [0; 8];
        time_buf[..4].copy_from_slice(&buf[..4]);
        let msg_time = u64::from_le_bytes(time_buf);
        let mask = cur_time & !U32_MAX; // get only the highest sig bytes
                                        // Calculate the possible msg times and select closest to actual time.
        let mut time = msg_time | mask;
        let abs_diff = (cur_time.wrapping_sub(time) as i64).abs();

        let (alt_diff, alt_pos) = if cur_time & U32_MAX >= 2u64.pow(31) {
            let after = mask.wrapping_add(U32_MAX + 1);
            let after_pos = msg_time & after;
            ((cur_time.wrapping_sub(after_pos) as i64).abs(), after_pos)
        } else {
            let before = mask.wrapping_sub(U32_MAX + 1);
            let before_pos = msg_time & before;
            ((cur_time.wrapping_sub(before_pos) as i64).abs(), before_pos)
        };
        if alt_diff < abs_diff {
            time = alt_pos;
        }

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
            let (offset, extra0) = match buf[i] >> 6 {
                0x00 => (0, 0),
                0x01 => (buf[i + 3] as i8 as i32, 1),
                0x02 => {
                    let mut i_buf = [0; 2];
                    i_buf.copy_from_slice(&buf[i + 3..i + 5]);
                    (i16::from_le_bytes(i_buf) as i32, 2)
                }
                0x03 => {
                    let mut i_buf = [0; 4];
                    i_buf.copy_from_slice(&buf[i + 3..i + 7]);
                    (i32::from_le_bytes(i_buf), 4)
                }
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
            let time = time.wrapping_add(offset as u64);
            let msg = LedMsg {
                time,
                element: buf[i + 1],
                color: buf[i + 2],
                cmd,
            };
            ret.push(msg);
            i += 3 + extra0 + extra1;
        }
        Ok(ret)
    }
    fn serialize(msgs: &[LedMsg], ret: &mut [u8], cur_time: u64) -> (usize, usize) {
        assert!(ret.len() >= 7);
        ret[0..4].copy_from_slice(&cur_time.to_le_bytes()[0..4]);
        let mut i = 4;
        for (j, msg) in msgs.iter().enumerate() {
            let mut buf = [0u8; 8];
            let offset = msg.time.wrapping_sub(cur_time) as i64;
            let (flag0, extra0) = if offset == 0 {
                ((0x0 << 6), 0)
            } else if let Ok(off) = i8::try_from(offset) {
                buf[3] = off as u8;
                ((0x1 << 6), 1)
            } else if let Ok(off) = i16::try_from(offset) {
                buf[3..5].copy_from_slice(&off.to_le_bytes()[..]);
                ((0x02 << 6), 2)
            } else if let Ok(off) = i32::try_from(offset) {
                buf[3..7].copy_from_slice(&off.to_le_bytes()[..]);
                ((0x03 << 6), 4)
            } else {
                // messages outside the interval are ignored
                continue;
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
                return (j, i);
            }
            if i + 3 > ret.len() {
                // we wont have any room for next message
                return (j + 1, i);
            }
        }
        (msgs.len(), i)
    }
}
pub trait Receiver {
    fn cur_time(&self) -> u64;
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
/*
impl<T: PacketReceiver> Receiver for T {
    #[inline]
    fn cur_time(&self) -> u32 {
        self.cur_time()
    }
    #[inline]
    fn recv_to(&mut self, timeout: Duration) -> Result<Vec<LedMsg>, Error> {
        let msg = self.recv_pkt_to(timeout)?;
        LedMsg::deserialize(&msg self.cur_time())
    }
    #[inline]
    fn recv(&mut self) -> Result<Vec<LedMsg>, Error> {
        let msg = self.recv_pkt()?;
        LedMsg::deserialize(&msg)
    }
}
*/

pub trait Sender {
    fn send(&mut self, msgs: &mut [LedMsg], is_time_offset: bool) -> Result<(), Error>;
    fn get_time(&self) -> u64;
}
/*
pub struct HamSender<T: PacketSender> {
    ham: T,
}
*/
/*
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
}*/

pub struct LocalReceiver {
    start: Instant,
    recv: mpsc::Receiver<Vec<LedMsg>>,
}

impl Receiver for LocalReceiver {
    #[inline]
    fn cur_time(&self) -> u64 {
        self.start.elapsed().as_micros() as u64
    }
    fn recv(&mut self) -> Result<Vec<LedMsg>, Error> {
        let msgs = self
            .recv
            .recv()
            .map_err(|_| Error::Unrecoverable("Sender has disconnected.".to_string()))?;
        Ok(msgs)
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
        Ok(msgs)
    }
}

pub struct LocalSender {
    start: Instant,
    sender: mpsc::SyncSender<Vec<LedMsg>>,
}
impl Sender for LocalSender {
    #[inline]
    fn send(&mut self, msgs: &mut [LedMsg], is_msg_offset: bool) -> Result<(), Error> {
        if is_msg_offset {
            let cur_time = self.get_time();
            for msg in msgs.iter_mut() {
                msg.time = cur_time.wrapping_add(msg.time);
            }
        }
        self.sender
            .send(Vec::from(msgs))
            .map_err(|_| Error::Unrecoverable("LocalSender: receiver disconnected".to_string()))
    }
    fn get_time(&self) -> u64 {
        self.start.elapsed().as_micros() as u64
    }
}

pub fn channel(size: usize) -> (LocalSender, LocalReceiver) {
    let (sender, recv) = mpsc::sync_channel(size);
    let start = Instant::now();
    (LocalSender { start, sender }, LocalReceiver { start, recv })
}
