use rustable::UUID;

mod bluetooth_receiver;
mod bluetooth_sender;

pub use bluetooth_receiver::BluetoothReceiver;
pub use bluetooth_sender::BluetoothSender;

const ECP_UUID: UUID = UUID(0x8a33385f446547aaa25a3631f01d4861);
