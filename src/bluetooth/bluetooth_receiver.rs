use super::{ecp_bufs, BMsg, BleOptions, Status, ECP_BUF1_BASE, ECP_UUID, parse_time_signal};
use crate::{Error, LedMsg, Receiver};
use nix::poll::{PollFd, PollFlags, poll};
use rustable::gatt::{CharFlags, Characteristic, NotifyPoller, Service, WriteType};
use rustable::interfaces::{BLUEZ_DEST, MANGAGED_OBJ_CALL, OBJ_MANAGER_IF_STR};
use rustable::{AdType, Advertisement, Bluetooth as BT};
use rustable::{Device, MAC, UUID};
use std::os::unix::io::RawFd;
use std::rc::Rc;
use std::sync::mpsc;
use std::thread::{sleep, spawn};
use std::time::{Duration, Instant};
use std::os::unix::io::AsRawFd;

struct Bluetooth {
    blue: BT,
    verbose: u8,
}

impl Bluetooth {
    fn new(blue_path: String, verbose: u8) -> Result<Self, Error> {
        let mut blue = BT::new("io.maves.ecp_receiver".to_string(), blue_path)?;
        blue.set_filter(None)?;
        blue.verbose = verbose.saturating_sub(1);
        let ret = Bluetooth { blue, verbose };
        Ok(ret)
    }
    fn find_any_device(&mut self, timeout: Duration) -> Result<MAC, Error> {
        // do initial check for service
        let ecp_uuid: Rc<str> = ECP_UUID.into();
        self.blue.discover_devices()?;
        for device_mac in self.blue.devices() {
            let device = self.blue.get_device(&device_mac).unwrap();
            if device.has_service(&ecp_uuid) {
                if device.connected() {
                    return Ok(device_mac);
                }
            }
        }
        // register advertisement and set discoverable true
        let mut adv = Advertisement::new(AdType::Peripheral, "ecp-device".to_string());
        let sec = timeout.as_secs().min(std::u16::MAX as u64) as u16;
        adv.duration = 2;
        adv.timeout = sec;
        adv.solicit_uuids = Vec::from(&[ecp_uuid.clone()][..]);
        self.blue.remove_all_adv()?;
        let ad_idx = match self.blue.start_adv(adv) {
            Ok(idx) => Some(idx),
            Err(err) => {
                eprintln!("Warning: failed to regster advertisement: {:?}", err);
                None
            }
        };
        self.blue.set_power(true)?;
        self.blue.set_discoverable(true)?;

        // init the im
        let tar = Instant::now() + timeout;
        let sleep_dur = Duration::from_secs(1);
        // perform a do-while loop checking for matching devices
        loop {
            self.blue.discover_devices()?;
            let devices = self.blue.devices();
            for device_mac in devices {
                let device = self.blue.get_device(&device_mac).unwrap();
                if device.has_service(&ecp_uuid) {
                    if device.connected() {
                        if let Some(idx) = ad_idx {
                            self.blue.remove_adv(idx).ok();
                        }
                        return Ok(device_mac);
                    }
                }
            }

            // do-while terminate check
            if tar.checked_duration_since(Instant::now()).is_none() {
                if let Some(idx) = ad_idx {
                    self.blue.remove_adv(idx)?;
                }
                return Err(Error::Timeout("Finding device timed out".to_string()));
            } else {
                let target = Instant::now() + sleep_dur;
                while target.checked_duration_since(Instant::now()).is_some() {
                    self.blue.process_requests()?;
                }
            }
        }
    }
    // Panics if the mac is non existent or ECP service is not avaliable on device
    fn collect_notify_fds(&mut self, mac: &MAC) -> Result<NotifyPoller, rustable::Error> {
        let mut device = if let Some(device) = self.blue.get_device(&mac) {
            device
        } else {
            unreachable!()
        };
        let ecp_uuid: UUID = ECP_UUID.into();
        let ecp_bufs = ecp_bufs();
        if let Some(mut ecp_service) = device.get_service(&ecp_uuid) {
            // let mut fds = Vec::with_capacity(10);
            let mut count = 0;
            let mut fds: [RawFd; 10] = [0; 10];
            for uuid in &ecp_bufs {
                if let Some(mut r_char) = ecp_service.get_char(uuid) {
                    match r_char.acquire_notify() {
                        Ok(fd) => {
                            fds[count] = fd;
                            count += 1
                        }
                        Err(e) => {
                            eprintln!("Error acquiring notify fd: {:?}", e);
                            break;
                        }
                    }
                } else {
                    break;
                }
            }
            if count == 10 {
                Ok(NotifyPoller::new(&fds))
            } else {
                Err(rustable::Error::DbusReqErr(format!(
                    "Failed to get notify for {}",
                    ecp_bufs[count]
                )))
            }
        } else {
            unreachable!()
        }
    }
    /*
    fn poll_for_msgs(&mut self) -> Result<Vec<LedMsg>, Error> {

    }*/
}

enum RecvMsg {
    LedMsgs(Vec<LedMsg>),
    Time(u32, Instant),
}
pub struct BluetoothReceiver {
    send_bmsg: mpsc::SyncSender<BMsg>,
    recv_msgs: mpsc::Receiver<RecvMsg>,
    handle: Status,
    time: u32,
    time_inst: Instant,
}

impl BluetoothReceiver {
    pub fn new(blue_path: String, options: BleOptions) -> Result<Self, Error> {
        let (send_bmsg, recv_bmsg) = mpsc::sync_channel(1);
        let (send_msgs, recv_msgs) = mpsc::sync_channel(1);
        let handle = Status::Running(spawn(move || {
            let mut blue = Bluetooth::new(blue_path, options.verbose)?;
            println!("Waiting for device to connect...");
            let to = Duration::from_secs(60);
            let ecp_uuid: UUID = ECP_UUID.into();
            let ecp_bufs = ecp_bufs();
            loop {
                let mac = loop {
                    match blue.find_any_device(to) {
                        Ok(mac) => break mac,
                        Err(e) => match e {
                            Error::Timeout(_) => (),
                            e => return Err(e),
                        },
                    }
                    for bmsg in recv_bmsg.try_iter() {
                        match bmsg {
                            BMsg::Alive => (),
                            BMsg::Terminate => return Ok(()),
                            _ => unreachable!(),
                        }
                    }
                    println!("Still waiting for device to connect...");
                };
                println!("Bluetooth device connected, starting message reception.");

                /*if verbose >= 3 {
                    eprintln!("Fds: {:?}", poller.fds)
                }*/
                // read initial time signal
                let mut device = match blue.blue.get_device(&mac) {
                    Some(dev) => dev,
                    None => continue,
                };
                let mut ecp_service = match device.get_service(&ecp_uuid) {
                    Some(serv) => serv,
                    None => continue,
                };
                let mut time_char = match ecp_service.get_char(&ecp_bufs[5]) {
                    Some(ch) => ch,
                    None => continue, // Add verbose error message
                };
                match time_char.read_wait() {
                    Ok(cv) => {
                        if cv.len() == 4 {
                            let now = Instant::now();
                            let time = parse_time_signal(&cv[..4]);
                            if let Err(_) = send_msgs.send(RecvMsg::Time(time, now)) {
                                return Err(Error::Unrecoverable(
                                    "Receiver is disconnected".to_string(),
                                ));
                            }
                        } else {
                            eprintln!(
                                "Expected time characteristic to be over length 4 (was {}).",
                                cv.len()
                            );
                            continue;
                        }
                    }
                    Err(e) => {
                        eprintln!("Error reading initial time characteristic: {:?}", e);
                        continue;
                    }
                }

                // acquire notify fds for characteristics
                if let Err(err) = time_char.acquire_notify() {
                    eprintln!(
                        "Error getting notification fd for time characteristic: {:?}",
                        err
                    );
                    continue;
                }
                let time_fd = time_char.get_notify_fd().unwrap();
                let msg_fd;
                match ecp_service.get_char(&ecp_bufs[0]) {
                    Some(mut ch) => {
                        if let Err(err) = ch.acquire_notify() {
                            eprintln!(
                                "Error getting notification fd for msg characteristic: {:?}",
                                err
                            );
                            continue;
                        }
                        msg_fd = ch.get_notify_fd().unwrap();
                    }
                    None => continue, //TODO: Add verbose error message
                }


                let blue_fd = blue.blue.as_raw_fd();
                let pollin = PollFlags::POLLIN;
                let mut polls = [PollFd::new(time_fd, pollin), PollFd::new(msg_fd, pollin), PollFd::new(blue_fd, pollin)];
                let wait = Duration::from_secs_f64(1.0 / 32.0).as_millis();

                // the stats data
                let target_dur = Duration::from_secs(options.stats.into());
                let stats_start_total = Instant::now();
                let mut stats_period_start = stats_start_total;
                let mut recv_pkts_cnt = 0;
                let mut recv_pkts_cnt_total = 0;
                let mut recv_bytes = 0;
                let mut recv_bytes_total = 0;

                // begin notification and render loop
                loop {
                    // check for incoming message from main thread
                    for bmsg in recv_bmsg.try_iter() {
                        match bmsg {
                            BMsg::Alive => (),
                            BMsg::Terminate => return Ok(()),
                            _ => unreachable!(),
                        }
                    }
                    if let Ok(i) = poll(&mut polls, wait as i32) {
                        if i > 0 {
                            if let Some(_) = polls[0].revents() {
                                let mut err_state = false;
                                let now = Instant::now();
                                // receive message from time signal
                                let mut device = match blue.blue.get_device(&mac) {
                                     Some(dev) => dev,
                                    None => break,
                                };
                                let mut ecp_service = match device.get_service(&ecp_uuid) {
                                    Some(serv) => serv,
                                    None => break,
                                };
                                let mut time_char = match ecp_service.get_char(&ecp_bufs[5]) {
                                // receive the most recent time from the sending device
                                    Some(time_char) => time_char,
                                    None => {
                                        err_state = true;
                                        break;
                                    }
                                };
                                let mut time_to_send = None;
                                loop {
                                    /* We only care about the latest value, and we want absolutely no delay
                                     * in the time, so we will loop repeatedly to get the latest value
                                     * without multiple iterations to be ASAP.
                                     */
                                    match time_char.try_get_notify() {
                                        Ok(value) => if value.len() == 4 {
                                            let time = parse_time_signal(value.as_slice());
                                            time_to_send = Some((time, now));
                                        },
                                        Err(err) => match err {
                                            rustable::Error::Timeout => break,
                                            err => {
                                                if options.verbose >= 1 {
                                                    eprintln!("Failed to retrieve notification from time characteristic: {:?}", err);
                                                }
                                                err_state = true;
                                                break;
                                            }
                                        },
                                    }
                                }
                                if err_state { break; }
                                if let Some((time, now)) = time_to_send {
                                    if let Err(_) = send_msgs.send(RecvMsg::Time(time, now)) {
                                        return Err(Error::Unrecoverable(
                                            "Receiver is disconnected!".to_string()
                                        ));
                                    }
                                }
                            }
                            if let Some(_) = polls[1].revents() {
                                let mut device = match blue.blue.get_device(&mac) {
                                     Some(dev) => dev,
                                    None => break,
                                };
                                let mut ecp_service = match device.get_service(&ecp_uuid) {
                                    Some(serv) => serv,
                                    None => break,
                                };
                                let mut msg_char = match ecp_service.get_char(&ecp_bufs[0]) {
                                    Some(msg_char) => msg_char,
                                    None => break,
                                };
                                let val = match msg_char.try_get_notify() {
                                    Ok(val) => val,
                                    Err(_) => break,

                                };
                                if val.len() >= 4 {
							 		if let None = msg_char.get_write_fd() {
								        if let Err(_) = msg_char.acquire_write() {
								    		break;
								    	}
								    }
								    if let Err(_) = msg_char.write(val[0..4].into(), WriteType::WithRes) {
									    break;
									}
							    }
                                let msgs = match LedMsg::deserialize(val.as_slice()) {
                                    Ok(recvd) => {
                                        if options.verbose >= 3 {
                                            eprintln!("Deserialized msgs: {:?}", recvd);
                                        }
                                        if options.stats != 0 {
                                            recv_pkts_cnt += 1;
                                            recv_pkts_cnt_total += 1;
                                            recv_bytes += val.len();
                                            recv_bytes_total += val.len();
                                        }
                                        recvd
                                    },
                                    Err(e) => {
                                        if options.verbose >= 3 {
                                            eprintln!(
                                                "LedMsgs failed to deserialize: {:?} for bytes:\n{:02x?}",
                                                e, &val
                                            );
                                        } else if options.verbose >= 2 {
                                            eprintln!("LedMsgs failed to deserialize: {:?}", e);
                                        }
                                        Vec::new()
                                    }
                                };
                                if msgs.len() != 0 {
                                    if options.verbose >= 3 {
                                        println!("LedMsgs to be send: {:?}", msgs);
                                    }
                                    if let Err(e) = send_msgs.try_send(RecvMsg::LedMsgs(msgs)) {
                                        if let mpsc::TrySendError::Disconnected(_) = e {
                                            return Err(Error::Unrecoverable(
                                                "Receiver is disconnected".to_string(),
                                            ));
                                        }
                                    }
                                }
                            }
                            // handle Dbus messages
                            if let Some(_) = polls[2].revents() {
                                blue.blue.process_requests()?;
                            }
                        }

                        // do statistic printing if enabled
                        if options.stats != 0 {
                            let now = Instant::now();
                            let since = now.duration_since(stats_period_start);
                            if since > target_dur {
                                let since_secs = since.as_secs_f64();
                                eprintln!("Receiving stats:\n\tPeriod throughput: {:.0} Bps, {:.1} Msgs/s, Avg size: {} bytes", recv_bytes as f64 / since_secs, recv_pkts_cnt as f64 / since_secs, recv_bytes.checked_div(recv_pkts_cnt).unwrap_or(0));
    
                                let since_secs_total =
                                    now.duration_since(stats_start_total).as_secs_f64();
                                eprintln!(
                                    "\tTotal throughput: {:.0} Bps, {:.1} Msgs/s, Avg size: {} bytes\n",
                                    recv_bytes_total as f64 / since_secs_total,
                                    recv_pkts_cnt_total as f64 / since_secs_total,
                                    recv_bytes_total.checked_div(recv_pkts_cnt_total).unwrap_or(0)
                                );
    
                                // reset period stats
                                stats_period_start = now;
                                recv_pkts_cnt = 0;
                                recv_bytes = 0;
                            }
                        }
                    }

                    if let Some(mut dev) = blue.blue.get_device(&mac) {
                        dev.forget_service(&ecp_uuid);
                    }
                }
            }
        }));
        let ret = BluetoothReceiver {
            send_bmsg,
            recv_msgs,
            handle,
            time: 0,
            time_inst: Instant::now(),
        };
        ret.is_alive();
        if ret.is_alive() {
            Ok(ret)
        } else {
            Err(ret.terminate().unwrap_err())
        }
    }
    fn is_alive(&self) -> bool {
        self.send_bmsg.send(BMsg::Alive).is_ok()
    }
    fn terminate(self) -> Result<(), Error> {
        self.send_bmsg.send(BMsg::Terminate).ok();
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

impl Receiver for BluetoothReceiver {
    fn cur_time(&self) -> u32 {
        self.time
            .wrapping_add(Instant::now().duration_since(self.time_inst).as_micros() as u32)
    }
    fn recv_to(&mut self, timeout: Duration) -> Result<Vec<LedMsg>, Error> {
        let target = Instant::now() + timeout;
        loop {
            match self
                .recv_msgs
                .recv_timeout(target.saturating_duration_since(Instant::now()))
            {
                Ok(recv_msg) => match recv_msg {
                    RecvMsg::LedMsgs(msgs) => return Ok(msgs),
                    RecvMsg::Time(time, inst) => {
                        self.time = time;
                        self.time_inst = inst;
                    }
                },
                Err(e) => {
                    return Err(match e {
                        mpsc::RecvTimeoutError::Timeout => Error::Timeout("".to_string()),
                        mpsc::RecvTimeoutError::Disconnected => match &self.handle {
                            Status::Running(_) => {
                                let mut handle = Status::Terminated;
                                std::mem::swap(&mut self.handle, &mut handle);
                                match handle {
                                    Status::Running(handle) => handle.join().unwrap().unwrap_err(),
                                    Status::Terminated => unreachable!(),
                                }
                            }
                            Status::Terminated => {
                                Error::Unrecoverable("Receiver thread is disconnected!".to_string())
                            }
                        },
                    })
                }
            }
        }
    }
    fn recv(&mut self) -> Result<Vec<LedMsg>, Error> {
        let dur = Duration::from_secs(std::u64::MAX);
        self.recv_to(dur)
    }
}
