use rs_ws281x::{ChannelBuilder, ControllerBuilder};

pub struct Controller {}

impl Controller {
    fn new(pin: i32, count: i32) -> Self {
        rs_ws281x::ChannelBuilder::new()
            .pin(pin)
            .strip_type(rs_ws281x::StripType::Ws2812)
            .count(count)
            .brightness(255)
            .build();
        let controller = ControllerBuilder::new()
            .freq(800_000)
            .channel(o, channnel)
            .build()
            .unwrap();
        controller(size);

        unimplemented!();
    }
}
