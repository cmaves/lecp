
use crate::LedMsg;
use std::rc::Rc;
use std::time::Instant;

mod bluetooth_receiver;
mod bluetooth_sender;

pub use bluetooth_sender::BluetoothSender;
pub use bluetooth_receiver::BluetoothReceiver;

const ECP_UUID: &'static str = "8a33385f-4465-47aa-a25a-3631f01d4861";

fn ecp_uuid_rc() -> Rc<str> {
	ECP_UUID.into()
}

enum BMsg {
    SendMsg(Vec<LedMsg>, Instant),
    Alive,
    Terminate,
}

