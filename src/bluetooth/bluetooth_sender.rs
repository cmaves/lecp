use super::{ecp_buf1, ecp_bufs, ecp_uuid_rc, BMsg, Status, ECP_BUF1_BASE, ECP_UUID};
use crate::{Error, LedMsg, Sender};
use rustable::gatt::{
    CharFlags, CharValue, Characteristic, LocalCharBase, LocalServiceBase, Service,
};
use rustable::{Bluetooth as BT, Device, ToUUID, ValOrFn, UUID};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TryRecvError};
use std::thread::{sleep, spawn, JoinHandle};
use std::time::{Duration, Instant};

const ECP_TIME: &'static str = "79f4bb2c-7885-4584-8ef9-ae205b0eb345";

#[derive(Clone, Copy)]
pub struct BleSenderOptions {
	pub verbose: u8,
	pub stats: u16,
}
struct Bluetooth {
    blue: BT,
    time: Rc<Cell<u32>>,
    last_set: Rc<Cell<Instant>>,
    msgs: Rc<RefCell<[Option<LedMsg>; 256]>>,
}

impl Bluetooth {
    fn new(blue_path: String, verbose: u8) -> Result<Self, Error> {
        let mut blue = BT::new("io.maves.ecp_sender".to_string(), blue_path)?;
        blue.set_filter(None)?;
        blue.verbose = verbose;
        let mut ret = Bluetooth {
            blue,
            time: Rc::new(Cell::new(0)),
            last_set: Rc::new(Cell::new(Instant::now())),
            msgs: Rc::new(RefCell::new([None; 256])),
        };
        ret.init_service()?;
        Ok(ret)
    }

    fn init_service(&mut self) -> Result<(), Error> {
        let ecp_uuid = ECP_UUID.to_uuid();
        let mut sender_service = LocalServiceBase::new(&ecp_uuid, true);
        let mut flags = CharFlags::default();
        flags.broadcast = true;
        flags.read = true;
        flags.notify = true;
        let uuids = ecp_bufs();
        for uuid in &uuids {
            let mut base = LocalCharBase::new(uuid, flags);
            base.notify_fd_buf = Some(256);
            sender_service.add_char(base);
        }
        self.blue.add_service(sender_service)?;
        let mut sender_service = self.blue.get_service(ecp_uuid).unwrap();
        for (i, uuid) in uuids[1..5].iter().enumerate() {
            let rc_msgs = self.msgs.clone();
            let read_fn = move || {
                let start = i * 64;
                let end = start + 64;
                let mut cv = CharValue::new(512);
                let mut msgs = [LedMsg::default(); 64];
                let mut cnt = 0;
                let borrow = rc_msgs.borrow();
                let iter = borrow[start..end].iter().filter_map(|x| *x);
                for (dst, src) in msgs.iter_mut().zip(iter) {
                    *dst = src;
                    cnt += 1;
                }
                let (len, msgs_consumed) = LedMsg::serialize(&msgs[..cnt], cv.as_mut_slice());
                debug_assert_eq!(msgs_consumed, cnt);
                cv.resize(len, 0);
                cv
            };
            let mut ecp_char = sender_service.get_char(uuid).unwrap();
            ecp_char.write_val_or_fn(&mut ValOrFn::Function(Box::new(read_fn)));
        }
        let time = self.time.clone();
        let last_set = self.last_set.clone();
        let time_closure = move || {
            time_fn(time.get(), last_set.get(), Instant::now())
                .to_be_bytes()
                .as_ref()
                .into()
        };
        let mut time_serv = sender_service.get_char(&uuids[5]).unwrap();
        time_serv.write_val_or_fn(&mut ValOrFn::Function(Box::new(time_closure)));
        self.blue.register_application()?;
        Ok(())
    }
    fn cur_time(&self) -> u32 {
        self.time.get().wrapping_add(
            Instant::now()
                .duration_since(self.last_set.get())
                .as_micros() as u32,
        )
    }
    fn process_requests(&mut self) -> Result<(), Error> {
        self.blue.process_requests()?;
        Ok(())
    }
}
fn time_fn(time: u32, last_set: Instant, now: Instant) -> u32 {
    time.wrapping_add(now.duration_since(last_set).as_micros() as u32)
}
pub struct BluetoothSender {
    sender: SyncSender<BMsg>,
    handle: Status,
}
impl BluetoothSender {
    pub fn new(blue_path: String, options: BleSenderOptions) -> Result<Self, Error> {
        let (sender, recv) = sync_channel(1);
        let handle = Status::Running(spawn(move || {
            let mut bt = Bluetooth::new(blue_path, options.verbose)?;
            let ecp_bufs = ecp_bufs();
            let mut last_notify_time = Instant::now();
			

			// the stats data
			let target_dur = Duration::from_secs(options.stats.into());
			let stats_start_total = Instant::now();
			let mut stats_period_start = stats_start_total;
			let mut sent_pkts_cnt = 0;
			let mut sent_pkts_cnt_total = 0;
			let mut sent_bytes = 0;
			let mut sent_bytes_total = 0;
            loop {
                bt.process_requests()?;
                match recv.try_recv() {
                    Ok(msg) => match msg {
                        BMsg::SendMsg(msgs, start) => {
                            if msgs.len() == 0 {
                                continue;
                            }
                            let now = Instant::now();
                            let old_time = time_fn(bt.time.get(), bt.last_set.get(), now);
                            let cur_time = time_fn(msgs[0].cur_time, start, now);

                            bt.last_set.set(now);
                            bt.time.set(cur_time);
                            let notify_time = (cur_time.wrapping_sub(old_time) as i32).abs()
                                > 5_000
                                || now.duration_since(last_notify_time).as_millis() > 5_000;

                            let mut mut_msgs = bt.msgs.borrow_mut();
                            for msg in &msgs {
                                mut_msgs[msg.element as usize] = Some(*msg);
                            }
                            for msg in mut_msgs.iter_mut() {
                                // prune old messages
                                if let Some(v) = msg {
                                    if (cur_time.wrapping_sub(v.cur_time) as i32).abs() > 5_000_000
                                    {
                                        // the i32::abs allows values that are up to 5 seconds early to array
                                        *msg = None;
                                    }
                                }
                            }
                            drop(mut_msgs);

                            // eprintln!("dirty received: {:?}", dirty);
                            // write out the dirty characteristics and
                            let mut service = bt.blue.get_service(ECP_UUID).unwrap();
                            if notify_time {
                                let mut character = service.get_char(ECP_TIME).unwrap();
                                character.notify(None)?;
                                last_notify_time = now;
                            }
                            let mut notify_char = service.get_char(&ecp_bufs[0]).unwrap();
                            let mtu = notify_char.notify_mtu().unwrap_or(23) - 3; // The 3 accounts for ATT HDR
                            let mut written = 0;
                            while written < msgs.len() {
                                let mut cv = CharValue::new(mtu as usize);

                                let (len, consumed) =
                                    LedMsg::serialize(&msgs[written..], cv.as_mut_slice());
                                cv.resize(len, 0);
                                written += consumed;
                                notify_char.write_wait(cv)?;
                                if let Err(e) = notify_char.notify(None) {
                                    if let rustable::Error::Timeout = e {
                                    } else {
                                        return Err(e.into());
                                    }
                                }
								if options.stats != 0  {
									sent_pkts_cnt += 1;
									sent_pkts_cnt_total += 1;
									sent_bytes += len;
									sent_bytes_total += len;
								}
                            }
							if options.stats != 0 { 
								let now = Instant::now();
								let since = now.duration_since(stats_period_start);
								if since > target_dur {
									let since_secs = since.as_secs_f64();
									eprintln!("Sending stats:\n\tPeriod throughput: {:.0} Bps, {:.0} msgs, Avg size: {} bytes", sent_bytes as f64 / since_secs, sent_pkts_cnt as f64 / since_secs, sent_bytes / sent_pkts_cnt);

									let since_secs_total = now.duration_since(stats_start_total).as_secs_f64();

									eprintln!("\tTotal throughput: {:.0} Bps, {:.0} msgs, Avg size: {} bytes\n", sent_bytes_total as f64 / since_secs_total, sent_pkts_cnt_total as f64 / since_secs_total, sent_bytes_total / sent_pkts_cnt_total);

									// reset period stats
									stats_period_start = now;
									sent_pkts_cnt = 0;
									sent_bytes = 0;
								}
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
        ret.is_alive();
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
        self.sender.send(BMsg::Terminate).ok();
        match self.handle {
            Status::Running(handle) => match handle.join() {
                Ok(ret) => ret,
                Err(err) => Err(Error::Unrecoverable(format!(
                    "DBus bluetooth thread panicked with: {:?}",
                    err
                ))),
            },
            Status::Terminated => Err(Error::BadInput("Thread already terminated".to_string())),
        }
    }
}

impl Sender for BluetoothSender {
    fn send(&mut self, msgs: &[LedMsg]) -> Result<(), Error> {
        let start = Instant::now();
        let msg_vec = Vec::from(msgs);
        match self.sender.send(BMsg::SendMsg(msg_vec, start)) {
            Ok(()) => Ok(()),
            Err(_) => match self.handle {
                Status::Running(_) => {
                    let mut handle = Status::Terminated;
                    std::mem::swap(&mut handle, &mut self.handle);
                    match handle {
                        Status::Running(handle) => handle.join().unwrap(),
                        Status::Terminated => unreachable!(),
                    }
                }
                Status::Terminated => Err(Error::Unrecoverable(
                    "BluetoothSender: Sending thread is disconnected!".to_string(),
                )),
            },
        }
    }
}
