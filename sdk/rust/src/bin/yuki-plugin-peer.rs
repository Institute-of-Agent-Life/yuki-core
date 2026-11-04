use std::{
    env, fs,
    io::{self, Write},
};
use yuki_plugin_sdk::{Binding, Id, Peer, Received, read_frame};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let mut initiate = false;
    let mut binding = Binding {
        namespace: "fixture.plugin".into(),
        grants: vec![],
        manifest_revision: "fixture-r1".into(),
        declarations: vec![],
    };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--initiate" => initiate = true,
            "--binding" => {
                binding = serde_json::from_slice(&fs::read(args.next().ok_or("missing binding")?)?)?
            }
            _ => return Err("unknown peer option".into()),
        }
    }
    let offer = binding.offer();
    let mut peer = Peer::new(binding)?;
    let mut output = io::stdout().lock();
    if initiate {
        writeln!(output, "{}", peer.request(Id::Number(1), offer)?)?;
        output.flush()?;
    }
    let mut input = io::stdin().lock();
    while let Some(frame) = read_frame(&mut input)? {
        match peer.receive(&frame)? {
            Received::Reply(value) => {
                writeln!(output, "{value}")?;
                output.flush()?;
            }
            Received::Response { result, .. } => {
                if result.is_err() {
                    return Err("initialize rejected".into());
                }
            }
            Received::Notification => {}
        }
    }
    peer.finish()?;
    Ok(())
}
