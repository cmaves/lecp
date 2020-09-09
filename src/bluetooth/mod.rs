use crate::{Error, LedMsg};
use rustable::UUID;
use std::rc::Rc;
use std::thread::JoinHandle;
use std::time::Instant;

mod bluetooth_receiver;
mod bluetooth_sender;

pub use bluetooth_receiver::BluetoothReceiver;
pub use bluetooth_sender::BluetoothSender;

const ECP_UUID: &'static str = "8a33385f-4465-47aa-a25a-3631f01d4861";
const ECP_BUF1_BASE: &'static str = "79f4bb2c-7885-4584-8ef9-ae205b0eb340";

#[derive(Clone, Copy)]
pub struct BleOptions {
    pub verbose: u8,
    pub stats: u16,
}

fn parse_time_signal(v: &[u8]) -> u32 {
    let mut bytes = [0; 4];
    bytes.copy_from_slice(v);
    u32::from_be_bytes(bytes)
}

fn ecp_uuid_rc() -> Rc<str> {
    ECP_UUID.into()
}
enum Status {
    Running(JoinHandle<Result<(), Error>>),
    Terminated,
}
enum BMsg {
    SendMsg(Vec<LedMsg>, Instant),
    Alive,
    Terminate,
}

fn ecp_bufs() -> [UUID; 6] {
    let mut ret = [
        "".into(),
        "".into(),
        "".into(),
        "".into(),
        "".into(),
        "".into(),
    ];
    for (i, v) in ret.iter_mut().enumerate() {
        *v = ecp_buf1(i as u8);
    }
    ret
}
fn ecp_buf1(u: u8) -> UUID {
    debug_assert!(u < 16);
    format!("{}{:x}", &ECP_BUF1_BASE[..35], u).into()
}
