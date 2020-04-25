use crate::color::Color;
use crate::color::ColorMap;
use crate::Error;
use crate::{Command, LedMsg, Receiver};
use std::thread::sleep;
use std::time::{Duration, Instant};

#[cfg(feature = "rpi")]
pub use rs_ws281x;

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
    work_buf: Vec<[u8; 4]>,
    msgs: Vec<LedMsg>,
    pub blend: u8,
    pub color_map: ColorMap,
    pub verbose: bool,
}

impl<R: Receiver, C: Controller> Renderer<R, C> {
    pub fn new(recv: R, ctl: C) -> Self {
        let work_buf = Vec::with_capacity(ctl.leds().len());
        Renderer {
            work_buf,
            recv,
            ctl,
            msgs: Vec::new(),
            blend: 0,
            color_map: ColorMap::default(),
            verbose: false,
        }
    }
    #[inline]
    pub fn set_blend(&mut self, blend: u8) {
        self.blend = blend;
    }
    #[inline]
    pub fn blend(&self) -> u8 {
        self.blend
    }
    pub fn update_leds(&mut self) -> Result<(), Error> {
        // append values to list of msg
        for msgs in self.recv.try_iter() {
            self.msgs.extend(msgs)
        }
        if self.msgs.len() == 0 {
            match self.recv.try_recv() {
                Ok(msgs) => self.msgs.extend(msgs),
                Err(e) => {
                    if let Error::Timeout(_) = e {
                        return Ok(());
                    } else {
                        return Err(e);
                    }
                }
            }
        }
        let mut elements = [None; 256];
        let mut last_active = 0;
        let mut first_active = 256;
        let cur_time = self.recv.cur_time();
        for (i, msg) in self.msgs.iter().enumerate().rev() {
            if cur_time.wrapping_sub(msg.cur_time) <= 5_000_000 {
                let e = msg.element as usize;
                if let None = elements[e] {
                    if e + 1 > last_active {
                        last_active = e + 1;
                    }
                    if e < first_active {
                        first_active = e;
                    }
                    elements[e] = Some(i);
                }
            }
        }
        let mut flat_stack = 0;
        let leds = self.ctl.leds_mut();
        let ratio = leds.len() as f32 / 256.0;
        self.work_buf.clear();
        self.work_buf.resize(leds.len(), [0; 4]);
        for i in first_active..last_active {
            if let Some(m) = elements[i as usize] {
                let msg = self.msgs[m];
                match msg.cmd {
                    Command::FlatStack(v) => {
                        let end = leds
                            .len()
                            .min(flat_stack + ((v as f32 + 1.0) * ratio).round() as usize);
                        let color = self.color_map[msg.color].to_bgra();
                        for j in flat_stack..end {
                            // add color
                            for (spt, sps) in self.work_buf[j].iter_mut().zip(color.iter()) {
                                *spt = spt.saturating_add(*sps);
                            }
                        }
                        flat_stack = end;
                    }
                    _ => unimplemented!(),
                }
            }
        }
        for (led, src) in leds.iter_mut().zip(self.work_buf.iter()) {
            if self.blend == 0 {
                *led = *src;
            } else {
                for (d, s) in led.iter_mut().zip(src.iter()) {
                    if *d != *s {
                        let blend = self.blend as u16;
                        let newval = (((*d as u16 * blend) + *s as u16) / (blend + 1)) as u8;
                        if newval == *d {
                            *d += 1;
                        } else {
                            *d = newval;
                        }
                    }
                }
            }
        }
        /*
        for (i, led) in leds.iter_mut().enumerate() {

            *led = match i % 4 {
                0 => Color::RED.to_bgra(),
                1 => Color::YELLOW.to_bgra(),
                2 => Color::GREEN.to_bgra(),
                3 => Color::BLUE.to_bgra(),
                _ => unreachable!()
            };
        }
        */
        self.ctl.render();

        // Prune old msgs
        let mut del = 0;
        for i in 0..self.msgs.len() {
            let msg = self.msgs[i];
            if elements[msg.element as usize] != Some(i)
                || cur_time.wrapping_sub(msg.cur_time) > 5_000_000
            {
                del += 1;
            } else if del > 0 {
                self.msgs.swap(i - del, i);
            }
        }
        if del > 0 {
            self.msgs.truncate(self.msgs.len() - del)
        }

        Ok(())
    }
    pub fn update_leds_loop(&mut self, target_fps: f64) -> Error {
        let fps_wait = Duration::from_secs_f64(1.0 / target_fps);
        let start = Instant::now();
        let mut fps_period = start;
        let mut period_start = 0;
        let mut count = 0;
        loop {
            let now = Instant::now();
            sleep((start + (fps_wait * count)).saturating_duration_since(now));
            if let Err(e) = self.update_leds() {
                return e;
            }
            if self.verbose {
                let since = now.duration_since(fps_period);
                if since > Duration::from_secs(5) {
                    eprintln!(
                        "FPS: {}, Lifetime FPS : {}",
                        (count - period_start) as f32 / since.as_secs_f32(),
                        count as f32 / now.duration_since(start).as_secs_f32()
                    );
                    period_start = count;
                    fps_period = now;
                }
            }
            count += 1;
        }
    }
}
