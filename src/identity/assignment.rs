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
    /// Without a person, this is a SELF/bot stage.
    Open {
        platform: String,
        namespace: String,
        native: String,
        observed: i64,
        #[arg(long)]
        person: Option<String>,
        #[arg(long)]
        start: Option<i64>,
    },
    Enabled {
        stage: String,
        #[arg(action=clap::ArgAction::Set)]
        enabled: bool,
        at: i64,
    },
    Transfer {
        stage: String,
        at: i64,
        observed: i64,
        #[arg(long)]
        person: Option<String>,
    },
    Resolve {
        account: String,
        observed: i64,
        #[arg(long)]
        occurred: Option<i64>,
    },
}

pub async fn invoke(store: &Store, action: Action) -> Result<Value> {
    if let Action::Resolve {
        account,
        observed,
        occurred,
    } = action
    {
        let mut conn = store.readers().acquire().await?;
        return resolve(&mut conn, &account, observed, occurred).await;
    }
    store.write(Mutation::Assignment(action)).await
}

pub async fn resolve(
    conn: &mut SqliteConnection,
    account: &str,
    observed: i64,
    occurred: Option<i64>,
) -> Result<Value> {
    // A proven prior generation can be attributed even after its route was closed.
    // When a timestamp is absent, ambiguity cannot be resolved by the arrival clock.
    let row = sqlx::query(
        "SELECT a.id,a.original_person,a.status,
        (SELECT status FROM assignment_states WHERE stage=a.id AND at<=coalesce(?2,?3)
         ORDER BY at DESC LIMIT 1) AS at_occurrence
        FROM accounts acct LEFT JOIN assignments a ON a.account_id=acct.id AND (
          (?2 IS NOT NULL AND ?2<=?3 AND ?2>=coalesce(a.started_at,a.first_observed)
           AND (a.ended_at IS NULL OR ?2<a.ended_at))
          OR (?2 IS NULL AND a.status<>'ended' AND a.first_observed<=?3
              AND (SELECT count(*) FROM assignments WHERE account_id=acct.id)=1))
        WHERE acct.id=?1",
    )
    .bind(account)
    .bind(occurred)
    .bind(observed)
    .fetch_one(conn)
    .await?;
    Ok(
        json!({"account":account,"assignment":row.try_get::<Option<String>,_>("id")?,
        "original":row.try_get::<Option<String>,_>("original_person")?,
        "status":row.try_get::<Option<String>,_>("status")?,
        "status_at_source":row.try_get::<Option<String>,_>("at_occurrence")?,
        "occurred":occurred,"observed":observed}),
    )
}

async fn insert(
    conn: &mut SqliteConnection,
    account: &str,
    person: &Option<String>,
    start: Option<i64>,
    observed: i64,
    previous: Option<&str>,
) -> Result<Value> {
    let stage = new_id(conn).await?;
    sqlx::query("INSERT INTO assignments(id,account_id,original_person,kind,status,first_observed,started_at,previous)
        VALUES(?,?,?,?,'active',?,?,?)").bind(&stage).bind(account).bind(person)
        .bind(if person.is_some(){"person"}else{"self"}).bind(observed).bind(start).bind(previous)
        .execute(&mut *conn).await?;
    sqlx::query("INSERT INTO assignment_states(stage,at,status) VALUES(?,?,'active')")
        .bind(&stage)
        .bind(start.unwrap_or(observed))
        .execute(conn)
        .await?;
    Ok(json!({"account":account,"assignment":stage}))
}

pub async fn apply(conn: &mut SqliteConnection, action: &Action) -> Result<Value> {
    match action {
        Action::Open {
            platform,
            namespace,
            native,
            observed,
            person,
            start,
        } => {
            let id = new_id(conn).await?;
            sqlx::query(
                "INSERT INTO accounts(id,platform,namespace,native) VALUES(?,?,?,?)
                ON CONFLICT(platform,namespace,native) DO NOTHING",
            )
            .bind(id)
            .bind(platform)
            .bind(namespace)
            .bind(native)
            .execute(&mut *conn)
            .await?;
            let account: String = sqlx::query_scalar(
                "SELECT id FROM accounts WHERE platform=? AND namespace=? AND native=?",
            )
            .bind(platform)
            .bind(namespace)
            .bind(native)
            .fetch_one(&mut *conn)
            .await?;
            if let Some(r)=sqlx::query("SELECT id,original_person,previous,started_at FROM assignments WHERE account_id=? AND status<>'ended'")
                .bind(&account).fetch_optional(&mut *conn).await? {
                if r.try_get::<Option<String>,_>("original_person")?!=*person || r.try_get::<Option<String>,_>("previous")?.is_some()
                    || r.try_get::<Option<i64>,_>("started_at")?!=*start {return Err("account has a different current assignment".into());}
                return Ok(json!({"account":account,"assignment":r.try_get::<String,_>("id")?}));
            }
            insert(conn, &account, person, *start, *observed, None).await
        }
        Action::Enabled { stage, enabled, at } => {
            let status = if *enabled { "active" } else { "disabled" };
            if let Some(old) = sqlx::query_scalar::<_, String>(
                "SELECT status FROM assignment_states WHERE stage=? AND at=?",
            )
            .bind(stage)
            .bind(at)
            .fetch_optional(&mut *conn)
            .await?
            {
                if old != status {
                    return Err("conflicting assignment transition".into());
                }
                return Ok(json!({"assignment":stage,"status":status}));
            }
            sqlx::query_scalar::<_,String>("UPDATE assignments SET status=? WHERE id=? AND status<>'ended'
                AND ?>(SELECT max(at) FROM assignment_states WHERE stage=assignments.id) RETURNING id")
                .bind(status).bind(stage).bind(at).fetch_one(&mut *conn).await?;
            sqlx::query("INSERT INTO assignment_states(stage,at,status) VALUES(?,?,?)")
                .bind(stage)
                .bind(at)
                .bind(status)
                .execute(conn)
                .await?;
            Ok(json!({"assignment":stage,"status":status}))
        }
        Action::Transfer {
            stage,
            at,
            observed,
            person,
        } => {
            if at > observed {
                return Err("effective transfer is later than its observation".into());
            }
            if let Some(row) = sqlx::query(
                "SELECT id,account_id,original_person,started_at FROM assignments WHERE previous=?",
            )
            .bind(stage)
            .fetch_optional(&mut *conn)
            .await?
            {
                if row.try_get::<Option<String>, _>("original_person")? != *person
                    || row.try_get::<Option<i64>, _>("started_at")? != Some(*at)
                {
                    return Err("previous stage already transferred with different terms".into());
                }
                return Ok(
                    json!({"account":row.try_get::<String,_>("account_id")?,"assignment":row.try_get::<String,_>("id")?}),
                );
            }
            let account:String=sqlx::query_scalar("UPDATE assignments SET ended_at=?,status='ended'
                WHERE id=? AND status<>'ended' AND ?>(SELECT max(at) FROM assignment_states WHERE stage=assignments.id) RETURNING account_id")
                .bind(at).bind(stage).bind(at).fetch_one(&mut *conn).await?;
            sqlx::query("INSERT INTO assignment_states(stage,at,status) VALUES(?,?,'ended')")
                .bind(stage)
                .bind(at)
                .execute(&mut *conn)
                .await?;
            sqlx::query("UPDATE entries SET closed_at=? WHERE closed_at IS NULL AND (actor_stage=? OR bot_stage=?)")
                .bind(at).bind(stage).bind(stage).execute(&mut *conn).await?;
            insert(conn, &account, person, Some(*at), *observed, Some(stage)).await
        }
        Action::Resolve { .. } => Err("resolution requires a reader".into()),
    }
}
