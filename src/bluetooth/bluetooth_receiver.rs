use std::time::{Duration, Instant};

use async_std::future::timeout;
use async_std::task::block_on;

use gatt::server::{AppWorker, Application};
use rustable::gatt;
use rustable::Adapter;

use crate::{Error, LedMsg, Receiver};
use btutils::messaging::{MsgChannelServ, ServerOptions};
use btutils::timing::TimeService;

pub struct BluetoothReceiver {
    app: AppWorker,
    time: TimeService,
    msg: MsgChannelServ,
}

impl BluetoothReceiver {
    pub async fn new(hci: u8) -> Result<Self, Error> {
        let hci = Adapter::new(hci).await?;
        let mut app = Application::new(&hci, "/io/maves/bt_recv");
        let mut options = ServerOptions::new();
        options.target_lt = Duration::from_millis(100);
        let msg = MsgChannelServ::new(&mut app, &options);
        let time = TimeService::new(&mut app);
        let app = app.register().await.expect("Err unimplementd");

        Ok(Self { msg, time, app })
    }
    pub async fn shutdown(self) -> Result<(), Error> {
        self.app.unregister().await.expect("Err unimplemented!");
        Ok(())
    }
}

impl Receiver for BluetoothReceiver {
    fn cur_time(&self) -> u64 {
        self.time.get_time()
    }
    fn recv_to(&mut self, to_dur: Duration) -> Result<Vec<LedMsg>, Error> {
        let deadline = Instant::now() + to_dur;
        loop {
            let to_dur = deadline.saturating_duration_since(Instant::now());
            return match block_on(timeout(to_dur, self.msg.recv_msg())) {
                Ok(Ok(data)) => match LedMsg::deserialize(&data, self.cur_time()) {
                    Ok(msgs) => Ok(msgs),
                    Err(_) => continue,
                },
                Ok(Err(_)) => Err(Error::Unrecoverable(
                    "BT message service has panicked!".into(),
                )),
                Err(_) => Err(Error::Timeout("Message reception timed out.".into())),
            };
        }
    }
    fn recv(&mut self) -> Result<Vec<LedMsg>, Error> {
        loop {
            return match block_on(self.msg.recv_msg()) {
                Ok(data) => match LedMsg::deserialize(&data, self.cur_time()) {
                    Ok(msgs) => Ok(msgs),
                    Err(_) => continue,
                },
                Err(_) => Err(Error::Unrecoverable(
                    "BT message service has panicked!".into(),
                )),
            };
        }
    }
}
