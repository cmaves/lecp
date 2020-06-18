use clap::{App, Arg, ArgMatches};
use ecp::bluetooth::BluetoothReceiver;
use ecp::color::{Color, ColorMap};
use ecp::controller::{Controller, Renderer};
use ecp::Receiver;
use gpio_cdev::Chip;
use ham::rfm69::Rfm69;
use ham::{IntoPacketReceiver, PacketReceiver};
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

    let mode = args.value_of("mode").unwrap();
    let verbose = args.occurrences_of("verbose") as u8;
    let mut color_map = ColorMap::default();
    color_map[2] = Color::YELLOW;
    color_map[3] = Color::GREEN;
    color_map[4] = Color::BLUE;
    let brightness = u8::from_str(args.value_of("brightness").unwrap()).unwrap() as f32 / 255.0;
    for color in color_map[0..5].iter_mut() {
        *color *= brightness;
    }
    match mode {
        "bluetooth" => {
            #[cfg(feature = "bluetooth")]
            {
                let recv = BluetoothReceiver::new("/org/bluez/hci0".to_string(), verbose).unwrap();
                let mut renderer = Renderer::new(recv, ctl);
                renderer.color_map = color_map;
                render(renderer, verbose);
            }
            #[cfg(not(feature = "bluetooth"))]
            {
                panic!("bluetooth was not enabled at compile time.")
            }
        }
        "ham" => {
            // let pin = u8::from_str(args.value_of("led_pin").unwrap()).unwrap() as i32;
            // let count = u16::from_str(args.value_of("led_count").unwrap()).unwrap() as i32;
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
            let mut recv = rfm.into_packet_receiver().unwrap();
            recv.start().unwrap();
            let renderer = Renderer::new(recv, ctl);
            render(renderer, verbose);
        }
        _ => unreachable!(),
    };
}
fn render<R: Receiver, C: Controller>(mut renderer: Renderer<R, C>, verbose: u8) {
    renderer.blend = 3;
    renderer.verbose = verbose;
    renderer.color_map[2] = Color::YELLOW;
    renderer.color_map[3] = Color::GREEN;
    renderer.color_map[4] = Color::BLUE;
    panic!("Rendering quit: {:?}", renderer.update_leds_loop(60.0));
}

fn parser<'a, 'b>() -> App<'a, 'b> {
    App::new("ECP receiver")
        .version("0.1")
        .author("Curtis Maves <curtismaves@gmail.com")
        .arg(
            Arg::with_name("mode")
                .short("m")
                .long("mode")
                .value_name("MODE")
                .possible_values(&["bluetooth", "ham"])
                .help("Control what source to use for light messages")
                .default_value("bluetooth")
                .takes_value(true),
        )
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
                .default_value("255")
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
        .arg(
            Arg::with_name("verbose")
                .short("v")
                .long("verbose")
                .multiple(true),
        )
}
