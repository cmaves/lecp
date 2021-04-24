use crate::color::Color;
use crate::{channel, Command, LedMsg, Receiver, Sender};
use rand::prelude::*;

fn rng() -> StdRng {
    let mut seed = [0; 32];
    for (i, v) in seed.iter_mut().enumerate() {
        *v = i as u8;
    }
    StdRng::from_seed(seed)
}
fn generate_test_msgs() -> [LedMsg; 255] {
    let mut test_vals = [LedMsg {
        cmd: Command::Null,
        time: 0,
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
    for msg in test_vals.iter_mut() {
        let b: u8 = rng.gen();
        msg.time = match b & 0x03 {
            0x00 => 0,
            0x01 => rng.gen::<i8>() as u64,
            0x02 => rng.gen::<i16>() as u64,
            0x03 => rng.gen::<i32>() as u64,
            _ => unreachable!(),
        };
        msg.element = rng.gen();
        msg.color = rng.gen();
    }
    test_vals
}
#[test]
fn serial_deserialize() {
    let test_vals = generate_test_msgs();
    let mut i = 0;
    // serialize
    let mut buf = [0; LedMsg::MAX_LEN + 4];
    while i < test_vals.len() {
        let (bytes, msgs) = LedMsg::serialize(&test_vals[i..], &mut buf, 0);
        eprintln!("bytes: {}, msgs: {}", bytes, msgs);
        eprintln!("{:X?}", &buf[..bytes]);
        eprintln!("{:#?}", &test_vals[i..i + msgs]);
        let cpy = LedMsg::deserialize(&buf[..bytes], 0).unwrap();
        assert_eq!(&test_vals[i..i + msgs], &cpy[..]);
        i += msgs;
    }
}

#[test]
fn local_send_receive() {
    let test_vals = generate_test_msgs();
    let (mut sender, mut recv) = channel(1);
    for i in 0..5 {
        sender.send(&test_vals[i * 51..(i + 1) * 51]).unwrap();
        let cpy = recv.recv().unwrap();
        assert_eq!(&test_vals[i * 51..(i + 1) * 51], &cpy[..]);
    }
}
