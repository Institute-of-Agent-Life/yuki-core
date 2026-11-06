//! Fetch bytes only from the installation, application and stage that actually received them.
use crate::{
    Result,
    media::{
        blob::{MAX_BYTES, Snapshot},
        handles,
    },
    store::Store,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub application: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,
}
impl Identity {
    pub fn valid(&self) -> bool {
        let ok =
            |s: &str| !s.trim().is_empty() && s.len() <= 128 && !s.chars().any(char::is_control);
        ok(&self.application) && self.tenant.as_ref().is_none_or(|s| ok(s))
    }
}
pub(super) async fn inbound_identity(
    db: &mut SqliteConnection,
    id: &str,
    generation: &str,
    p: &Value,
) -> Result<Option<Identity>> {
    let Some(v) = p.get("media_identity") else {
        return Ok(None);
    };
    super::domain::active(db, id, generation, "gateway.media").await?;
    let identity: Identity = serde_json::from_value(v.clone())?;
    let install = crate::operator::domain::get(db, &format!("plugin-install:{id}"))
        .await?
        .ok_or("missing installed gateway")?;
    if !identity.valid() || install["gateway_media"] != serde_json::to_value(&identity)? {
        return Err("gateway application or tenant differs".into());
    }
    Ok(Some(identity))
}
pub(super) async fn record(
    db: &mut SqliteConnection,
    id: &str,
    p: &Value,
    identity: &Identity,
    event: i64,
    previous: bool,
) -> Result<()> {
    let locator = json!({"source":p["source"],"native_id":p["native_id"],"native_version":p["native_version"],"space":p["space"],"topic":p["topic"],"sender":p["sender"]});
    let identity = serde_json::to_string(identity)?;
    let locator = locator.to_string();
    if previous {
        // A retransmission cannot repair missing provenance or adopt another receipt.
        if let Some((old, origin, revision, current))=sqlx::query_as::<_,(String,String,String,String)>("SELECT o.identity,o.locator,o.deployment,json_extract(a.value,'$.deployment') FROM gateway_media_origins o JOIN host_installations i ON i.plugin=o.plugin JOIN core_meta a ON a.key='plugin-approval:'||i.plugin WHERE o.event=? AND o.plugin=?").bind(event).bind(id).fetch_optional(db).await? && (old!=identity || origin!=locator || revision!=current) {return Err("original gateway receipt differs".into())}
        return Ok(());
    }
    let owns=sqlx::query("INSERT INTO gateway_media_origins SELECT ?1,i.plugin,i.revision,json_extract(a.value,'$.deployment'),i.namespace,i.platform,i.bot_stage,?3,?4 FROM host_installations i JOIN core_meta a ON a.key='plugin-approval:'||i.plugin JOIN events e ON e.id=?1 AND e.bot_stage=i.bot_stage WHERE i.plugin=?2 AND EXISTS(SELECT 1 FROM media_sources s WHERE s.event=?1) ON CONFLICT(event) DO NOTHING").bind(event).bind(id).bind(identity).bind(&locator).execute(&mut *db).await?.rows_affected();
    if owns == 1 {
        sqlx::query("UPDATE media_sources SET expires=(SELECT observed_at FROM events WHERE id=?1)+?2 WHERE event=?1 AND state='unprepared' AND expires IS NULL").bind(event).bind(handles::LIFETIME).execute(db).await?;
    }
    Ok(())
}
/// No fallback to the QQ resolver or to an arbitrary URL for a plugin event, including old unbound events.
pub(crate) async fn hosted(db: &mut SqliteConnection, event: i64) -> Result<bool> {
    Ok(
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM host_ingress WHERE event=?)")
            .bind(event)
            .fetch_one(db)
            .await?,
    )
}
pub(crate) async fn cached_authority(db: &mut SqliteConnection, event: i64) -> Result<()> {
    let ok:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM gateway_media_origins o JOIN host_installations i ON i.plugin=o.plugin AND i.enabled AND i.revision=o.revision AND i.bot_stage=o.stage AND i.platform=o.platform AND i.namespace=o.namespace JOIN core_meta a ON a.key='plugin-approval:'||i.plugin AND json_extract(a.value,'$.deployment')=o.deployment WHERE o.event=? AND EXISTS(SELECT 1 FROM json_each(i.grants) WHERE value='gateway.media') AND EXISTS(SELECT 1 FROM json_each(i.grants) WHERE value='gateway.inbound'))").bind(event).fetch_one(db).await?;
    if !ok {
        return Err("original gateway media authority unavailable".into());
    }
    Ok(())
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Gate {
    exec: String,
    actor: String,
    event: i64,
    slot: i64,
    plugin: String,
    generation: String,
    descriptor: Value,
}
async fn descriptor(db: &mut SqliteConnection, proof: &Value) -> Result<(String, String, Value)> {
    let source = proof["source"].as_i64().ok_or("missing source")?;
    let r=sqlx::query("SELECT o.*,i.generation FROM gateway_media_origins o JOIN host_installations i ON i.plugin=o.plugin AND i.revision=o.revision AND i.platform=o.platform AND i.namespace=o.namespace AND i.bot_stage=o.stage JOIN core_meta a ON a.key='plugin-approval:'||i.plugin AND json_extract(a.value,'$.deployment')=o.deployment WHERE o.event=? AND o.stage=? AND i.enabled AND i.online AND EXISTS(SELECT 1 FROM host_ingress h WHERE h.plugin=o.plugin AND h.event=o.event)").bind(source).bind(proof["stage"].as_str()).fetch_one(&mut *db).await?;
    let plugin: String = r.get("plugin");
    let generation: String = r.get("generation");
    super::domain::active(db, &plugin, &generation, "gateway.media").await?;
    super::domain::active(db, &plugin, &generation, "gateway.inbound").await?;
    let value = json!({"namespace":r.get::<String,_>("namespace"),"platform":r.get::<String,_>("platform"),"bot_stage":r.get::<String,_>("stage"),"media_identity":serde_json::from_str::<Identity>(r.get("identity"))?,"source_event":source,"attachment_index":proof["slot"],"segment_index":proof["segment"],"kind":proof["kind"],"reference":proof["origin"],"locator":serde_json::from_str::<Value>(r.get("locator"))?});
    Ok((plugin, generation, value))
}
impl Gate {
    async fn new(
        db: &mut SqliteConnection,
        exec: &str,
        actor: &str,
        event: i64,
        slot: i64,
    ) -> Result<Self> {
        let proof = handles::original(db, exec, actor, event, slot).await?;
        if proof["state"] != "unprepared" {
            return Err("original attachment unavailable".into());
        }
        let (plugin, generation, descriptor) = descriptor(db, &proof).await?;
        Ok(Self {
            exec: exec.into(),
            actor: actor.into(),
            event,
            slot,
            plugin,
            generation,
            descriptor,
        })
    }
    pub(crate) async fn check(
        &self,
        db: &mut SqliteConnection,
        id: &str,
        generation: &str,
    ) -> Result<()> {
        if self.plugin != id || self.generation != generation {
            return Err("different gateway generation".into());
        }
        let proof = handles::original(db, &self.exec, &self.actor, self.event, self.slot).await?;
        if !matches!(proof["state"].as_str(), Some("unprepared" | "cached")) {
            return Err("original attachment unavailable".into());
        }
        let (plugin, live, d) = descriptor(db, &proof).await?;
        if plugin != id || live != generation || d != self.descriptor {
            return Err("original gateway media scope changed".into());
        }
        Ok(())
    }
    pub(crate) async fn recheck(&self, db: &mut SqliteConnection) -> Result<()> {
        self.check(db, &self.plugin, &self.generation).await
    }
}
pub(crate) async fn fetch(
    store: &Store,
    exec: &str,
    actor: &str,
    event: i64,
    slot: i64,
) -> Result<(Snapshot, Gate)> {
    let gate = {
        let mut tx = store.readers().begin().await?;
        Gate::new(&mut tx, exec, actor, event, slot).await?
    };
    let install = store
        .host
        .as_ref()
        .ok_or("original gateway offline")?
        .slot(&gate.plugin)?;
    let session = install
        .session
        .lock()
        .await
        .clone()
        .filter(|s| s.generation == gate.generation)
        .ok_or("original gateway generation offline")?;
    let transfer = super::personal::credentials::random::<24>()?;
    let contract = &super::grants::methods()["methods"]["gateway.media"];
    let params = jsonschema::validator_for(&contract["params"])?;
    let result = jsonschema::validator_for(&contract["result"])?;
    let mut bytes = Vec::new();
    let mut original = None;
    loop {
        let mut request = gate.descriptor.clone();
        request["transfer"] = json!(transfer);
        request["offset"] = json!(bytes.len());
        request["length"] = json!(24576);
        if !params.is_valid(&request) {
            return Err("original gateway media locator exceeds wire bounds".into());
        }
        let reply = session.fetch_page(store, request, gate.clone()).await?;
        if !result.is_valid(&reply)
            || reply["transfer"] != transfer
            || reply["offset"] != bytes.len()
        {
            return Err("invalid original media page".into());
        }
        let size = usize::try_from(reply["size"].as_u64().ok_or("missing media size")?)?;
        let digest = reply["sha256"].as_str().ok_or("missing media digest")?;
        if size > MAX_BYTES
            || size <= bytes.len()
            || original
                .as_ref()
                .is_some_and(|x: &(usize, String)| x.0 != size || x.1 != digest)
        {
            return Err("gateway replaced original media version".into());
        }
        original.get_or_insert_with(|| (size, digest.to_owned()));
        let page = STANDARD.decode(reply["data"].as_str().ok_or("missing media page")?)?;
        if page.len() != (size - bytes.len()).min(24576) {
            return Err("incomplete or oversized gateway media page".into());
        }
        bytes.extend_from_slice(&page);
        if bytes.len() == size {
            let data = Snapshot::new(bytes)?;
            if data.digest != digest {
                return Err("original gateway content digest differs".into());
            }
            return Ok((data, gate));
        }
    }
}
