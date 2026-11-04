mod source;
mod writer;
use crate::{
    Result,
    model::{
        attempt,
        chat::{Mode, Task},
    },
    runtime::main::{self, Config as Profile},
    store::{Mutation, Store},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::SqliteConnection;
use std::sync::{Arc, LazyLock};

static GOLD: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../fixtures/extract/v4-batch.json"))
        .expect("fixed v4 extraction contract")
});
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub profile: Profile,
    pub mode: Mode,
    #[serde(default = "trigger_count")]
    pub trigger_count: u32,
    #[serde(default = "trigger_count")]
    pub max_events: u32,
    #[serde(default = "chars")]
    pub max_characters: u32,
    #[serde(default = "age")]
    pub max_wait_seconds: u32,
}
fn trigger_count() -> u32 {
    12
}
fn chars() -> u32 {
    8000
}
fn age() -> u32 {
    3600
}
impl Config {
    pub fn validate(&self) -> Result<()> {
        self.profile.validate()?;
        if self.profile.native_web_search
            || self.profile.binding_endpoint.is_some()
            || self.profile.tool_choice.is_some()
            || self.trigger_count == 0
            || self.max_events == 0
            || self.max_events > 512
            || self.max_characters == 0
        {
            return Err(
                "extraction requires a dedicated structured profile and valid batch limits".into(),
            );
        }
        Ok(())
    }
    fn task(&self) -> Task {
        Task {
            mode: self.mode.clone(),
            schema: GOLD["schema"].clone(),
            validation_retries: 0,
        }
    }
}
pub enum Change {
    Enable(bool),
    Tick(Arc<Config>, i64, bool),
}
pub async fn apply(conn: &mut SqliteConnection, act: &Change) -> Result<Value> {
    match act {
        Change::Enable(enable) => {
            sqlx::query("UPDATE extraction_policy SET enabled=? WHERE id=1")
                .bind(enable)
                .execute(conn)
                .await?;
            Ok(Value::Null)
        }
        Change::Tick(cfg, now, foreground) => writer::tick(conn, cfg, *now, *foreground).await,
    }
}
pub(crate) async fn can_dispatch(conn: &mut SqliteConnection, exec: &str) -> Result<bool> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM extraction_batches j JOIN extraction_policy policy ON policy.id=1 AND policy.enabled WHERE j.execution=? AND j.state='prepared'
       AND EXISTS(SELECT 1 FROM extraction_sources f WHERE f.batch=j.id AND f.role='primary')
       AND NOT EXISTS(SELECT 1 FROM extraction_sources f JOIN events e ON e.id=f.event LEFT JOIN entries d ON d.id=e.entry_id LEFT JOIN persons p ON p.id=e.raw_actor LEFT JOIN persons r ON r.id=coalesce(p.current_root,p.id)
         LEFT JOIN assignments a ON a.id=e.actor_stage LEFT JOIN assignments b ON b.id=e.bot_stage WHERE f.batch=j.id AND (d.closed_at IS NOT NULL OR NOT p.enabled OR NOT r.enabled OR a.status!='active' OR b.status!='active' OR EXISTS(SELECT 1 FROM invalidated_sources i WHERE i.source=f.event))))").bind(exec).fetch_one(conn).await?)
}

pub async fn start(
    store: Store,
    cfg: Option<Config>,
) -> Result<Option<tokio::task::JoinHandle<()>>> {
    store
        .write(Mutation::Extraction(Box::new(Change::Enable(
            cfg.is_some(),
        ))))
        .await?;
    let Some(cfg) = cfg else { return Ok(None) };
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(45))
        .build()?;
    let cfg = Arc::new(cfg);
    Ok(Some(tokio::spawn(async move {
        let mut timer = tokio::time::interval(std::time::Duration::from_millis(250));
        timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut last_error = None;
        loop {
            timer.tick().await;
            let result = dispatch(&store, &cfg, &client)
                .await
                .err()
                .map(|err| err.to_string());
            if result.is_some() && result != last_error {
                eprintln!(
                    "Memory extraction paused: {}",
                    result.as_deref().unwrap_or_default()
                );
            }
            last_error = result;
        }
    })))
}
async fn dispatch(store: &Store, cfg: &Arc<Config>, client: &reqwest::Client) -> Result<()> {
    let tick = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis(),
    )?;
    {
        let mut read = store.readers().acquire().await?;
        if !writer::needed(&mut read, cfg, tick, store.main.is_some()).await? {
            return Ok(());
        }
    }
    let job = store
        .write(Mutation::Extraction(Box::new(Change::Tick(
            cfg.clone(),
            tick,
            store.main.is_some(),
        ))))
        .await?;
    let Some(logical) = job["logical"].as_str() else {
        return Ok(());
    };
    let attempt_id = format!("{logical}:http:0");
    let permit = attempt::invoke(
        store,
        attempt::Action::Dispatch {
            logical: logical.into(),
            attempt: attempt_id.clone(),
        },
    )
    .await?;
    if permit["dispatch_now"] != true {
        return Ok(());
    }
    let raw = permit["request_raw"]
        .as_str()
        .ok_or("original extraction request missing")?;
    let result = main::physical_request(client, &cfg.profile, raw).await;
    let Ok((status, bytes, complete)) = result else {
        attempt::invoke(
            store,
            attempt::Action::Unknown {
                attempt: attempt_id,
            },
        )
        .await?;
        return Ok(());
    };
    let text = String::from_utf8_lossy(&bytes).to_string();
    let p = cfg.profile.protocol;
    let usage = p.usage(&text).to_string();
    if !complete {
        attempt::invoke(
            store,
            attempt::Action::Hold {
                attempt: attempt_id,
                body: text,
                usage,
                diagnostic: "extraction_incomplete".into(),
                partial_bytes: Some(bytes),
                unknown: true,
                status: status.into(),
            },
        )
        .await?;
    } else if status != 200 {
        attempt::invoke(
            store,
            attempt::Action::Rejected {
                attempt: attempt_id,
                status: status.into(),
                unbilled_before_model: false,
                body: text,
                usage,
            },
        )
        .await?;
    } else {
        let parsed = serde_json::from_str(&text).unwrap_or(Value::Null);
        if !p.complete(&parsed)
            || p.decode(
                &text,
                Some(&cfg.task()),
                &cfg.profile,
                &serde_json::from_str(raw)?,
            )
            .is_err()
        {
            attempt::invoke(
                store,
                attempt::Action::Hold {
                    attempt: attempt_id,
                    body: text,
                    usage,
                    diagnostic: "extraction_invalid_output".into(),
                    partial_bytes: None,
                    unknown: false,
                    status: 200,
                },
            )
            .await?;
        } else {
            attempt::invoke(
                store,
                attempt::Action::Response {
                    attempt: attempt_id,
                    body: text,
                    usage,
                    intents: json!([]).to_string(),
                },
            )
            .await?;
            #[cfg(test)]
            crate::store::tests::commit_barrier("extract-response-before-facts").await;
        }
    }
    Ok(())
}
