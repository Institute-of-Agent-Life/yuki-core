use crate::{
    Result,
    store::{Mutation, Store},
};
use clap::Subcommand;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection, SqlitePool};

/// Commands on the protected local operator channel. A device ID is never a bearer credential.
#[derive(Subcommand, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Create {
        id: String,
        #[arg(long, default_value = "")]
        name: String,
    },
    Get {
        id: String,
    },
    Merge {
        from: String,
        into: String,
    },
    SetEnabled {
        id: String,
        #[arg(action = clap::ArgAction::Set)]
        enabled: bool,
    },
    SetAdmin {
        id: String,
        #[arg(action = clap::ArgAction::Set)]
        admin: bool,
    },
    BindDevice {
        device: String,
        original: String,
    },
    CheckDevice {
        device: String,
    },
}

pub enum Change {
    Rename(String, String),
    Create(String, String),
    Merge(String, String),
    Enable(String, bool),
    Admin(String, bool),
    BindDevice(String, String),
}

pub async fn invoke(store: &Store, action: Action) -> Result<Value> {
    let change = match action {
        Action::Get { id } => return view(store.readers(), &id).await,
        Action::CheckDevice { device } => return device_principal(store.readers(), &device).await,
        Action::Create { id, name } => Change::Create(id, name),
        Action::Merge { from, into } => Change::Merge(from, into),
        Action::SetEnabled { id, enabled } => Change::Enable(id, enabled),
        Action::SetAdmin { id, admin } => Change::Admin(id, admin),
        Action::BindDevice { device, original } => Change::BindDevice(device, original),
    };
    store.write(Mutation::Person(change)).await
}

async fn view(pool: &SqlitePool, id: &str) -> Result<Value> {
    let row = sqlx::query(
        "SELECT p.id, p.name, p.enabled, p.revision, r.id AS root, r.enabled AS root_enabled, r.admin,
        (SELECT json_group_array(id) FROM
          (SELECT id FROM persons WHERE id = r.id OR current_root = r.id ORDER BY id)) AS family
        FROM persons p JOIN persons r ON r.id = coalesce(p.current_root,p.id) WHERE p.id = ?")
        .bind(id).fetch_one(pool).await?;
    Ok(
        json!({"original":id,"root":row.try_get::<String,_>("root")?,
        "name":row.try_get::<String,_>("name")?, "enabled":row.try_get::<bool,_>("enabled")?,
        "revision":row.try_get::<i64,_>("revision")?, "root_enabled":row.try_get::<bool,_>("root_enabled")?,
        "admin":row.try_get::<bool,_>("admin")? && row.try_get::<bool,_>("enabled")? && row.try_get::<bool,_>("root_enabled")?,
        "family":serde_json::from_str::<Vec<String>>(row.try_get("family")?)?}),
    )
}

/// Resolve the authenticated device's stored origin at execution time, including both enable flags.
async fn device_principal(pool: &SqlitePool, device: &str) -> Result<Value> {
    let row = sqlx::query(
        "SELECT p.id AS original,r.id AS root,p.enabled AND r.enabled AS allowed,r.admin
        FROM device_origins d JOIN persons p ON p.id=d.original_person
        JOIN persons r ON r.id=coalesce(p.current_root,p.id) WHERE d.device_id=?",
    )
    .bind(device)
    .fetch_one(pool)
    .await?;
    if !row.try_get::<bool, _>("allowed")? {
        return Err("device original person or current root disabled".into());
    }
    Ok(
        json!({"original":row.try_get::<String,_>("original")?, "root":row.try_get::<String,_>("root")?,
              "admin":row.try_get::<bool,_>("admin")?}),
    )
}

async fn root(conn: &mut SqliteConnection, id: &str) -> Result<String> {
    Ok(
        sqlx::query_scalar("SELECT coalesce(current_root,id) FROM persons WHERE id=?")
            .bind(id)
            .fetch_one(conn)
            .await?,
    )
}

pub async fn apply(conn: &mut SqliteConnection, change: &Change) -> Result<Value> {
    match change {
        Change::Rename(id, name) => {
            let name = name.trim();
            if name.chars().count() > 128 {
                return Err("person name exceeds 128 characters".into());
            }
            sqlx::query("INSERT OR IGNORE INTO person_names SELECT id,name FROM persons WHERE id=? AND name<>'' AND name<>?")
                .bind(id).bind(name).execute(&mut *conn).await?;
            let revision: i64 = sqlx::query_scalar("UPDATE persons SET name=?,revision=revision+(name<>?) WHERE id=? RETURNING revision")
                .bind(name).bind(name).bind(id).fetch_one(conn).await?;
            Ok(json!({"original":id,"name":name,"revision":revision}))
        }
        Change::Create(id, name) => {
            if name.chars().count() > 128 {
                return Err("person name exceeds 128 characters".into());
            }
            sqlx::query("INSERT INTO persons(id,name) VALUES(?,?)")
                .bind(id)
                .bind(name)
                .execute(conn)
                .await?;
            Ok(json!({"original":id,"root":id}))
        }
        Change::Merge(from, into) => {
            let source = root(conn, from).await?;
            let target = root(conn, into).await?;
            if source != target {
                // Combine only the two current roots' grants. Old aliases cannot revive a revoked grant.
                sqlx::query("UPDATE persons SET
                    name=CASE WHEN name='' THEN (SELECT name FROM persons WHERE id=?) ELSE name END,
                    admin=admin OR (SELECT admin FROM persons WHERE id=?), revision=revision+1 WHERE id=?")
                    .bind(&source).bind(&source).bind(&target).execute(&mut *conn).await?;
                sqlx::query(
                    "UPDATE persons SET current_root=?, admin=0, revision=revision+1
                    WHERE id=? OR current_root=?",
                )
                .bind(&target)
                .bind(&source)
                .bind(&source)
                .execute(conn)
                .await?;
            }
            Ok(json!({"original":from,"root":target,"changed":source!=target}))
        }
        Change::Enable(id, enabled) => {
            let revision: i64 = sqlx::query_scalar(
                "UPDATE persons SET enabled=?,revision=revision+(enabled<>?)
                WHERE id=? RETURNING revision",
            )
            .bind(enabled)
            .bind(enabled)
            .bind(id)
            .fetch_one(conn)
            .await?;
            Ok(json!({"original":id,"enabled":enabled,"revision":revision}))
        }
        Change::Admin(id, admin) => {
            let target = root(conn, id).await?;
            sqlx::query("UPDATE persons SET admin=?,revision=revision+(admin<>?) WHERE id=?")
                .bind(admin)
                .bind(admin)
                .bind(&target)
                .execute(conn)
                .await?;
            Ok(json!({"original":id,"root":target,"admin":admin}))
        }
        Change::BindDevice(device, original) => {
            sqlx::query(
                "INSERT INTO device_origins(device_id,original_person) VALUES(?,?)
                ON CONFLICT(device_id) DO NOTHING",
            )
            .bind(device)
            .bind(original)
            .execute(&mut *conn)
            .await?;
            let owner: String =
                sqlx::query_scalar("SELECT original_person FROM device_origins WHERE device_id=?")
                    .bind(device)
                    .fetch_one(conn)
                    .await?;
            if owner != *original {
                return Err("device already bound to another original person".into());
            }
            Ok(json!({"device":device,"original":original}))
        }
    }
}
