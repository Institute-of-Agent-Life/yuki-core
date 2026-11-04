use super::new_id;
use crate::{
    Result,
    store::{Mutation, Store},
};
use clap::Subcommand;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

#[derive(Subcommand, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Private {
        actor: String,
        bot: String,
        #[arg(long)]
        topic: Option<String>,
    },
    Group {
        space: String,
        #[arg(long)]
        topic: Option<String>,
    },
    /// Inspect a private person's current access, including closed original history.
    Access { entry: String, person: String },
}

pub async fn invoke(store: &Store, action: Action) -> Result<Value> {
    if let Action::Access { entry, person } = action {
        // Both the requesting original and its root must still be enabled.
        let row=sqlx::query("SELECT (who.enabled AND r.enabled AND
            coalesce(who.current_root,who.id)=coalesce(owner.current_root,owner.id)) AS readable,
            (e.closed_at IS NULL AND a.status='active' AND bot.status='active' AND owner.enabled) AS routable
            FROM entries e JOIN assignments a ON a.id=e.actor_stage JOIN assignments bot ON bot.id=e.bot_stage
            JOIN persons owner ON owner.id=a.original_person JOIN persons who ON who.id=?
            JOIN persons r ON r.id=coalesce(who.current_root,who.id) WHERE e.id=? AND e.scope='private'")
            .bind(person).bind(entry).fetch_optional(store.readers()).await?;
        let (read, route) = match row {
            Some(r) => (
                r.try_get::<bool, _>("readable")?,
                r.try_get::<bool, _>("routable")?,
            ),
            None => (false, false),
        };
        return Ok(json!({"read":read,"send":read&&route}));
    }
    store.write(Mutation::Entry(action)).await
}

pub async fn apply(conn: &mut SqliteConnection, action: &Action) -> Result<Value> {
    let id = new_id(conn).await?;
    let found: String = match action {
        Action::Private { actor, bot, topic } => {
            let legal:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM assignments a JOIN persons p ON p.id=a.original_person
              JOIN persons r ON r.id=coalesce(p.current_root,p.id) JOIN assignments bot ON bot.id=?
              JOIN accounts source ON source.id=a.account_id JOIN accounts receiver ON receiver.id=bot.account_id
              WHERE source.platform=receiver.platform AND source.namespace=receiver.namespace AND a.id=? AND a.kind='person' AND bot.kind='self' AND a.status='active' AND bot.status='active' AND p.enabled AND r.enabled)")
              .bind(bot).bind(actor).fetch_one(&mut *conn).await?;
            if !legal {
                return Err("private entry requires active human and receiving bot stages".into());
            }
            sqlx::query("INSERT INTO entries(id,scope,actor_stage,bot_stage,topic) VALUES(?,'private',?,?,?)
                ON CONFLICT DO NOTHING").bind(id).bind(actor).bind(bot).bind(topic).execute(&mut *conn).await?;
            sqlx::query_scalar("SELECT id FROM entries WHERE scope='private' AND actor_stage=? AND bot_stage=? AND topic IS ? AND closed_at IS NULL")
                .bind(actor).bind(bot).bind(topic).fetch_one(conn).await?
        }
        Action::Group { space, topic } => {
            sqlx::query("INSERT INTO entries(id,scope,space,topic) VALUES(?,'group',?,?) ON CONFLICT DO NOTHING")
                .bind(id).bind(space).bind(topic).execute(&mut *conn).await?;
            sqlx::query_scalar("SELECT id FROM entries WHERE scope='group' AND space=? AND topic IS ? AND closed_at IS NULL")
                .bind(space).bind(topic).fetch_one(conn).await?
        }
        Action::Access { .. } => return Err("access inspection requires a reader".into()),
    };
    Ok(json!({"entry":found}))
}
