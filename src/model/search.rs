mod source;
mod worker;
use crate::{
    Result,
    runtime::main::Config as Profile,
    store::{Mutation, Store},
    tools::registry::Tool,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::SqliteConnection;
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Disabled,
    Native,
    Tavily,
    Both,
}
#[derive(Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Strategy {
    #[default]
    External,
    Bridge,
    Native,
    Both,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub mode: Mode,
    #[serde(default)]
    pub strategy: Strategy,
    pub connection: Option<Profile>,
    pub connection_id: Option<String>,
    pub credential_namespace: Option<String>,
    #[serde(default = "ttl")]
    pub cache_seconds: u32,
}
fn ttl() -> u32 {
    600
}
impl Config {
    pub fn bridge(&self) -> bool {
        matches!(self.mode, Mode::Native | Mode::Both)
            && matches!(self.strategy, Strategy::Bridge | Strategy::Both)
    }
    pub fn inline(&self) -> bool {
        matches!(self.mode, Mode::Native | Mode::Both)
            && matches!(self.strategy, Strategy::Native | Strategy::Both)
    }
    pub fn validate(&self) -> Result<()> {
        if self.cache_seconds > 86400 {
            return Err("search cache TTL exceeds 24 hours".into());
        }
        if self.bridge() {
            let p = self
                .connection
                .as_ref()
                .ok_or("selected native search requires an independent connection")?;
            p.validate()?;
            if p.protocol.is_chat()
                || p.tool_choice
                    .as_deref()
                    .is_some_and(|choice| !matches!(choice, "auto" | "required"))
                || !p.native_web_search
                || p.binding_endpoint.is_some()
                || self
                    .connection_id
                    .as_deref()
                    .is_none_or(|s| s.trim().is_empty())
                || self
                    .credential_namespace
                    .as_deref()
                    .is_none_or(|s| s.trim().is_empty())
            {
                return Err("native search requires an authorized capable profile, connection and credential namespace".into());
            }
        }
        Ok(())
    }
}
pub struct Engine {
    cfg: Config,
    client: reqwest::Client,
    locks: [tokio::sync::Mutex<()>; 32],
}
pub fn start(cfg: Option<Config>) -> Result<Option<Arc<Engine>>> {
    cfg.map(|cfg| -> Result<_> {
        cfg.validate()?;
        Ok(Arc::new(Engine {
            cfg,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            locks: std::array::from_fn(|_| tokio::sync::Mutex::new(())),
        }))
    })
    .transpose()
}
#[derive(Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct Query {
    pub query: String,
    #[serde(default = "topic")]
    pub topic: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_range: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_date: Option<String>,
    #[serde(default = "count")]
    pub max_results: usize,
}
fn topic() -> String {
    "general".into()
}
fn count() -> usize {
    5
}
impl Query {
    fn normalize(mut self) -> Result<Self> {
        self.query = self.query.split_whitespace().collect::<Vec<_>>().join(" ");
        for date in [&self.start_date, &self.end_date].into_iter().flatten() {
            chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")?;
        }
        if self.query.is_empty()
            || self.query.chars().count() > 2048
            || !matches!(self.topic.as_str(), "general" | "news")
            || !(1..=20).contains(&self.max_results)
            || self
                .time_range
                .as_deref()
                .is_some_and(|s| !matches!(s, "day" | "week" | "month" | "year"))
            || matches!((&self.start_date,&self.end_date),(Some(s),Some(e)) if s>e)
        {
            return Err("invalid search constraints".into());
        }
        Ok(self)
    }
    fn wire(&self, p: &Profile) -> Result<Value> {
        let mut fields = Vec::new();
        fn field(k: &str, v: &Value) -> String {
            format!("{}: {}", json!(k), v)
        }
        fields.push(field("query", &json!(self.query)));
        fields.push(field("topic", &json!(self.topic)));
        for (k, v) in [
            ("time_range", &self.time_range),
            ("start_date", &self.start_date),
            ("end_date", &self.end_date),
        ] {
            if let Some(s) = v {
                fields.push(field(k, &json!(s)))
            }
        }
        fields.push(field("max_results", &json!(self.max_results)));
        let prompt = format!(
            "请使用已声明的原生网页搜索查找以下问题的公开来源。只依据实际搜索到的来源回答；不要编造 URL、来源或发布日期。日期无法核实时请说明。\n{{{}}}",
            fields.join(", ")
        );
        p.protocol.request(json!({"model":p.model,"max_tokens":p.max_output,"messages":[{"role":"user","content":prompt}],"stream":false}),p,None)
    }
}
pub fn declaration() -> Tool {
    Tool {
        provider: None,
        name: "web_search".into(),
        description: "用獨立的已配置搜索連線查詢公開來源；結果保留原生來源及部分失敗狀態。".into(),
        schema: json!({"type":"object","properties":{"query":{"type":"string","minLength":1},"topic":{"enum":["general","news"]},"time_range":{"enum":["day","week","month","year"]},"start_date":{"type":"string"},"end_date":{"type":"string"},"max_results":{"type":"integer","minimum":1,"maximum":20}},"required":["query"],"additionalProperties":false}),
        parallel_read: false,
        cacheable: false,
        code_direct: false,
    }
}
#[derive(clap::Args, Serialize, Deserialize)]
pub struct Action {
    pub execution: String,
    pub actor: String,
    pub operation: String,
    pub request: String,
}
pub enum Change {
    Prepare {
        cfg: Config,
        exec: String,
        actor: String,
        op: String,
        query: Query,
        namespace: String,
    },
    Finish {
        cfg: Config,
        exec: String,
        actor: String,
        logical: String,
        query: Query,
        reply: worker::Reply,
    },
    Tool {
        exec: String,
        actor: String,
        op: String,
        value: Value,
        ok: bool,
    },
}
pub async fn apply(conn: &mut SqliteConnection, c: &Change) -> Result<Value> {
    worker::apply(conn, c).await
}
pub async fn invoke(store: &Store, act: Action) -> Result<Value> {
    worker::run(
        store,
        &act.execution,
        &act.actor,
        &act.operation,
        serde_json::from_str(&act.request)?,
    )
    .await
}
pub async fn tool(store: &Store, exec: &str, actor: &str, op: &str, args: &str) -> Result<()> {
    let result =
        async { worker::run(store, exec, actor, op, serde_json::from_str(args)?).await }.await;
    let (ok, v) = match result {
        Ok(v) => (v["status"] == "complete" || v["status"] == "partial", v),
        Err(_) => (false, json!({"status":"search_unavailable"})),
    };
    store
        .write(Mutation::Search(Box::new(Change::Tool {
            exec: exec.into(),
            actor: actor.into(),
            op: op.into(),
            value: v,
            ok,
        })))
        .await?;
    Ok(())
}
