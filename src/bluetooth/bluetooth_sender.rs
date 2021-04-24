use async_std::task::block_on;
use futures::future::join;

use crate::{Error, LedMsg, Sender as LECPSender};

use rustable::{Adapter, MAC};

use btutils::messaging::{ClientOptions as MsgOptions, MsgChannelClient, SERV_UUID as MSG_SERV};
use btutils::timing::{TimeClient, TIME_SERV};

use std::time::{Duration, Instant};

use super::ECP_UUID;

pub struct BluetoothSender {
    _last_sync: Instant,
    time: TimeClient,
    msg: MsgChannelClient,
}

impl BluetoothSender {
    pub async fn new(hci: u8, mac: MAC) -> Result<Self, Error> {
        let hci = Adapter::new(hci).await?;
        let dev = hci.get_device(mac).await?;
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
            None => dev.get_service(TIME_SERV).await?,
        };
        let msg_serv = match msg_serv {
            Some(s) => s,
            None => dev.get_service(MSG_SERV).await?,
        };

        let time = TimeClient::from_service(time_serv)
            .await
            .map_err(|_| rustable::Error::UnknownServ(TIME_SERV))?;
        let mut msg_options = MsgOptions::new(mac);
        msg_options.target_lt = Duration::from_millis(100);
        let msg = MsgChannelClient::from_service(msg_serv, msg_options)
            .await
            .map_err(|_| rustable::Error::UnknownServ(MSG_SERV))?;
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
    fn send(&mut self, msgs: &[LedMsg]) -> Result<(), Error> {
        let mut out_buf = [0; 512];
        let mtu = self.msg.get_out_mtu();
        let out_buf = &mut out_buf[..mtu as usize];
        let mut msgs_sent = 0;
        while msgs_sent < msgs.len() {
            let to_send = &msgs[msgs_sent..];
            let (sent, used) = LedMsg::serialize(to_send, out_buf, self.get_time());
            msgs_sent += sent;
            eprintln!("sending ({}, {}): {:x?}", sent, used, &out_buf[..used]);
            block_on(self.msg.send_msg(&out_buf[..used])).expect("Err unimplemented");
        }
        Ok(())
    }
    fn get_time(&self) -> u64 {
        self.time.get_time()
    }
}
