use super::{Change, write};
use crate::{
    Result,
    model::{attempt, chat::Task},
    runtime::main,
    store::Store,
};
use serde_json::{Value, json};

pub(super) async fn run(
    store: &Store,
    plugin: &str,
    generation: &str,
    request: &str,
) -> Result<()> {
    let permit = write(
        store,
        Change::Dispatch {
            plugin: plugin.into(),
            generation: generation.into(),
            request: request.into(),
        },
    )
    .await?;
    if permit["dispatch_now"] != true {
        return Ok(());
    }
    #[cfg(test)]
    crate::store::tests::commit_barrier("aux-dispatched-before-http").await;
    let profile: main::Config = serde_json::from_value(permit["profile"].clone())?;
    let task: Option<Task> = serde_json::from_value(permit["task"].clone())?;
    let attempt = permit["attempt"]
        .as_str()
        .ok_or("original auxiliary attempt missing")?
        .to_owned();
    let raw = permit["request_raw"]
        .as_str()
        .ok_or("original auxiliary request missing")?;
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    let result = main::physical_request(&client, &profile, raw).await;
    let Ok((status, bytes, complete)) = result else {
        attempt::invoke(store, attempt::Action::Unknown { attempt }).await?;
        return Ok(());
    };
    let body = String::from_utf8_lossy(&bytes).into_owned();
    let usage = profile.protocol.usage(&body).to_string();
    let action = if !complete {
        attempt::Action::Hold {
            attempt,
            body,
            usage,
            partial_bytes: Some(bytes),
            diagnostic: "auxiliary_incomplete_response".into(),
            unknown: true,
            status: status.into(),
        }
    } else if status != 200 {
        attempt::Action::Rejected {
            attempt,
            status: status.into(),
            unbilled_before_model: false,
            body,
            usage,
        }
    } else if !profile
        .protocol
        .complete(&serde_json::from_str(&body).unwrap_or(Value::Null))
        || profile
            .protocol
            .decode(&body, task.as_ref(), &profile, &serde_json::from_str(raw)?)
            .is_err()
    {
        attempt::Action::Hold {
            attempt,
            body,
            usage,
            partial_bytes: None,
            diagnostic: "auxiliary_invalid_output".into(),
            unknown: false,
            status: 200,
        }
    } else {
        attempt::Action::Response {
            attempt,
            body,
            usage,
            intents: json!([]).to_string(),
        }
    };
    attempt::invoke(store, action).await?;
    #[cfg(test)]
    crate::store::tests::commit_barrier("aux-response-before-reply").await;
    Ok(())
}
