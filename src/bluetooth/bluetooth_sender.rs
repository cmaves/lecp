use super::{ecp_buf1, ecp_bufs, ecp_uuid_rc, BMsg, ECP_BUF1_BASE, ECP_UUID};
use crate::{Error, LedMsg, Sender};
use rustable::gatt::{CharFlags, Characteristic, LocalCharBase, LocalServiceBase, Service};
use rustable::{Bluetooth as BT, Device, ValOrFn, UUID};
use std::rc::Rc;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TryRecvError};
use std::thread::{sleep, spawn, JoinHandle};
use std::time::{Duration, Instant};

const ECP_TIME: &'static str = "79f4bb2c-7885-4584-8ef9-ae205b0eb349";

struct Bluetooth<'a, 'b> {
    blue: BT<'a, 'b>,
    time: u32,
    last_set: Instant,
    msgs: [Option<LedMsg>; 256],
}

impl Bluetooth<'_, '_> {
    fn new(blue_path: String, verbose: u8) -> Result<Self, Error> {
        let mut blue = BT::new("ecp", blue_path)?;
        blue.verbose = verbose;
        let mut ret = Bluetooth {
            blue,
            time: 0,
            last_set: Instant::now(),
            msgs: [None; 256],
        };
        ret.blue.filter_dest = None;
        ret.init_service()?;
        Ok(ret)
    }

    fn init_service(&mut self) -> Result<(), Error> {
        let mut sender_service = LocalServiceBase::new(ECP_UUID, true);
        let mut flags = CharFlags::default();
        flags.broadcast = true;
        flags.read = true;
        flags.notify = true;
        flags.indicate = false;
        for uuid in &ecp_bufs() {
            sender_service.add_char(LocalCharBase::new(uuid, flags));
        }
        self.blue.add_service(sender_service)?;
        self.blue.register_application()?;
        Ok(())
    }
    fn cur_time(&self) -> u32 {
        self.time
            .wrapping_add(Instant::now().duration_since(self.last_set).as_micros() as u32)
    }
    fn process_requests(&mut self) -> Result<(), Error> {
        self.blue.process_requests()?;
        Ok(())
    }
}
enum Status {
	Running(JoinHandle<Result<(), Error>>),
	Terminated
}
pub struct BluetoothSender {
    sender: SyncSender<BMsg>,
    handle: Status,
}
impl BluetoothSender {
    pub fn new(blue_path: String, verbose: u8) -> Result<Self, Error> {
        let (sender, recv) = sync_channel(1);
        let handle = Status::Running(spawn(move || {
            let mut bt = Bluetooth::new(blue_path, verbose)?;
			let ecp_bufs = ecp_bufs();
            loop {
                bt.process_requests()?;
                match recv.try_recv() {
                    Ok(msg) => match msg {
                        BMsg::SendMsg(msgs, start) => {
                            if msgs.len() == 0 {
                                continue;
                            }
                            bt.last_set = Instant::now();
                            bt.time = msgs[0]
                                .cur_time
                                .wrapping_add(bt.last_set.duration_since(start).as_micros() as u32);
                            let mut dirty = [false; 9]; // to keep track which characteristics need to be updated
                            for msg in msgs {
                                bt.msgs[msg.element as usize] = Some(msg);
                                dirty[msg.element as usize / 31] = true;
                            }
                            for msg in bt.msgs.iter_mut() {
                                // prune old messages
                                if let Some(v) = msg {
                                    if (bt.time.wrapping_sub(v.cur_time) as i32).abs() > 5_000_000 {
                                        // the i32::abs allows values that are up to 5 seconds early to array
                                        *msg = None;
                                    }
                                }
                            }

							// eprintln!("dirty received: {:?}", dirty);
                            // write out the dirty characteristics and
                            let mut service = bt.blue.get_service(ECP_UUID).unwrap();
                            for (i, &d) in dirty.iter().enumerate() {
                                if d {
                                    let mut msgs = [LedMsg::default(); 31];
                                    let (start, end) = (i * 31, (i + 1) * 31);
                                    let mut count = 0;
									//eprintln!("bt.msgs[start..end]: {:?}", &bt.msgs[start..end]);
                                    for msg in &bt.msgs[start..end] {
                                        if let Some(msg) = msg {
                                            msgs[count] = *msg;
                                            count += 1;
                                        }
                                    }
									//eprintln!("msgs[..count]: {:?}", &msgs[..count]);
                                    let mut buf = [0; 255];
                                    let (len, _) = LedMsg::serialize(&msgs[..count], &mut buf);
									//eprintln!("buf[..len]: {:?}", &buf[..len]);
									//eprintln!("ecp_bufs[i]: {:?}", &ecp_bufs[i]);
                                    let mut character =
                                        service.get_char(&ecp_bufs[i]).unwrap();
                                    character.write(&buf[..len])?;
									character.notify()?;
                                }
                            }
                            let cur_time = bt.time;
                            let last_set = bt.last_set;
                            let time_fn = move || {
                                let mut buf = [0; 255];
                                buf[..4].copy_from_slice(
                                    &cur_time
                                        .wrapping_add(
                                            Instant::now().duration_since(last_set).as_micros()
                                                as u32,
                                        )
                                        .to_be_bytes(),
                                );
                                (buf, 4)
                            };
                            let mut character = service.get_char(ECP_TIME).unwrap();
                            let (new_time, new_len) = time_fn();
                            let mut val = ValOrFn::Function(Box::new(time_fn));
                            character.write_val_or_fn(&mut val);
                            let (old_time, old_len) = val.to_value();
                            if old_len == 4 {
                                debug_assert_eq!(4, new_len);
                                let mut new_buf = [0; 4];
                                let mut old_buf = [0; 4];
                                new_buf.copy_from_slice(&new_time[..4]);
                                old_buf.copy_from_slice(&old_time[..4]);
                                let new_time = i32::from_be_bytes(new_buf);
                                let old_time = i32::from_be_bytes(old_buf);
                                if new_time.wrapping_sub(old_time).abs() > 5000 {
                                    character.notify()?;
                                }
                            } else {
                                character.notify()?;
                            }
                        }
                        BMsg::Terminate => return Ok(()),
                        BMsg::Alive => (),
                    },
                    Err(e) => {
                        if let TryRecvError::Disconnected = e {
                            return Err(Error::Unrecoverable(
                                "BT sender thread: Msg channel disconnected! exiting..."
                                    .to_string(),
                            ));
                        }
                    }
                }
            }
        }));
        sleep(Duration::from_millis(500));
        let ret = BluetoothSender { sender, handle };
        if ret.is_alive() {
            Ok(ret)
        } else {
            Err(ret.terminate().unwrap_err())
        }
    }
    pub fn is_alive(&self) -> bool {
        self.sender.send(BMsg::Alive).is_ok()
    }
    pub fn terminate(self) -> Result<(), Error> {
        self.sender.send(BMsg::Terminate);
		match self.handle {
        	Status::Running(handle) =>match handle.join() {
				Ok(_) => Ok(()),
            	Err(err) => Err(Error::Unrecoverable(format!("DBus bluetooth thread panicked with: {:?}", err)))
        	},
			Status::Terminated => Err(Error::BadInput("Thread already terminated".to_string()))
		}
    }
}

impl Sender for BluetoothSender {
    fn send(&mut self, msgs: &[LedMsg]) -> Result<(), Error> {
        let start = Instant::now();
        let msg_vec = Vec::from(msgs);
        match self.sender.send(BMsg::SendMsg(msg_vec, start)) {
			Ok(()) => Ok(()),
			Err(_) =>  {
				match self.handle {
					Status::Running(_) => {
						let mut handle = Status::Terminated;
						std::mem::swap(&mut handle, &mut self.handle);
						match handle {
							Status::Running(handle) => handle.join().unwrap(),
							Status::Terminated => unreachable!()
						}
					},
                	Status::Terminated => Err(Error::Unrecoverable("BluetoothSender: Sending thread is disconnected!".to_string()))
				}
			}
		}
	}
}
