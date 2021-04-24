use async_std::task::block_on;
use futures::future::join;

use crate::{Error, LedMsg, Sender as LECPSender};

use rustable::{Adapter, MAC};

use btutils::messaging::{ClientOptions as MsgOptions, MsgChannelClient, SERV_UUID as MSG_SERV};
use btutils::timing::{TimeClient, TIME_SERV};

use std::time::{Duration, Instant};

use super::ECP_UUID;

use log::{error, warn};

pub struct BluetoothSender {
    _last_sync: Instant,
    time: TimeClient,
    msg: MsgChannelClient,
}

impl BluetoothSender {
    pub async fn new(hci: u8, mac: MAC) -> Result<Self, Error> {
        let hci = Adapter::new(hci).await?;
        let dev = hci.get_device(mac).await?;
        if !dev.connected().await? {
            return Err(Error::NotConnected);
        }
        let ecp_serv = dev.get_service(ECP_UUID).await?;
        let includes = ecp_serv.get_includes().await?;
        let mut time_serv = None;
        let mut msg_serv = None;
        for serv in includes {
            if serv.uuid() == TIME_SERV {
                time_serv = Some(serv);
            } else if serv.uuid() == MSG_SERV {
                msg_serv = Some(serv);
            }
            if time_serv.is_some() && msg_serv.is_some() {
                break;
            }
        }
        let time_serv = match time_serv {
            Some(s) => s,
            None => {
                warn!("LECP service didn't include Time Service. Fetching manually");
                dev.get_service(TIME_SERV).await?
            }
        };
        let msg_serv = match msg_serv {
            Some(s) => s,
            None => {
                warn!("LECP service didn't include Msg Service. Fetching manually");
                dev.get_service(MSG_SERV).await?
            }
        };

        let time = TimeClient::from_service(time_serv).await?;
        let mut msg_options = MsgOptions::new(mac);
        msg_options.target_lt = Duration::from_millis(100);
        let msg = MsgChannelClient::from_service(msg_serv, msg_options).await?;
        let synced = time.do_client_sync().await.expect("Err unimplemented!");
        if !synced {
            unimplemented!("What to do if time sync fails?")
        }
        let last_sync = Instant::now();
        Ok(Self {
            time,
            msg,
            _last_sync: last_sync,
        })
    }
    pub fn do_time_sync(&self) -> Result<(), Error> {
        let synced = block_on(self.time.do_client_sync()).expect("Err unimplemented!");
        if !synced {
            unimplemented!();
        } else {
            Ok(())
        }
    }
    pub fn get_avg_lat(&self) -> Duration {
        self.msg.get_avg_lat()
    }
    pub fn get_sent(&self) -> u32 {
        self.msg.get_sent()
    }
    pub fn get_loss_rate(&self) -> f64 {
        self.msg.get_loss_rate()
    }
    pub fn mtu(&self) -> u16 {
        self.msg.get_out_mtu()
    }
    pub fn shutdown(self) -> Result<(), Error> {
        let t_shut = self.time.shutdown();
        let m_shut = self.msg.shutdown();
        let (res1, res2) = block_on(join(t_shut, m_shut));
        res1.expect("Err unimplemented!");
        res2.expect("Err unimplemented!");
        Ok(())
    }
}

impl LECPSender for BluetoothSender {
    fn send(&mut self, msgs: &mut [LedMsg], is_time_offset: bool) -> Result<(), Error> {
        let mut out_buf = [0; 512];
        let mtu = self.msg.get_out_mtu();
        let out_buf = &mut out_buf[..mtu as usize];
        let mut msgs_sent = 0;
        let cur_time = self.get_time();
        if is_time_offset {
            for msg in msgs.iter_mut() {
                msg.time = cur_time.wrapping_add(msg.time);
            }
        }
        while msgs_sent < msgs.len() {
            let to_send = &msgs[msgs_sent..];
            let (sent, used) = LedMsg::serialize(to_send, out_buf, cur_time);
            msgs_sent += sent;
            block_on(self.msg.send_msg(&out_buf[..used])).expect("Err unimplemented");
        }
        Ok(())
    }
    fn get_time(&self) -> u64 {
        self.time.get_time()
    }
}
