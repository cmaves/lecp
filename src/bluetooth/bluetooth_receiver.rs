use super::{ecp_bufs, BMsg, Status, ECP_BUF1_BASE, ECP_UUID};
use crate::{Error, LedMsg, Receiver};
use nix::poll::PollFlags;
use rustable::gatt::{CharFlags, Characteristic, NotifyPoller, Service};
use rustable::interfaces::{BLUEZ_DEST, MANGAGED_OBJ_CALL, OBJ_MANAGER_IF_STR};
use rustable::{AdType, Advertisement, Bluetooth as BT};
use rustable::{Device, MAC, UUID};
use std::os::unix::io::RawFd;
use std::rc::Rc;
use std::sync::mpsc;
use std::thread::{sleep, spawn};
use std::time::{Duration, Instant};

struct Bluetooth {
    blue: BT,
    verbose: u8,
}

fn parse_time_signal(v: &[u8]) -> u32 {
    let mut bytes = [0; 4];
    bytes.copy_from_slice(v);
    u32::from_be_bytes(bytes)
}
impl Bluetooth {
    fn new(blue_path: String, verbose: u8) -> Result<Self, Error> {
        let mut blue = BT::new("ecp_recv", blue_path)?;
        blue.verbose = verbose.saturating_sub(1);
        let mut ret = Bluetooth { blue, verbose };
        ret.blue.filter_dest = None;
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
        adv.duration = sec;
        adv.timeout = sec;
        adv.solicit_uuids = Vec::from(&[ecp_uuid.clone()][..]);
        let ad_idx = self.blue.start_advertise(adv).ok();
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
                            self.blue.remove_advertise_no_dbus(idx).ok();
                        }
                        return Ok(device_mac);
                    }
                }
            }

            // do-while terminate check
            if tar.checked_duration_since(Instant::now()).is_none() {
                if let Some(idx) = ad_idx {
                    self.blue.remove_advertise_no_dbus(idx).ok();
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
    pub fn new(blue_path: String, verbose: u8) -> Result<Self, Error> {
        let (send_bmsg, recv_bmsg) = mpsc::sync_channel(1);
        let (send_msgs, recv_msgs) = mpsc::sync_channel(1);
        let handle = Status::Running(spawn(move || {
            let mut blue = Bluetooth::new(blue_path, verbose)?;
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
                let mut poller = match blue.collect_notify_fds(&mac) {
                    Ok(p) => p,
                    Err(e) => {
                        if verbose >= 1 {
                            eprintln!("Error occurred acquiring notify file descriptors: {:?}", e);
                        }
                        continue;
                    }
                };

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
                let mut time_char = match ecp_service.get_char(&ecp_bufs[9]) {
                    Some(ch) => ch,
                    None => continue,
                };
                match time_char.read() {
                    Ok((v, l)) => {
                        if l == 4 {
                            let now = Instant::now();
                            let time = parse_time_signal(&v[..4]);
                            if let Err(_) = send_msgs.send(RecvMsg::Time(time, now)) {
                                return Err(Error::Unrecoverable(
                                    "Receiver is disconnected".to_string(),
                                ));
                            }
                        } else {
                            eprintln!(
                                "Expected time characteristic to be over length 4 (was {}).",
                                l
                            );
                            continue;
                        }
                    }
                    Err(e) => {
                        eprintln!("Error reading initial time characteristic: {:?}", e);
                        continue;
                    }
                }
                // loop over incoming notification and continue to
                let zero = Duration::from_secs(0);
                loop {
                    for bmsg in recv_bmsg.try_iter() {
                        match bmsg {
                            BMsg::Alive => (),
                            BMsg::Terminate => return Ok(()),
                            _ => unreachable!(),
                        }
                    }
                    blue.blue.process_requests()?;
                    let mut device = match blue.blue.get_device(&mac) {
                        Some(dev) => dev,
                        None => break,
                    };
                    let mut ecp_service = match device.get_service(&ecp_uuid) {
                        Some(serv) => serv,
                        None => break,
                    };
                    if let Err(_) = poller.poll(Some(zero)) {
                        eprintln!("Polling error getting next device.");
                        break;
                    }

                    let ready = poller.get_ready();
                    if verbose >= 3 {
                        eprintln!("Ready fds: {:?}", ready);
                    }
                    let mut msgs = Vec::new();
                    let mut err_state = false;
                    for &fd_idx in ready {
                        if poller
                            .get_flags(fd_idx)
                            .unwrap()
                            .contains(PollFlags::POLLERR)
                        {
                            println!(
                                "Notify file descriptor for {} is in error state.",
                                ecp_bufs[fd_idx]
                            );
                            err_state = true;
                            break;
                        }
                        if poller
                            .get_flags(fd_idx)
                            .unwrap()
                            .contains(PollFlags::POLLHUP)
                        {
                            println!(
                                "Notify file descriptor for {} has hung up.",
                                ecp_bufs[fd_idx]
                            );
                            err_state = true;
                            break;
                        }
                        let (v, l) = ecp_service
                            .get_char(&ecp_bufs[fd_idx])
                            .unwrap()
                            .try_get_notify()
                            .unwrap()
                            .unwrap();
                        if fd_idx == 9 {
                            // time signal
                            let now = Instant::now();
                            if l == 4 {
                                let time = parse_time_signal(&v[..4]);
                                if let Err(_) = send_msgs.send(RecvMsg::Time(time, now)) {
                                    return Err(Error::Unrecoverable(
                                        "Receiver is disconnected".to_string(),
                                    ));
                                }
                            }
                        } else {
                            // normal signal
                            let offset = (31 * fd_idx) as u8;
                            match LedMsg::deserialize(&v[..l]) {
                                Ok(received) => {
                                    let len = received.len();
                                    for mut msg in received {
                                        match msg.element.checked_add(offset) {
                                            Some(val) => msg.element = val,
                                            None => {
                                                println!(
                                                    "Received to many msgs from characteristic {}!",
                                                    ecp_bufs[fd_idx]
                                                );
                                                err_state = true;
                                                break;
                                            }
                                        }
                                        msgs.push(msg);
                                    }
                                    if verbose >= 3 {
                                        eprintln!(
                                            "Deserialized msgs: {:?}",
                                            &msgs[msgs.len() - len..]
                                        );
                                    }
                                }
                                Err(e) => {
                                    if verbose >= 3 {
                                        eprintln!("LedMsgs failed to deserialize: {:?} for bytes:\n{:02x?}", e, &v[..l]);
                                    } else if verbose >= 2 {
                                        eprintln!("LedMsgs failed to deserialize: {:?}", e);
                                    }
                                }
                            }
                        }
                    }
                    if err_state {
                        // one of the notify fds is in an error state so try to find new device
                        blue.blue
                            .get_device(&mac)
                            .unwrap()
                            .forget_service(&ecp_uuid);
                        break;
                    }
                    if msgs.len() != 0 {
                        if verbose >= 3 {
                            eprintln!("LedMsgs to be send: {:?}", msgs);
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
