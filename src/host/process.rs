use super::{
    Change, calls,
    grants::{self, Install},
    write,
};
use crate::{Result, store::Store};
use serde_json::{Value, json};
use std::{collections::HashMap, path::Path, process::Stdio, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::Command,
    sync::{Mutex, mpsc, oneshot, watch},
};
use yuki_plugin_sdk::{Id, MAX_FRAME_BYTES, Peer, Received};

static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
struct Message {
    frame: Value,
    activity: Option<super::activity::Gate>,
    handle: Option<String>,
    flushed: Option<oneshot::Sender<()>>,
    fetch: Option<(Id, super::gateway_media::Gate)>,
}
struct Pending {
    handle: Option<String>,
    mentions_sha256: Option<String>,
    reply: oneshot::Sender<Result<Value>>,
}
#[derive(Clone)]
pub struct Session {
    pub generation: String,
    plugin: String,
    send: mpsc::Sender<Message>,
    stop: watch::Sender<bool>,
    closed: watch::Receiver<bool>,
    pending: Arc<Mutex<HashMap<Id, Pending>>>,
}
impl Session {
    pub fn stopped(&self) -> bool {
        *self.stop.borrow()
    }
    pub fn stop(&self) {
        self.stop.send_replace(true);
    }
    pub async fn wait_closed(&self) {
        let mut closed = self.closed.clone();
        if !*closed.borrow() {
            let _ = tokio::time::timeout(Duration::from_secs(5), closed.changed()).await;
        }
    }
    pub fn notify(&self, frame: Value) -> bool {
        self.send
            .try_send(Message {
                frame,
                activity: None,
                handle: None,
                fetch: None,
                flushed: None,
            })
            .is_ok()
    }
    pub(super) fn activity(&self, gate: super::activity::Gate) {
        let frame = json!({"jsonrpc":"2.0","method":"gateway.activity","params":{
            "lease_id":gate.execution,"route":gate.route,"enabled":gate.enabled,"ttl_ms":5000}});
        let _ = self.send.try_send(Message {
            frame,
            activity: Some(gate),
            handle: None,
            fetch: None,
            flushed: None,
        });
    }
    async fn reply(&self, frame: Value) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.send
            .send(Message {
                frame,
                activity: None,
                handle: None,
                fetch: None,
                flushed: Some(tx),
            })
            .await?;
        tokio::time::timeout(Duration::from_secs(5), rx).await??;
        Ok(())
    }
    pub async fn call(
        &self,
        store: &Store,
        method: &str,
        params: Value,
        handle: Option<String>,
    ) -> Result<Value> {
        self.call_scoped(store, method, params, handle, None).await
    }
    pub(super) async fn fetch_page(
        &self,
        store: &Store,
        params: Value,
        gate: super::gateway_media::Gate,
    ) -> Result<Value> {
        self.call_scoped(store, "gateway.media", params, None, Some(gate))
            .await
    }
    async fn call_scoped(
        &self,
        store: &Store,
        method: &str,
        params: Value,
        handle: Option<String>,
        fetch: Option<super::gateway_media::Gate>,
    ) -> Result<Value> {
        if grants::stable_base() && grants::methods()["methods"].get(method).is_none() {
            if let Some(handle) = handle {
                write(
                    store,
                    Change::Call(calls::Change::Unsent {
                        handle,
                        generation: self.generation.clone(),
                    }),
                )
                .await?;
            }
            return Err("method not supported by this base".into());
        }
        let id = Id::Text(format!(
            "host-{}",
            NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending.lock().await;
            if pending.len() >= 128 {
                drop(pending);
                if let Some(h) = handle {
                    write(
                        store,
                        Change::Call(calls::Change::Unsent {
                            handle: h,
                            generation: self.generation.clone(),
                        }),
                    )
                    .await?;
                }
                return Err("plugin busy".into());
            }
            pending.insert(
                id.clone(),
                Pending {
                    handle: handle.clone(),
                    mentions_sha256: params["arguments"]["mentions_source"]["sha256"]
                        .as_str()
                        .filter(|_| matches!(method, "gateway.plan_send" | "gateway.dispatch_part"))
                        .map(str::to_owned),
                    reply: tx,
                },
            );
        }
        let msg = Message {
            frame: json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}),
            activity: None,
            handle: handle.clone(),
            fetch: fetch.map(|gate| (id.clone(), gate)),
            flushed: None,
        };
        if self.send.try_send(msg).is_err() {
            self.pending.lock().await.remove(&id);
            if let Some(h) = handle {
                write(
                    store,
                    Change::Call(calls::Change::Unsent {
                        handle: h,
                        generation: self.generation.clone(),
                    }),
                )
                .await?;
            }
            return Err("plugin outbound queue unavailable".into());
        }
        let timeout = if let Some(handle) = &handle {
            sqlx::query_scalar::<_,f64>("SELECT json_extract(contract,'$.request_timeout_seconds') FROM code_parents WHERE handle=?").bind(handle).fetch_optional(store.readers()).await?.unwrap_or(12.0)
        } else if method == "plugin.observe" {
            5.0
        } else {
            12.0
        };
        match tokio::time::timeout(Duration::from_secs_f64(timeout), rx).await {
            Ok(Ok(reply)) => reply,
            _ => {
                self.stop();
                Err("plugin did not complete the original request".into())
            }
        }
    }
}

pub async fn launch(
    store: &Store,
    install: &Install,
    data: &Path,
    cleanup: bool,
) -> Result<(Session, oneshot::Receiver<bool>)> {
    let generation = write(
        store,
        Change::Start(
            install.id.clone(),
            cleanup,
            crate::memory::scope::hash(&serde_json::to_value(install)?),
        ),
    )
    .await?
    .as_str()
    .ok_or("missing generation")?
    .to_owned();
    let approved: String = sqlx::query_scalar(
        "SELECT grants FROM host_installations WHERE plugin=? AND generation=? AND enabled",
    )
    .bind(&install.id)
    .bind(&generation)
    .fetch_one(store.readers())
    .await?;
    let approved: std::collections::BTreeSet<String> = serde_json::from_str(&approved)?;
    let (send, mut queue) = mpsc::channel::<Message>(48);
    let (stop, mut stopping) = watch::channel(false);
    let (on_exit, closed) = watch::channel(false);
    let session = Session {
        generation: generation.clone(),
        plugin: install.id.clone(),
        send,
        stop,
        closed,
        pending: Default::default(),
    };
    let (ready, ready_rx) = oneshot::channel();
    let wd = data.join("plugins").join(&install.id);
    std::fs::create_dir_all(&wd)?;
    let mut command = Command::new(&install.program);
    command
        .args(if cleanup {
            install
                .erasure_args
                .as_ref()
                .ok_or("missing cleanup entry")?
        } else {
            &install.args
        })
        .current_dir(&wd)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    for key in &install.env {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    #[cfg(windows)]
    if let Some(system) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", system);
    }
    command.env(
        "YUKI_PLUGIN_START_MODE",
        if cleanup { "erasure_only" } else { "normal" },
    );
    let mut binding = install.binding()?;
    binding.grants = approved.iter().cloned().collect();
    command.env("YUKI_PLUGIN_BINDING", serde_json::to_string(&binding)?);
    let mut child = command.spawn()?;
    let stdout = child.stdout.take().ok_or("plugin stdout missing")?;
    let mut stdin = child.stdin.take().ok_or("plugin stdin missing")?;
    let store_writer = store.clone();
    let id = install.id.clone();
    let instance = generation.clone();
    let halt = session.stop.clone();
    let mut w_stop = stopping.clone();
    let waiters = session.pending.clone();
    let writer = tokio::spawn(async move {
        loop {
            let msg = tokio::select! {biased;_=w_stop.changed()=>break,m=queue.recv()=>match m{Some(v)=>v,None=>break}};
            let Ok(mut bytes) = serde_json::to_vec(&msg.frame) else {
                break;
            };
            if bytes.len() > MAX_FRAME_BYTES {
                break;
            }
            // Check at dequeue, not at the earlier time the request entered the pipe queue.
            if let Some(gate) = &msg.activity {
                if gate.enabled
                    && !store_writer
                        .active_turns
                        .subscribe()
                        .borrow()
                        .contains_key(&gate.execution)
                {
                    continue;
                }
                if write(
                    &store_writer,
                    Change::ActivityDispatch(gate.clone(), id.clone(), instance.clone()),
                )
                .await
                .is_err()
                {
                    continue;
                }
            }
            if let Some((request, gate)) = &msg.fetch
                && write(
                    &store_writer,
                    Change::FetchDispatch(Box::new(gate.clone()), id.clone(), instance.clone()),
                )
                .await
                .is_err()
            {
                if let Some(p) = waiters.lock().await.remove(request) {
                    let _ = p
                        .reply
                        .send(Err("original gateway media dispatch denied".into()));
                }
                continue;
            }
            if let Some(h) = &msg.handle
                && write(
                    &store_writer,
                    Change::Call(calls::Change::Dispatch {
                        plugin: id.clone(),
                        generation: instance.clone(),
                        handle: h.clone(),
                    }),
                )
                .await
                .is_err()
            {
                let _ = write(
                    &store_writer,
                    Change::Call(calls::Change::Unsent {
                        handle: h.clone(),
                        generation: instance.clone(),
                    }),
                )
                .await;
                break;
            }
            bytes.push(b'\n');
            if stdin.write_all(&bytes).await.is_err() || stdin.flush().await.is_err() {
                break;
            }
            if let Some(tx) = msg.flushed {
                let _ = tx.send(());
            }
        }
        halt.send_replace(true);
    });
    let link = session.clone();
    let store = store.clone();
    tokio::spawn(async move {
        let mut peer = Peer::new(binding).expect("validated binding");
        let mut input = BufReader::new(stdout);
        let mut ready = Some(ready);
        let mut rejected_offer = false;
        let mut callbacks = tokio::task::JoinSet::new();
        loop {
            let mut bytes = Vec::new();
            let read = tokio::select! {biased;_=stopping.changed()=>break,
            r=async{(&mut input).take((MAX_FRAME_BYTES+1) as u64).read_until(b'\n',&mut bytes).await}=>r};
            if read.is_err() || bytes.last() != Some(&b'\n') || bytes.len() > MAX_FRAME_BYTES + 1 {
                break;
            }
            bytes.pop();
            let Ok(msg) = serde_json::from_slice::<Value>(&bytes) else {
                break;
            };
            if msg["jsonrpc"] != "2.0" {
                break;
            }
            if let Some(method) = msg["method"].as_str() {
                if method == "initialize" {
                    let Ok(Received::Reply(mut reply)) = peer.receive(&bytes) else {
                        break;
                    };
                    let accepted = reply.get("result").is_some();
                    if accepted {
                        let revisions = reply["result"]["method_revisions"].as_array_mut().unwrap();
                        for (name, d) in grants::methods()["methods"].as_object().unwrap() {
                            if approved.contains(d["grant"].as_str().unwrap())
                                && (!cleanup || d["grant"] == "forget.owner")
                            {
                                revisions.push(json!({"name":name,"revision":d["revision"]}));
                            }
                        }
                        reply["result"]["host_schema_revision"] = json!(2);
                    }
                    if accepted
                        && write(
                            &store,
                            Change::Online(link.plugin.clone(), link.generation.clone()),
                        )
                        .await
                        .is_err()
                    {
                        break;
                    }
                    if link.reply(reply).await.is_err() {
                        break;
                    }
                    if !accepted {
                        rejected_offer = true;
                        break;
                    }
                    if let Some(tx) = ready.take() {
                        let _ = tx.send(true);
                    }
                } else {
                    let valid_id = serde_json::from_value::<Id>(msg["id"].clone()).is_ok();
                    if !valid_id || msg.get("result").is_some() || msg.get("error").is_some() {
                        break;
                    }
                    if ready.is_some() || callbacks.len() >= 32 {
                        if !link.notify(json!({"jsonrpc":"2.0","id":msg["id"],"error":{"code":-32001,"message":"instance unavailable"}})){break;}
                        continue;
                    }
                    if grants::stable_base() && grants::methods()["methods"].get(method).is_none() {
                        if !link.notify(json!({"jsonrpc":"2.0","id":msg["id"],"error":{"code":-32601,"message":"method not supported by this base"}})) {
                            break;
                        }
                        continue;
                    }
                    let s = store.clone();
                    let l = link.clone();
                    let method = method.to_owned();
                    callbacks.spawn(async move{
                        let r=if matches!(method.as_str(),"llm.complete"|"llm.result") {
                            super::aux_model::callback(&s,&l.plugin,&l.generation,&method,msg["params"].clone()).await
                        } else if method.starts_with("notification.") {
                            super::notification::callback(&s,&l.plugin,&l.generation,&method,msg["params"].clone()).await
                        } else if method.starts_with("control.") {
                            super::control::callback(&s,&l.plugin,&l.generation,&method,msg["params"].clone()).await
                        } else if method.starts_with("session.") {
                            super::plugin_sessions::callback(&s,&l.plugin,&l.generation,&method,msg["params"].clone()).await
                        } else if method.starts_with("maintenance.") {
                            super::maintenance::callback(&s,&l.plugin,&l.generation,&method,msg["params"].clone()).await
                        } else if method.starts_with("participation.") {
                            super::participation::callback(&s,&l.plugin,&l.generation,&method,msg["params"].clone()).await
                        } else if super::composition::handles(&method) {
                            super::composition::callback(&s,&l.plugin,&l.generation,&method,msg["params"].clone()).await
                        } else {
                            let digest = if method == "gateway.read_mentions" {
                                let handle = msg["params"]["handle"].as_str();
                                l.pending.lock().await.values()
                                    .find(|pending| handle.is_some() && pending.handle.as_deref() == handle)
                                    .and_then(|pending| pending.mentions_sha256.clone())
                            } else {
                                None
                            };
                            write(&s,Change::Callback(l.plugin.clone(),l.generation.clone(),method,msg["params"].clone(),digest)).await
                        };
                        let out=match r{
                            Ok(v)=>json!({"result":v}),
                            Err(e) if e.is::<super::maintenance::ReflectionDeferred>()=>
                                json!({"error":{"code":-32003,"message":"reflection_deferred_for_foreground"}}),
                            Err(_)=>json!({"error":{"code":-32003,"message":"method, grant or original scope denied"}})
                        };
                        let mut frame=out;frame["id"]=msg["id"].clone();frame["jsonrpc"]=json!("2.0");
                        if !l.notify(frame){l.stop();}
                    });
                }
            } else {
                let Ok(id) = serde_json::from_value::<Id>(msg["id"].clone()) else {
                    break;
                };
                let error = msg.get("error").is_some();
                if error == msg.get("result").is_some() {
                    break;
                }
                let Some(pending) = link.pending.lock().await.remove(&id) else {
                    break;
                };
                let result = if let Some(h) = pending.handle {
                    write(
                        &store,
                        Change::Call(calls::Change::Settle {
                            handle: h,
                            plugin: link.plugin.clone(),
                            generation: link.generation.clone(),
                            body: msg[if error { "error" } else { "result" }].clone(),
                            error,
                        }),
                    )
                    .await
                } else if error {
                    Err("plugin rejected request".into())
                } else {
                    Ok(msg["result"].clone())
                };
                let _ = pending.reply.send(result);
            }
            while callbacks.try_join_next().is_some() {}
        }
        link.stop();
        writer.abort();
        let _ = writer.await;
        callbacks.abort_all();
        // Closing stdin lets a rejected peer consume the flushed reason and exit.
        // A peer that ignores EOF cannot hold up the owner indefinitely.
        if !rejected_offer
            || tokio::time::timeout(Duration::from_secs(1), child.wait())
                .await
                .is_err()
        {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        let _ = write(
            &store,
            Change::Close(link.plugin.clone(), link.generation.clone()),
        )
        .await;
        link.pending.lock().await.clear();
        on_exit.send_replace(true);
    });
    Ok((session, ready_rx))
}
