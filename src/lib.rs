#[cfg(feature = "controller")]
pub mod controller;

pub mod color;

#[cfg(test)]
pub mod tests;

use ham::{IntoPacketReceiver, PacketReceiver, PacketSender};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LedMsg {
    cur_time: u32,
    element: u8,
    color: u8,
    cmd: Command,
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
    Ham(ham::Error),
}
impl From<ham::Error> for Error {
    fn from(err: ham::Error) -> Self {
        Error::Ham(err)
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
        if buf.len() > 0 && buf.len() < 4 {
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

                _ => unreachable!(),
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
    fn serialize(msgs: &[LedMsg], ret: &mut [u8]) -> (usize, usize) {
        assert!(ret.len() >= 12);
        let time = match msgs.get(0) {
            Some(msg) => msg.cur_time,
            None => 0,
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
            if i + msg_len < ret.len() {
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

impl<T: PacketReceiver> Receiver for HamReceiver<T> {
    #[inline]
    fn cur_time(&self) -> u32 {
        self.ham.cur_time()
    }
    #[inline]
    fn recv_to(&mut self, timeout: Duration) -> Result<Vec<LedMsg>, Error> {
        let msg = self.ham.recv_pkt_to(timeout)?;
        LedMsg::deserialize(&msg)
    }
    #[inline]
    fn recv(&mut self) -> Result<Vec<LedMsg>, Error> {
        let msg = self.ham.recv_pkt()?;
        LedMsg::deserialize(&msg)
    }
}

pub trait Sender {
    fn send(&mut self, msgs: &[LedMsg]) -> Result<(), Error>;
}
pub struct HamSender<T: PacketSender> {
    ham: T,
}

impl<T: PacketSender> Sender for HamSender<T> {
    fn send(&mut self, msgs: &[LedMsg]) -> Result<(), Error> {
        let start = Instant::now();
        let mut i = 0;
        let mtu = self.ham.mtu();
        let first_msg_time = if let Some(msg) = msgs.get(0) {
            msg.cur_time
        } else {
            return Ok(());
        };
        while i < msgs.len() {
            let mut buf = vec![0; mtu];
            let (bytes, procs) = LedMsg::serialize(&msgs[i..], &mut buf);
            i += procs;
            buf.resize(bytes, 0);
            self.ham.send_packet(
                &buf,
                first_msg_time
                    .wrapping_add(Instant::now().duration_since(start).as_micros() as u32),
            )?;
        }
        Ok(())
    }
}
