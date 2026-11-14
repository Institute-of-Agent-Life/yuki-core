use super::{Change, scope::Granted};
use crate::Result;
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Row, SqliteConnection};

pub(super) fn descriptor(kind: &str, id: &str, bytes: &[u8]) -> Value {
    json!({"kind":kind,"id":id,"sha256":format!("{:x}",Sha256::digest(bytes)),"size":bytes.len()})
}
pub(super) async fn read(db: &mut SqliteConnection, c: &Change, g: &Granted) -> Result<Value> {
    let kind = c.params["kind"].as_str().ok_or("content kind required")?;
    let id = c.params["id"].as_str().ok_or("content ID required")?;
    let value = if kind == "tool" {
        let receipt = super::tools::full(db, g, id.parse()?).await?;
        super::tools::retain(db, &c.plugin, &receipt).await?;
        receipt.value
    } else if matches!(kind, "fact" | "evidence") {
        let method = if kind == "evidence" {
            "maintenance.evidence"
        } else {
            "maintenance.fact.page"
        };
        if !g.policy.methods.contains(method) {
            return Err("content method outside installed scope".into());
        }
        let call = Change {
            plugin: c.plugin.clone(),
            generation: c.generation.clone(),
            method: method.into(),
            params: json!({"scope":c.params["scope"],"purpose":c.params["purpose"],"fact":id}),
            paid_request: None,
        };
        let page = super::reads::facts_full(db, &call, g, kind == "evidence").await?;
        page["items"]
            .as_array()
            .and_then(|items| items.first())
            .ok_or("original fact unavailable")?
            .clone()
    } else {
        if kind != "preview" || !g.policy.methods.contains("maintenance.extract.preview") {
            return Err("preview content outside installed scope".into());
        }
        let preview=sqlx::query("SELECT id FROM maintenance_previews WHERE id=? AND plugin=? AND revision=? AND scope=? AND purpose=? AND state='ready'")
            .bind(id).bind(&c.plugin).bind(&g.revision).bind(c.params["scope"].as_str()).bind(c.params["purpose"].as_str()).fetch_one(&mut *db).await?;
        let sources:Vec<(i64,String)>=sqlx::query_as("SELECT event,version FROM maintenance_sources WHERE preview=? AND role IN('primary','context') ORDER BY event")
            .bind(preview.get::<&str,_>("id")).fetch_all(&mut *db).await?;
        for (event, version) in sources {
            let current = super::reads::original(db, c, g, event).await?;
            if current.version != version {
                return Err("original preview source changed".into());
            }
        }
        let facts:Vec<(String,i64,String,String)>=sqlx::query_as("SELECT fact,revision,source_version,role FROM maintenance_fact_inputs WHERE preview=? ORDER BY fact").bind(id).fetch_all(&mut *db).await?;
        let refs=facts.into_iter().map(|(fact,revision,version,role)|json!({"fact":fact,"revision":revision,"source_version":version,"role":role})).collect::<Vec<_>>();
        super::fact_sources::validate(db, g, &json!(refs), Some(id)).await?;
        super::tools::original(db, g, id).await?;
        let view = super::preview::raw_view(db, id, false).await?;
        json!({"actions":view["actions"],"extraction_details":view["extraction_details"]})
    };
    let bytes = serde_json::to_vec(&value)?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    if c.params["sha256"] != digest {
        return Err("original content version changed".into());
    }
    let offset = c.params["offset"].as_u64().unwrap_or(0) as usize;
    if offset > bytes.len() {
        return Err("content offset outside original bytes".into());
    }
    let end = bytes
        .len()
        .min(offset + c.params["limit"].as_u64().unwrap_or(12288) as usize);
    Ok(
        json!({"id":id,"kind":kind,"sha256":digest,"size":bytes.len(),"offset":offset,"next_offset":if end<bytes.len(){Some(end)}else{None},"bytes":STANDARD.encode(&bytes[offset..end])}),
    )
}

pub(super) async fn dependencies(
    db: &mut SqliteConnection,
    c: &Change,
    g: &Granted,
) -> Result<Value> {
    let id = c.params["id"].as_str().ok_or("resource ID required")?;
    let kind = c.params["kind"].as_str().ok_or("resource kind required")?;
    let sources = if kind == "tool" {
        let receipt = super::tools::full(db, g, id.parse()?).await?;
        if c.params["source_version"] != crate::memory::scope::hash(&receipt.value) {
            return Err("original tool dependency version changed".into());
        }
        receipt.sources
    } else if kind == "fact" {
        if !g.policy.methods.contains("maintenance.fact.page")
            || crate::memory::scope::visible(db, &g.reader, Some(id), "", 1)
                .await?
                .is_empty()
        {
            return Err("fact dependencies outside installed scope".into());
        }
        if c.params["source_version"] != super::fact_sources::version(db, id).await? {
            return Err("original fact dependency version changed".into());
        }
        super::fact_sources::source_ids(db, id).await?
    } else {
        if kind != "preview" || !g.policy.methods.contains("maintenance.extract.preview") {
            return Err("preview dependencies outside installed scope".into());
        }
        sqlx::query_scalar::<_,String>("SELECT id FROM maintenance_previews WHERE id=? AND plugin=? AND revision=? AND scope=? AND purpose=? AND state='ready'")
            .bind(id).bind(&c.plugin).bind(&g.revision).bind(c.params["scope"].as_str()).bind(c.params["purpose"].as_str()).fetch_one(&mut *db).await?;
        sqlx::query_scalar("SELECT event FROM maintenance_sources WHERE preview=? ORDER BY event")
            .bind(id)
            .fetch_all(&mut *db)
            .await?
    };
    let after = c.params["after"].as_i64().unwrap_or(0);
    let remaining = sources
        .iter()
        .copied()
        .filter(|id| *id > after)
        .collect::<Vec<_>>();
    let page = remaining.iter().take(128).copied().collect::<Vec<_>>();
    Ok(
        json!({"kind":kind,"id":id,"sources":page,"source_count":sources.len(),"next":if remaining.len()>128{page.last().copied()}else{None}}),
    )
}
