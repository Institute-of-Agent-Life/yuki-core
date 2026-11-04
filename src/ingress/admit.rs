use crate::{
    Result,
    identity::{assignment, entry, new_id},
    store::{Mutation, Store},
};
use clap::Subcommand;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

#[derive(Subcommand, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    /// A normalized platform envelope, supplied over protected local IPC.
    Record {
        frame: String,
        #[arg(long)]
        accept: bool,
    },
    Claim {
        event: i64,
    },
    Pending,
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Message,
    Edit,
    Recall,
    Data,
    Echo,
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct Span {
    pub segment: usize,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Segment {
    Text {
        text: String,
    },
    Mention {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        account: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        native: Option<String>,
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        span: Option<Span>,
    },
    Media {
        kind: String,
        reference: String,
    },
    Face {
        id: String,
        label: Option<String>,
    },
    Reply {
        native_id: String,
    },
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Sender {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nickname: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Frame {
    pub source: String,
    pub native_id: Option<String>,
    pub account: String,
    pub bot_stage: String,
    pub space: Option<String>,
    pub topic: Option<String>,
    pub occurred: Option<i64>,
    pub observed: i64,
    pub kind: Kind,
    pub segments: Vec<Segment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender: Option<Sender>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_bot: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

impl Frame {
    fn source_scope(&self, stage: Option<&str>) -> Result<String> {
        if self.source.is_empty()
            || self.space.as_deref() == Some("")
            || self.topic.as_deref() == Some("")
        {
            return Err("empty source, space or topic key".into());
        }
        let (kind, scope) = match &self.space {
            Some(space) => ("group", space.as_str()),
            None => match stage {
                Some(stage) => ("private", stage),
                None => ("account_only", self.account.as_str()),
            },
        };
        Ok(serde_json::to_string(&(
            kind,
            scope,
            &self.topic,
            &self.bot_stage,
        ))?)
    }

    fn ordinary(&self) -> bool {
        self.kind == Kind::Message
            && self.segments.iter().any(|s| match s {
                Segment::Text { text } => !self.text.as_deref().unwrap_or(text).trim().is_empty(),
                Segment::Mention {
                    account, native, ..
                } => account.is_some() || native.is_some(),
                Segment::Media { reference, .. } => !reference.is_empty(),
                Segment::Face { id, .. } => !id.is_empty(),
                Segment::Reply { native_id } => !native_id.is_empty(),
            })
    }
}

#[derive(PartialEq)]
pub struct Facts {
    stage: Option<String>,
    actor: Option<String>,
    current: bool,
    mentions: Value,
}

async fn facts(conn: &mut SqliteConnection, f: &Frame) -> Result<Facts> {
    let bot=sqlx::query("SELECT b.status FROM assignments b JOIN accounts receiver ON b.account_id=receiver.id
        JOIN accounts sender ON sender.platform=receiver.platform AND sender.namespace=receiver.namespace
        WHERE b.id=? AND b.kind='self' AND sender.id=?").bind(&f.bot_stage).bind(&f.account)
        .fetch_one(&mut *conn).await?;
    let actor = assignment::resolve(conn, &f.account, f.observed, f.occurred).await?;
    let person = actor["original"].as_str();
    let enabled: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM persons p JOIN persons r ON r.id=coalesce(p.current_root,p.id)
      WHERE p.id=? AND p.enabled AND r.enabled)",
    )
    .bind(person)
    .fetch_one(&mut *conn)
    .await?;
    let current = enabled
        && bot.try_get::<String, _>("status")? == "active"
        && actor["status"] == "active"
        && actor["status_at_source"] == "active";
    let mut mentions = Vec::new();
    for (index, segment) in f.segments.iter().enumerate() {
        if let Segment::Mention {
            account,
            native,
            label,
            span,
        } = segment
        {
            let resolved = if let Some(id) = account {
                let scoped:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM accounts a
                    JOIN accounts s ON a.platform=s.platform AND a.namespace=s.namespace WHERE a.id=? AND s.id=?)")
                    .bind(id).bind(&f.account).fetch_one(&mut *conn).await?;
                if scoped {
                    assignment::resolve(conn, id, f.observed, f.occurred).await?
                } else {
                    Value::Null
                }
            } else {
                Value::Null
            };
            mentions.push(
                json!({"position":span.as_ref().map_or(index,|s|s.segment),"text_start":span.as_ref().map(|s|s.start),"text_end":span.as_ref().map(|s|s.end),"native":native,"label":label,"account":account,
                "assignment":resolved["assignment"],"original":resolved["original"]}),
            );
        }
    }
    Ok(Facts {
        stage: actor["assignment"].as_str().map(str::to_string),
        actor: person.map(str::to_string),
        current,
        mentions: json!(mentions),
    })
}

pub async fn record(store: &Store, frame: Frame, accept: bool) -> Result<Value> {
    record_scoped(store, frame, accept, None).await
}

pub(crate) async fn record_scoped(
    store: &Store,
    frame: Frame,
    accept: bool,
    shared_image: Option<String>,
) -> Result<Value> {
    let prepared = {
        let mut conn = store.readers().acquire().await?;
        facts(&mut conn, &frame).await?
    };
    store
        .write(Mutation::Ingress(Box::new(Admission {
            frame,
            accept,
            prepared,
            shared_image,
        })))
        .await
}

pub async fn invoke(store: &Store, action: Action) -> Result<Value> {
    match action {
        Action::Record { frame, accept } => {
            record(store, serde_json::from_str(&frame)?, accept).await
        }
        Action::Claim { event } => store.write(Mutation::Claim(event)).await,
        Action::Pending => {
            let events: Vec<i64> = sqlx::query_scalar(
                "SELECT s.event_id FROM accepted_sources s LEFT JOIN executions x
                ON x.source=s.event_id WHERE x.id IS NULL AND NOT EXISTS(SELECT 1 FROM invalidated_sources WHERE source=s.event_id) ORDER BY s.event_id",
            )
            .fetch_all(store.readers())
            .await?;
            Ok(json!({"pending":events}))
        }
    }
}

pub struct Admission {
    frame: Frame,
    accept: bool,
    prepared: Facts,
    shared_image: Option<String>,
}

async fn entry_for(
    conn: &mut SqliteConnection,
    f: &Frame,
    verified: &Facts,
) -> Result<Option<String>> {
    let Some(stage) = &verified.stage else {
        return Ok(None);
    };
    if let Some(space) = &f.space {
        let entry = entry::apply(
            conn,
            &entry::Action::Group {
                space: space.clone(),
                topic: f.topic.clone(),
            },
        )
        .await?;
        return Ok(entry["entry"].as_str().map(str::to_owned));
    }
    if verified.current {
        let entry = entry::apply(
            conn,
            &entry::Action::Private {
                actor: stage.clone(),
                bot: f.bot_stage.clone(),
                topic: f.topic.clone(),
            },
        )
        .await?;
        return Ok(entry["entry"].as_str().map(str::to_owned));
    }
    Ok(sqlx::query_scalar(
        "SELECT id FROM entries WHERE actor_stage=? AND bot_stage=? AND topic IS ?
        ORDER BY closed_at IS NULL DESC,closed_at DESC LIMIT 1",
    )
    .bind(stage)
    .bind(&f.bot_stage)
    .bind(&f.topic)
    .fetch_optional(conn)
    .await?)
}

pub async fn apply(conn: &mut SqliteConnection, request: &Admission) -> Result<Value> {
    let f = &request.frame;
    let verified = facts(conn, f).await?;
    let scope = if let Some(image) = &request.shared_image {
        serde_json::to_string(&(
            "verified_group",
            image,
            f.space.as_ref().ok_or("shared key outside group")?,
            &f.topic,
        ))?
    } else {
        f.source_scope(verified.stage.as_deref())?
    };
    let existing = sqlx::query(
        "SELECT e.id,e.envelope,e.actor_stage,e.raw_actor,
        EXISTS(SELECT 1 FROM entries WHERE id=e.entry_id AND closed_at IS NULL) AS open_entry, EXISTS(SELECT 1 FROM invalidated_sources WHERE source=e.id) AS erased
        FROM events e WHERE source_scope=? AND source_key=?",
    )
    .bind(&scope)
    .bind(&f.source)
    .fetch_optional(&mut *conn)
    .await?;
    let (id, eligible) = if let Some(row) = existing {
        if row.try_get::<bool, _>("erased")? {
            return Ok(json!({"event":row.try_get::<i64,_>("id")?,"accepted":false,"erased":true}));
        }
        let mut original: Frame = serde_json::from_str(row.try_get("envelope")?)?;
        original.observed = f.observed; // Arrival timestamps may differ on redelivery.
        let shared_peer = request.shared_image.is_some() && original.bot_stage != f.bot_stage;
        if (shared_peer
            && (original.account != f.account
                || original.kind != f.kind
                || original.occurred != f.occurred))
            || (!shared_peer && original != *f)
        {
            return Err("source key reused with different envelope".into());
        }
        let same_actor = row.try_get::<Option<String>, _>("actor_stage")? == verified.stage
            && row.try_get::<Option<String>, _>("raw_actor")? == verified.actor;
        if shared_peer && !same_actor {
            return Err("conflicting actor for shared QQ key".into());
        }
        (
            row.try_get("id")?,
            same_actor && row.try_get::<bool, _>("open_entry")?,
        )
    } else {
        // A changing assignment is reclassified inside this writer transaction.
        let facts = if verified == request.prepared {
            &request.prepared
        } else {
            &verified
        };
        let entry = entry_for(conn, f, facts).await?;
        let id:i64=sqlx::query_scalar("INSERT INTO events(source_scope,source_key,entry_id,account_id,raw_actor,actor_stage,
            bot_stage,occurred_at,observed_at,envelope,mentions) VALUES(?,?,?,?,?,?,?,?,?,?,?) RETURNING id")
            .bind(&scope).bind(&f.source).bind(&entry).bind(&f.account).bind(&facts.actor).bind(&facts.stage)
            .bind(&f.bot_stage).bind(f.occurred).bind(f.observed).bind(serde_json::to_string(f)?)
            .bind(facts.mentions.to_string()).fetch_one(&mut *conn).await?;
        (id, entry.is_some())
    };
    if let Some(native) = &f.native_id {
        sqlx::query(
            "INSERT INTO event_locators(event_id,receiver,native_id) VALUES(?,?,?)
            ON CONFLICT(event_id,receiver) DO NOTHING",
        )
        .bind(id)
        .bind(&f.bot_stage)
        .bind(native)
        .execute(&mut *conn)
        .await?;
        let saved: String = sqlx::query_scalar(
            "SELECT native_id FROM event_locators WHERE event_id=? AND receiver=?",
        )
        .bind(id)
        .bind(&f.bot_stage)
        .fetch_one(&mut *conn)
        .await?;
        if saved != *native {
            return Err("conflicting locator for the same receiver".into());
        }
    }
    if request.accept && eligible && verified.current && f.ordinary() {
        sqlx::query(
            "INSERT INTO accepted_sources(event_id) VALUES(?) ON CONFLICT(event_id) DO NOTHING",
        )
        .bind(id)
        .execute(&mut *conn)
        .await?;
    }
    let accepted: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM accepted_sources WHERE event_id=?)")
            .bind(id)
            .fetch_one(conn)
            .await?;
    Ok(json!({"event":id,"accepted":accepted}))
}

pub async fn claim(conn: &mut SqliteConnection, event: i64) -> Result<Value> {
    let row = sqlx::query(
        "SELECT e.envelope,e.raw_actor,e.actor_stage,e.mentions,e.entry_id FROM accepted_sources s
        JOIN events e ON e.id=s.event_id JOIN entries dest ON dest.id=e.entry_id AND dest.closed_at IS NULL WHERE s.event_id=? AND NOT EXISTS(SELECT 1 FROM invalidated_sources WHERE source=e.id)",
    )
    .bind(event)
    .fetch_one(&mut *conn)
    .await?;
    let frame: Frame = serde_json::from_str(row.try_get("envelope")?)?;
    let verified = facts(conn, &frame).await?;
    let original = row.try_get::<String, _>("raw_actor")?;
    if !verified.current
        || verified.actor.as_deref() != Some(&original)
        || verified.stage != row.try_get::<Option<String>, _>("actor_stage")?
    {
        return Err("original source is no longer authorized".into());
    }
    let id = new_id(conn).await?;
    sqlx::query(
        "INSERT INTO executions(id,source,original_actor,state) VALUES(?,?,?,'pending')
        ON CONFLICT(source) DO NOTHING",
    )
    .bind(id)
    .bind(event)
    .bind(&original)
    .execute(&mut *conn)
    .await?;
    let execution: String = sqlx::query_scalar("SELECT id FROM executions WHERE source=?")
        .bind(event)
        .fetch_one(conn)
        .await?;
    Ok(
        json!({"execution":execution,"source":event,"original_actor":original,"entry":row.try_get::<String,_>("entry_id")?,
        "envelope":frame,"mentions":serde_json::from_str::<Value>(row.try_get("mentions")?)?}),
    )
}
