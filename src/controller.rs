use crate::color::ColorMap;
use crate::{Command, LedMsg, Receiver};

#[cfg(feature = "rpi")]
pub use rs_ws281x;
use std::collections::VecDeque;

pub trait Controller {
    fn leds_mut(&mut self) -> &mut [[u8; 4]];
    fn leds(&self) -> &[[u8; 4]];
    fn render(&mut self);
}

#[cfg(feature = "rpi")]
impl Controller for rs_ws281x::Controller {
    #[inline]
    fn leds_mut(&mut self) -> &mut [[u8; 4]] {
        self.leds_mut(0)
    }
    #[inline]
    fn leds(&self) -> &[[u8; 4]] {
        self.leds(0)
    }
    #[inline]
    fn render(&mut self) {
        self.render().unwrap()
    }
}
pub struct Renderer<T: Receiver, C: Controller> {
    recv: T,
    ctl: C,
    msgs: VecDeque<(bool, LedMsg)>,
    blend: u8,
    color_map: ColorMap,
}

impl<R: Receiver, C: Controller> Renderer<R, C> {
    pub fn new(recv: R, ctl: C) -> Result<Self, std::io::Error> {
        /*
        let channel = rs_ws281x::ChannelBuilder::new()
            .pin(pin)
            .strip_type(rs_ws281x::StripType::Ws2812)
            .count(count)
            .brightness(255)
            .build();
        let ctl = ControllerBuilder::new()
            .freq(800_000)
            .channel(0, channel)
            .build()
            .unwrap(); // TODO: figure out how to map this
         */
        Ok(Renderer {
            recv,
            ctl,
            msgs: VecDeque::new(),
            blend: 0,
            color_map: ColorMap::default(),
        })
    }
    #[inline]
    pub fn set_blend(&mut self, blend: u8) {
        self.blend = blend;
    }
    #[inline]
    pub fn blend(&self) -> u8 {
        self.blend
    }
    pub fn update_leds(&mut self) {
        // append values to list of msg
        for msgs in self.recv.try_iter() {
            self.msgs.extend(msgs.into_iter().map(|x| (true, x)));
        }
        // purge leading old message
        while let Some(v) = self.msgs.get_mut(0) {
            if !v.0 || v.1.cur_time > self.recv.cur_time() + 5_000_000 {
                self.msgs.pop_front();
            } else {
                break;
            }
        }
        let mut elements = [None; 256];
        let mut last_active = 0;
        let mut first_active = 256;
        let cur_time = self.recv.cur_time();
        for i in 0..self.msgs.len() {
            let mut msg = self.msgs[i];
            if msg.0 {
                if msg.1.cur_time > cur_time + 5_000_000 {
                    msg.0 = false;
                } else {
                    let e = msg.1.element as usize;
                    if let Some(j) = elements[e] {
                        self.msgs[j].0 = false; // disable overwritten element
                    } else {
                        if e > last_active {
                            last_active = e;
                        }
                        if e < first_active {
                            first_active = e;
                        }
                    }
                    elements[e] = Some(i);
                }
            }
        }
        let mut flat_stack = 0;
        let mut buf = [[0_u8; 4]; 256];
        for i in first_active..last_active {
            if let Some(m) = elements[i as usize] {
                let msg = self.msgs[m];
                match msg.1.cmd {
                    Command::FlatStack(v) => {
                        let end = 255.min(flat_stack + v as usize);
                        let color = self.color_map[msg.1.color].to_rgba();
                        for j in flat_stack..end {
                            // add color
                            for (spt, sps) in buf[j].iter_mut().zip(color.iter()) {
                                *spt = spt.saturating_add(*sps);
                            }
                        }
                        flat_stack = end;
                    }
                    _ => unimplemented!(),
                }
            }
        }
        let leds = self.ctl.leds_mut();
        for (led, src) in leds.iter_mut().zip(buf.iter()) {
            if self.blend == 0 {
                *led = *src;
            } else {
                for (d, s) in led.iter_mut().zip(src.iter()) {
                    let blend = self.blend as u16;
                    *d = (((*d as u16 * blend) + *s as u16) / (blend + 1)) as u8;
                }
            }
        }
    }
}
