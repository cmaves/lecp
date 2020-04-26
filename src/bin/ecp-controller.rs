use clap::{App, Arg, ArgMatches};
use ecp::color::Color;
use ecp::controller::Renderer;
use gpio_cdev::Chip;
use ham::rfm69::Rfm69;
use ham::IntoPacketReceiver;
use spidev::Spidev;
use std::num::{NonZeroU16, NonZeroU8};
use std::str::FromStr;
use std::thread::Builder;
use std::time::Instant;

pub fn main() {
    let parser = parser();
    let args = parser.get_matches();
    let pin = u8::from_str(args.value_of("led_pin").unwrap()).unwrap() as i32;
    let count = u16::from_str(args.value_of("led_count").unwrap()).unwrap() as i32;
    let mut chip = Chip::new("/dev/gpiochip0").unwrap();
    let en = chip
        .get_line(u32::from_str(args.value_of("en").unwrap()).unwrap())
        .unwrap();
    let rst = chip
        .get_line(u32::from_str(args.value_of("rst").unwrap()).unwrap())
        .unwrap();
    let spi = Spidev::open(args.value_of("spi").unwrap()).unwrap();
    let mut rfm = Rfm69::new(rst, en, spi).unwrap();
    rfm.set_bitrate(45000).unwrap();
    let recv = rfm.into_packet_receiver().unwrap();
    Builder::new()
        .name("rendering".to_string())
        .spawn(move || {
            let channel = rs_ws281x::ChannelBuilder::new()
                .pin(pin)
                .strip_type(rs_ws281x::StripType::Ws2812)
                .count(count)
                .brightness(255)
                .build();
            let ctl = rs_ws281x::ControllerBuilder::new()
                .freq(800_000)
                .channel(0, channel)
                .build()
                .unwrap();
            let mut renderer = Renderer::new(recv, ctl);
            renderer.blend = 3;
            renderer.verbose = true;
            renderer.color_map[2] = Color::YELLOW;
            renderer.color_map[3] = Color::GREEN;
            renderer.color_map[4] = Color::BLUE;
            panic!(
                "Rendering thread quit: {:?}",
                renderer.update_leds_loop(60.0)
            );
        })
        .unwrap();
}

fn parser<'a, 'b>() -> App<'a, 'b> {
    App::new("ECP receiver")
        .version("0.1")
        .author("Curtis Maves <curtismaves@gmail.com")
        .arg(
            Arg::with_name("value")
                .short("a")
                .long("alg")
                .value_name("ALGORITHM")
                .possible_values(&["linear", "quadratic"])
                .help("Sets the algorithm used to scale the light bars.")
                .default_value("quadratic")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("brightness")
                .short("b")
                .long("brightness")
                .value_name("BRIGHTNESS")
                .takes_value(true)
                .validator(|s| {
                    NonZeroU8::from_str(&s)
                        .map(|_| ())
                        .map_err(|e| format!("{:?}", e))
                }),
        )
        .arg(
            Arg::with_name("led_pin")
                .short("p")
                .long("pin")
                .value_name("PIN")
                .takes_value(true)
                .validator(|s| u8::from_str(&s).map(|_| ()).map_err(|e| format!("{:?}", e)))
                .default_value("18"),
        )
        .arg(
            Arg::with_name("led_count")
                .short("c")
                .long("count")
                .value_name("COUNT")
                .takes_value(true)
                .validator(|s| {
                    NonZeroU16::from_str(&s)
                        .map(|_| ())
                        .map_err(|e| format!("{:?}", e))
                })
                .default_value("288"),
        )
        .arg(
            Arg::with_name("spi")
                .short("s")
                .long("spi")
                .value_name("SPIPATH")
                .takes_value(true)
                .default_value("/dev/spidev0.0"),
        )
        .arg(
            Arg::with_name("rst")
                .short("r")
                .long("reset")
                .value_name("RSTPIN")
                .takes_value(true)
                .validator(|s| {
                    NonZeroU8::from_str(&s)
                        .map(|_| ())
                        .map_err(|e| format!("{:?}", e))
                }),
        )
        .arg(
            Arg::with_name("en")
                .short("e")
                .long("enable")
                .value_name("ENPIN")
                .takes_value(true)
                .validator(|s| {
                    NonZeroU8::from_str(&s)
                        .map(|_| ())
                        .map_err(|e| format!("{:?}", e))
                }),
        )
}
