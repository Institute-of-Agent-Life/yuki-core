use super::{extract, retrieve, scope, tools::bind_sources};
use crate::{Result, store::Store};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::SqliteConnection;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Target {
    scope: String,
    subject_ref: Option<String>,
    display_name: Option<String>,
    person_id: Option<String>,
    space_name: Option<String>,
    space_id: Option<String>,
}
impl Target {
    pub(super) async fn select(
        &self,
        db: &mut SqliteConnection,
        r: &scope::Reader,
        exec: &str,
        ids: &str,
    ) -> Result<String> {
        let spaces = serde_json::to_string(&r.base.entries)?;
        let space = match (&self.space_id, &self.space_name) {
            (None, None) => None,
            (id, name) if id.is_some() != name.is_some() => {
                let found:Vec<String> = sqlx::query_scalar("SELECT DISTINCT d.space FROM entries d JOIN json_each(?1) a ON a.value=d.id WHERE d.scope='group' AND (d.space=?2 OR (?3 IS NOT NULL AND EXISTS(SELECT 1 FROM events e WHERE e.entry_id=d.id AND json_extract(e.envelope,'$.group_name')=?3 AND NOT EXISTS(SELECT 1 FROM invalidated_sources i WHERE i.source=e.id)))) ORDER BY d.space LIMIT 2").bind(&spaces).bind(id).bind(name).fetch_all(&mut *db).await?;
                let [one] = found.as_slice() else {
                    return Err("group selector absent or ambiguous within authorized scope".into());
                };
                Some(one.clone())
            }
            _ => return Err("use a single group selector".into()),
        };
        let people = [&self.person_id, &self.display_name, &self.subject_ref]
            .into_iter()
            .flatten()
            .count();
        let person = match self.scope.as_str() {
            "person" if people == 1 => {
                let raw = if let Some(id) = &self.person_id {
                    id.clone()
                } else if let Some(name) = &self.display_name {
                    let found:Vec<String>=sqlx::query_scalar("SELECT DISTINCT coalesce(p.current_root,p.id) FROM persons p WHERE p.enabled AND (coalesce(p.current_root,p.id)=?1 OR EXISTS(SELECT 1 FROM memory_group_relations a JOIN memory_group_relations b USING(space) WHERE a.person=?1 AND b.person=coalesce(p.current_root,p.id))) AND (p.name=?2 OR EXISTS(SELECT 1 FROM events e JOIN json_each(?3) a ON a.value=e.entry_id WHERE e.raw_actor=p.id AND ?2 IN(json_extract(e.envelope,'$.sender.card'),json_extract(e.envelope,'$.sender.nickname')) AND NOT EXISTS(SELECT 1 FROM invalidated_sources i WHERE i.source=e.id))) LIMIT 2").bind(&r.root).bind(name).bind(&spaces).fetch_all(&mut *db).await?;
                    let [one] = found.as_slice() else {
                        return Err(
                            "person name absent or ambiguous within authorized scope".into()
                        );
                    };
                    one.clone()
                } else if self.subject_ref.as_deref() == Some("current_speaker") {
                    r.root.clone()
                } else {
                    let reference = self
                        .subject_ref
                        .as_deref()
                        .ok_or("original subject required")?;
                    let id: i64 = sqlx::query_scalar(
                        "SELECT source FROM executions WHERE id=? AND source IS NOT NULL",
                    )
                    .bind(exec)
                    .fetch_one(&mut *db)
                    .await?;
                    let event = extract::source::load(db, id, "UTC")
                        .await?
                        .ok_or("source no longer available")?;
                    let key = if reference == "replied_message_author" {
                        "reply_author"
                    } else if reference.starts_with("member_") {
                        reference
                    } else {
                        return Err("unsupported original subject".into());
                    };
                    event
                        .aliases
                        .get(key)
                        .ok_or("subject absent from original event")?
                        .clone()
                };
                let owner:Option<String>=sqlx::query_scalar("SELECT coalesce(p.current_root,p.id) FROM persons p JOIN persons root ON root.id=coalesce(p.current_root,p.id) WHERE p.id=?1 AND p.enabled AND root.enabled AND ((?3 IS NULL AND root.id=?2) OR EXISTS(SELECT 1 FROM memory_group_relations a JOIN memory_group_relations b USING(space) WHERE a.person=?2 AND b.person=root.id AND (?3 IS NULL OR a.space=?3)))").bind(raw).bind(&r.root).bind(&space).fetch_optional(&mut *db).await?;
                Some(owner.ok_or("person unavailable in selected scope")?)
            }
            "self" if people == 0 && space.is_none() => None,
            "group" if people == 0 && space.is_some() => None,
            _ => return Err("invalid memory target shape".into()),
        };
        let filtered:Vec<String>=sqlx::query_scalar("SELECT f.id FROM json_each(?1) a JOIN memory_facts f ON f.id=a.value LEFT JOIN persons p ON p.id=f.person WHERE (?2='self' AND f.kind='self') OR (?2='group' AND f.kind='group' AND f.space=?4) OR (?2='person' AND coalesce(p.current_root,p.id)=?3 AND (f.kind='person' OR (f.kind='person_group' AND (?4 IS NULL OR f.space=?4))))").bind(ids).bind(&self.scope).bind(person).bind(space).fetch_all(db).await?;
        Ok(serde_json::to_string(&filtered)?)
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    query: String,
    target: Option<Target>,
    limit: Option<usize>,
    purpose: Option<String>,
    start_at: Option<String>,
    end_at: Option<String>,
    temporal_constraint: Option<String>,
}
impl Request {
    fn query(&self, exec: &str, actor: &str, op: &str) -> Result<retrieve::Query> {
        let limit = self.limit.unwrap_or(10);
        if !(1..=100).contains(&limit)
            || !matches!(
                self.purpose.as_deref(),
                None | Some("recall" | "background" | "continuation" | "correct" | "verify")
            )
            || !matches!(
                self.temporal_constraint.as_deref(),
                None | Some("strict" | "soft")
            )
        {
            return Err("invalid memory search intent".into());
        }
        Ok(retrieve::Query {
            execution: exec.into(),
            actor: actor.into(),
            operation: op.into(),
            text: self.query.clone(),
            start: self.start_at.clone(),
            end: self.end_at.clone(),
            limit,
            candidates: 50.max(limit),
            backend: retrieve::Backend::Exact,
            explain: false,
        })
    }
}
pub(crate) async fn perform(
    store: &Store,
    exec: &str,
    actor: &str,
    op: &str,
    raw: &str,
) -> Result<()> {
    let result = async {
        let a: Request = serde_json::from_str(raw)?;
        retrieve::scoped(store, &a.query(exec, actor, op)?, a.target.as_ref()).await
    }
    .await;
    crate::tools::settle(store, exec, actor, op, result).await
}
pub(crate) async fn confirm(
    db: &mut SqliteConnection,
    exec: &str,
    actor: &str,
    args: &Value,
    v: &Value,
) -> Result<Value> {
    let request: Request = serde_json::from_value(args.clone())?;
    let reader = scope::Reader::at(db, exec, actor, false, false).await?;
    let ids = serde_json::to_string(&scope::visible(db, &reader, None, "", i64::MAX).await?)?;
    let selected = match request.target.as_ref() {
        Some(t) => t.select(db, &reader, exec, &ids).await?,
        None => ids,
    };
    let hits = v["items"].as_array().ok_or("invalid retrieval result")?;
    for hit in hits {
        let fact = hit["fact"].as_str().ok_or("missing original fact")?;
        let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM json_each(?1) a JOIN retrieval_documents f ON f.id=a.value WHERE f.id=?2 AND f.revision=?3 AND (f.valid_until IS NULL OR julianday(f.valid_until)>julianday('now')))").bind(&selected).bind(fact).bind(hit["revision"].as_i64()).fetch_one(&mut *db).await?;
        if !valid {
            return Err("memory scope or revision changed before publication".into());
        }
    }
    for hit in hits {
        bind_sources(db, exec, hit["fact"].as_str().ok_or("invalid fact")?).await?;
    }
    let partial = v["partial_failure"] == true;
    let lexical = v["mode"] == "lexical-only";
    Ok(
        json!({"memories":hits,"returned_count":hits.len(),"authorized":v["authorized"],"result_scope":if request.target.is_some(){"explicit_targets"}else{"authorized_maximum"},"semantic_status":if lexical{"not_configured"}else if partial{"degraded"}else{"ready"},"semantic_degraded":partial,"semantic_approximate":v["approximate"],"stale_count":v["stale_count"],"truncated":v["truncated"],"partial_failure":partial,"partial_reason":if lexical{Some("semantic_not_configured")}else if partial{Some("index_or_provider_degraded")}else{None},"exhaustive":!lexical&&!partial&&v["truncated"]==false,"effective_query":{"mode":"hybrid","purpose":request.purpose.as_deref().unwrap_or("recall"),"start_at":request.start_at,"end_at":request.end_at,"interval":"start_inclusive_end_exclusive"}}),
    )
}
