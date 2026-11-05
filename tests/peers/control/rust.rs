//! Independent packaged SDK consumer; the test inbox supplies adversarial business calls.
use serde_json::{Value, json};
use std::{fs, io::{self, Write}, path::{Path, PathBuf}, sync::mpsc, time::Duration};
use yuki_plugin_sdk::{Binding, Id, Peer, Received, host::{Calls, Incoming}, read_frame};
fn send(out: &mut impl Write, frame: &Value) -> Result<(), Box<dyn std::error::Error>> {
    writeln!(out,"{frame}")?; out.flush()?; Ok(())
}
fn save(folder: &Path, name: &str, value: &Value) -> io::Result<()> {
    let temp=folder.join(format!("{name}.tmp"));
    fs::write(&temp,value.to_string())?;fs::rename(temp,folder.join(format!("{name}.json")))
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let folder=PathBuf::from(std::env::args().nth(1).ok_or("missing test inbox")?);
    let binding: Binding=serde_json::from_str(&std::env::var("YUKI_PLUGIN_BINDING")?)?;
    let offer=binding.offer();let mut peer=Peer::new(binding)?;
    let mut output=io::stdout().lock();let input=io::stdin();
    send(&mut output,&peer.request(Id::Number(1),offer)?)?;
    let raw=read_frame(&mut input.lock())?.ok_or("missing handshake")?;
    if !matches!(peer.receive(&raw)?,Received::Response{result:Ok(_),..}) { return Err("handshake denied".into()); }
    let initialized:Value=serde_json::from_slice(&raw)?;
    let mut calls=Calls::negotiated(&initialized["result"])?;
    let (sender,receiver)=mpsc::channel();
    std::thread::spawn(move || {
        let mut input=io::stdin().lock();
        while let Ok(Some(raw))=read_frame(&mut input) { if sender.send(raw).is_err(){break;} }
    });
    fs::write(folder.join("ready"),b"rust packaged SDK")?;
    loop {
        match receiver.recv_timeout(Duration::from_millis(10)) {
            Ok(raw)=> {
                let message:Value=serde_json::from_slice(&raw)?;
                match calls.receive(&raw)? {
                    Incoming::Call{method,id,params} if method=="command.invoke"=>save(&folder,"command",&json!({"id":id,"params":params}))?,
                    Incoming::Result{..}=>save(&folder,&format!("response-{}",message["id"]),&message)?,
                    _=>return Err("unexpected control message".into()),
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected)=>break,
            Err(mpsc::RecvTimeoutError::Timeout)=>(),
        }
        for item in fs::read_dir(&folder)? {
            let path=item?.path();let name=path.file_name().and_then(|s|s.to_str()).unwrap_or("");
            if !name.starts_with("request-")||!name.ends_with(".json"){continue;}
            let Ok(raw)=fs::read(&path) else {continue};
            let Ok(request)=serde_json::from_slice::<Value>(&raw) else {continue};
            let frame=calls.request(serde_json::from_value(request["id"].clone())?,request["method"].as_str().ok_or("method required")?,request["params"].clone())?;
            fs::remove_file(path)?;send(&mut output,&frame)?;
        }
    }
    Ok(())
}
