//! Host supplies the authenticated process and original invocation handle.
use crate::{
    Result,
    operator::{
        config::Authority,
        domain::{self, Request},
    },
    store::Store,
};
use serde_json::Value;

pub(super) async fn callback(
    store: &Store,
    plugin: &str,
    generation: &str,
    method: &str,
    args: Value,
) -> Result<Value> {
    super::grants::schema(method, "plugin_to_host", &args)?;
    let authority = Authority::Invocation {
        plugin: plugin.into(),
        generation: generation.into(),
        handle: args["handle"].as_str().ok_or("unauthenticated")?.into(),
    };
    let action = args["method"]
        .as_str()
        .ok_or("validation_error")?
        .to_owned();
    match method {
        "control.read" => {
            domain::invoke(
                store,
                Request::Read(authority, action, args["args"].clone()),
            )
            .await
        }
        "control.write" => domain::save(store, authority, action, args["body"].clone()).await,
        _ => Err("unsupported_control_method".into()),
    }
}
