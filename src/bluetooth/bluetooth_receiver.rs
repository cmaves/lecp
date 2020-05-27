

use rustable::{Bluetooth as BT};
use rustable::interfaces::{MANGAGED_OBJ_CALL, OBJ_MANAGER_IF_STR, BLUEZ_DEST};
use rustable::gatt::{Charactersitic, Service, CharFlags};
use crate::{Error, LedMsg};
use std::sync::mpsc;
use std::thread::{spawn, JoinHandle};
use super::BMsg;


struct Bluetooth<'a, 'b> {
	blue: BT<'a, 'b>,
	verbose: u8
}


impl Bluetooth<'_, '_> {
    fn new(blue_path: String, verbose: u8) -> Result<Self, Error> {
		let mut blue = BT::new("ecp_recv", blue_path)?;
		blue.verbose = verbose.saturating_sub(1);
		let ret = Bluetooth {
			blue,
			verbose
		};
		
		Ok(ret)
	}
	fn find_device_init(&mut self) {
		//let msg = MessageBuilder::new().call(MANGAGED_OBJ_CALL.to_string()).on("/".to_string()).with_interface(OBJ_MANAGER_IF_STR.to_string()).at(BLUEZ_DEST.to_string()).build();
		

		unimplemented!()
	}
}

pub struct BluetoothReceiver {
	send_bmsg: mpsc::SyncSender<BMsg>,
	recv_msgs: mpsc::Receiver<Vec<LedMsg>>,
    handle: JoinHandle<Result<(), Error>>,
}


impl BluetoothReceiver {
	pub fn new(blue_path: String, verbose: u8) -> Result<Self, Error> {
		let (send_bmsg, recv_bmsg) = mpsc::sync_channel(1);
		let (send_msgs, recv_msgs) = mpsc::sync_channel(1);
		let handle = spawn(move || {
			let mut blue = Bluetooth::new(blue_path, verbose)?;
			
			unimplemented!()	
		});
		let ret = BluetoothReceiver {
			send_bmsg,
			recv_msgs,
			handle
		};
		Ok(ret)
	}
}
