use crate::color::Color;
use crate::{Command, LedMsg};
use rand::prelude::*;

fn rng() -> StdRng {
    let mut seed = [0; 32];
    for (i, v) in seed.iter_mut().enumerate() {
        *v = i as u8;
    }
    StdRng::from_seed(seed)
}
#[test]
fn serial_deserialize() {
    let mut test_vals = [LedMsg {
        cmd: Command::Null,
        cur_time: 0,
        color: 0,
        element: 0,
    }; 255];
    let mut rng = rng();

    for (i, cmd) in [
        Command::Null,
        Command::Flat(0),
        Command::FlatStack(0),
        Command::PulseLinear(0),
        Command::PulseQuadratic(0),
    ]
    .iter()
    .enumerate()
    {
        for msg in test_vals[i * 51..(i + 1) * 51].iter_mut() {
            let cmd = match cmd {
                Command::Null => Command::Null,
                Command::Flat(_) => Command::Flat(rng.gen()),
                Command::FlatStack(_) => Command::FlatStack(rng.gen()),
                Command::PulseLinear(_) => Command::PulseLinear(rng.gen()),
                Command::PulseQuadratic(_) => Command::PulseQuadratic(rng.gen()),
            };
            msg.cmd = cmd;
        }
    }
    test_vals.shuffle(&mut rng);
    for i in 0..4 {
        for msg in test_vals.iter_mut().skip(i).step_by(4) {
            let cur_time: u32 = rng.gen();
            msg.cur_time = cur_time >> (i * 8);
            msg.element = rng.gen();
            msg.color = rng.gen();
        }
    }
    let mut i = 0;
    // serialize
    while i < test_vals.len() {
        let mut buf = [0; 255];
        let (bytes, msgs) = LedMsg::serialize(&test_vals[i..], &mut buf);
        eprintln!("bytes: {}, msgs: {}", bytes, msgs);
        eprintln!("{:#?}", &test_vals[i..i + msgs]);
        //eprintln!("{:#4x?}", &buf[..bytes]);
        let cpy = LedMsg::deserialize(&buf[..bytes]).unwrap();
        assert_eq!(&test_vals[i..i + msgs], &cpy[..]);
        i += msgs;
    }
}
