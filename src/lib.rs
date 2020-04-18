use std::convert::{Infallible, TryFrom};

#[cfg(feature = "controller")]
pub mod controller;

#[derive(Debug)]
struct LedMsg {
    cur_time: u32,
    element: u8,
    color: u8,
    cmd: Command,
}
pub enum Error {
    BadInput(String),
}

impl LedMsg {
    fn deserilize(buf: &[u8], time: u32) -> Result<Vec<LedMsg>, Error> {
        let mut ret = Vec::new();
        let mut i = 0;
        while i < buf.len() {
            let (offset, extra0) = match buf[i] >> 6 {
                0x00 => (0, 0),
                0x01 => (buf[i + 3] as u32, 1),
                0x02 => ((((buf[i + 3] as u32) << 8) + buf[i + 4] as u32), 2),
                0x03 => (
                    (((buf[i + 3] as u32) << 16) + ((buf[i + 4] as u32) << 8) + buf[i + 5] as u32),
                    3,
                ),
                _ => unreachable!(),
            };
            let (cmd, extra1) = match (buf[i] >> 4) & 0x03 {
                0x00 => (Command::Null, 0),
                0x01 => (Command::Flat(buf[i + 3 + extra0]), 1),
                0x02 => (Command::PulseLinear(buf[i + 3 + extra0]), 1),
                0x03 => (Command::PulseQuadratic(buf[i + 3 + extra0]), 1),
                _ => unreachable!(),
            };
            let msg = LedMsg {
                cur_time: time.wrapping_add(offset),
                element: buf[i + 1],
                color: buf[i + 2],
                cmd,
            };
            i += 3 + extra0 + extra1;
            ret.push(msg);
        }
        Ok(ret)
    }
    fn serialize(msgs: &[LedMsg], time: u32) -> Result<Vec<u8>, Error> {
        let mut ret = Vec::new();
        ret.extend_from_slice(&time.to_be_bytes());

        for msg in msgs.iter() {
            let mut buf = [0; 7];
            let diff = msg.cur_time - time;
            let (flag0, extra0) = if diff == 0 {
                (0x0 << 6, 0)
            } else if diff < 256 {
                buf[3] = diff as u8;
                (0x1 << 6, 1)
            } else if diff < 65536 {
                buf[3] = (diff >> 8) as u8;
                buf[4] = diff as u8;
                (0x02 << 6, 2)
            } else if diff < 16777216 {
                buf[3] = (diff >> 16) as u8;
                buf[4] = (diff >> 8) as u8;
                buf[5] = diff as u8;
                (0x03 << 6, 3)
            } else {
                return Err(Error::BadInput(format!("{:?}'s offset was too much to be offset from time diff ({}) must be < 16777216 us", msg, diff)));
            };
            let (flag1, extra1) = match msg.cmd {
                Command::Null => (0x00 << 4, 0),
                Command::Flat(v) => {
                    buf[3 + extra0] = v;
                    (0x01 << 4, 1)
                }
                Command::PulseLinear(v) => {
                    buf[3 + extra0] = v;
                    (0x02 << 4, 1)
                }
                Command::PulseQuadratic(v) => {
                    buf[3 + extra0] = v;
                    (0x03 << 4, 1)
                }
            };
            buf[0] = flag0 | flag1;
            buf[1] = msg.element;
            buf[2] = msg.color;
            ret.extend_from_slice(&buf[..3 + extra0 + extra1]);
        }
        Ok(ret)
    }
}

#[derive(Copy, Clone, Debug)]
enum Command {
    Null,
    Flat(u8),
    PulseLinear(u8),
    PulseQuadratic(u8),
}
impl From<Command> for (u8, u8) {
    fn from(cmd: Command) -> (u8, u8) {
        match cmd {
            Command::Flat(v) => (0x00, v),
            Command::PulseLinear(v) => (0x01, v),
            Command::PulseQuadratic(v) => (0x02, v),
            _ => unimplemented!(),
        }
    }
}
impl TryFrom<(u8, u8)> for Command {
    type Error = Infallible;
    fn try_from(pair: (u8, u8)) -> Result<Command, Infallible> {
        match pair.0 {
            0x00 => Ok(Command::Flat(pair.1)),
            0x01 => Ok(Command::PulseLinear(pair.1)),
            0x02 => Ok(Command::PulseQuadratic(pair.1)),
            _ => unimplemented!(),
        }
    }
}
