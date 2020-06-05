use super::{ecp_bufs, BMsg, ECP_BUF1_BASE, ECP_UUID};
use crate::{Error, LedMsg, Receiver};
use rustable::gatt::{CharFlags, Characteristic, NotifyPoller, Service};
use rustable::interfaces::{BLUEZ_DEST, MANGAGED_OBJ_CALL, OBJ_MANAGER_IF_STR};
use rustable::{AdType, Advertisement, Bluetooth as BT};
use rustable::{Device, MAC, UUID};
use std::rc::Rc;
use std::sync::mpsc;
use std::thread::{sleep, spawn, JoinHandle};
use std::time::{Duration, Instant};

struct Bluetooth<'a, 'b> {
    blue: BT<'a, 'b>,
    verbose: u8,
}

impl Bluetooth<'_, '_> {
    fn new(blue_path: String, verbose: u8) -> Result<Self, Error> {
        let mut blue = BT::new("ecp_recv", blue_path)?;
        blue.verbose = verbose.saturating_sub(1);
        let ret = Bluetooth { blue, verbose };

        Ok(ret)
    }
    fn find_any_device(&mut self, timeout: Duration) -> Result<MAC, Error> {
        let mut adv = Advertisement::new(AdType::Peripheral, "ecp-device".to_string());
        let ecp_uuid: [UUID; 1] = [ECP_UUID.into()];
        adv.solicit_uuids = Vec::from(&ecp_uuid[..]);
        self.blue.start_advertise(adv)?;
        let tar = Instant::now() + timeout;
        let sleep_dur = Duration::from_secs(1);
        loop {
            self.blue.discover_devices()?;
            let devices = self.blue.devices();
            for device_mac in devices {
                let device = self.blue.get_device(&device_mac).unwrap();
                let ecp_uuid: Rc<str> = ECP_UUID.into();
                if device.has_service(&ecp_uuid) {
                    if device.connected() {
                        return Ok(device_mac);
                    }
                }
            }
            if tar.checked_duration_since(Instant::now()).is_none() {
                return Err(Error::Timeout("Finding device timed out".to_string()));
            } else {
                sleep(sleep_dur);
            }
        }
    }
}

enum RecvMsg {
    LedMsgs(Vec<LedMsg>),
    Time(u32, Instant),
}
pub struct BluetoothReceiver {
    send_bmsg: mpsc::SyncSender<BMsg>,
    recv_msgs: mpsc::Receiver<RecvMsg>,
    handle: JoinHandle<Result<(), Error>>,
    time: u32,
    time_inst: Instant,
}

impl BluetoothReceiver {
    pub fn new(blue_path: String, verbose: u8) -> Result<Self, Error> {
        let (send_bmsg, recv_bmsg) = mpsc::sync_channel(1);
        let (send_msgs, recv_msgs) = mpsc::sync_channel(1);
        let handle = spawn(move || {
            let mut blue = Bluetooth::new(blue_path, verbose)?;
            println!("Waiting for device to connect...");
            let to = Duration::from_secs(60);
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
                let mut device = if let Some(device) = blue.blue.get_device(&mac) {
                    device
                } else {
                    continue;
                };
                let ecp_uuid: UUID = ECP_UUID.into();
                let ecp_bufs = ecp_bufs();
                let fds = if let Some(mut ecp_service) = device.get_service(&ecp_uuid) {
                    let mut fds = Vec::with_capacity(10);
                    for uuid in &ecp_bufs {
                        if let Some(mut r_char) = ecp_service.get_char(uuid) {
                            match r_char.acquire_notify() {
                                Ok(fd) => {
                                    fds.push(fd);
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
                    if fds.len() != 10 {
                        continue;
                    }
                    fds
                } else {
                    continue;
                };

                // loop over incoming notification and continue to
                let mut poller = NotifyPoller::new(&fds);
                loop {
                    blue.blue.process_requests()?;
                    let mut device = match blue.blue.get_device(&mac) {
                        Some(dev) => dev,
                        None => break,
                    };
                    let mut ecp_service = match device.get_service(&ecp_uuid) {
                        Some(serv) => serv,
                        None => break,
                    };
                    let zero = Duration::from_secs(0);
                    let ready = match poller.poll(Some(zero)) {
                        Ok(ready) => ready,
                        Err(_) => {
                            eprintln!("Polling error getting next device.");
                            break;
                        }
                    };
                    let zero = Duration::from_secs(0);
                    let mut msgs = Vec::new();
                    for fd_idx in ready {
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
                                let mut bytes = [0; 4];
                                bytes.copy_from_slice(&v[..4]);
                                let time = u32::from_be_bytes(bytes);
                                if let Err(_) = send_msgs.send(RecvMsg::Time(time, now)) {
                                    return Err(Error::Unrecoverable(
                                        "Receiver is disconnected".to_string(),
                                    ));
                                }
                            }
                        } else {
                            // normal signal
                            let offset = (31 * fd_idx) as u8;
                            if let Ok(received) = LedMsg::deserialize(&v[..l]) {
                                msgs.extend(received.into_iter().map(|mut msg| {
                                    msg.element = msg.element + offset;
                                    msg
                                }));
                            } else if verbose >= 2 {
                                eprintln!("LedMsgs failed to deserialize; skipping...");
                            }
                        }
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
        });
        let ret = BluetoothReceiver {
            send_bmsg,
            recv_msgs,
            handle,
            time: 0,
            time_inst: Instant::now(),
        };
        Ok(ret)
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
                        mpsc::RecvTimeoutError::Disconnected => {
                            Error::Unrecoverable("Receiver thread is disconnected!".to_string())
                        }
                    })
                }
            }
        }
    }
    fn recv(&mut self) -> Result<Vec<LedMsg>, Error> {
        loop {
            match self.recv_msgs.recv() {
                Ok(recv_msg) => match recv_msg {
                    RecvMsg::LedMsgs(msgs) => return Ok(msgs),
                    RecvMsg::Time(time, inst) => {
                        self.time = time;
                        self.time_inst = inst;
                    }
                },
                Err(_) => {
                    return Err(Error::Unrecoverable(
                        "Receiver thread is disconnected!".to_string(),
                    ))
                }
            }
        }
    }
}
