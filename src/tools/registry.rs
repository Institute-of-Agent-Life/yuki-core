use crate::{Result, history, social};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::SqliteConnection;
use std::collections::BTreeMap;

#[derive(Clone, Deserialize, Serialize)]
pub struct Provider {
    pub id: String,
    pub revision: String,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Tool {
    pub name: String,
    pub description: String,
    pub schema: Value,
    pub parallel_read: bool,
    pub cacheable: bool,
    pub code_direct: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<Provider>,
}
impl Tool {
    /// Directory pages show a short description. Exact-name lookup returns the entire
    /// frozen declaration, even when the provider wrote a long instruction manual.
    fn summary(&self) -> String {
        let first = self
            .description
            .split_inclusive(['。', '！', '？', '\n'])
            .next()
            .unwrap_or("")
            .trim();
        let first = first
            .split_once(". ")
            .map_or(first, |(sentence, _)| sentence);
        let mut chars = first.chars();
        let mut text: String = chars.by_ref().take(120).collect();
        if chars.next().is_some() {
            text.push('…');
        }
        text
    }
    pub fn wire(&self) -> Value {
        json!({"type":"function","function":{"name":self.name,"description":self.description,"parameters":self.schema}})
    }
}
fn object(props: Value, required: &[&str]) -> Value {
    json!({"type":"object","properties":props,"required":required,"additionalProperties":false})
}
pub fn builtin(name: &str) -> Option<Tool> {
    if let Some(tool) = crate::media::declaration(name) {
        return Some(tool);
    }
    if let Some(tool) = crate::history::query::contacts::declaration(name) {
        return Some(tool);
    }
    if let Some(tool) = crate::memory::tools::declaration(name) {
        return Some(tool);
    }
    if let Some(tool) = crate::work::schedule::declaration(name) {
        return Some(tool);
    }
    if let Some(tool) = crate::work::declaration(name) {
        return Some(tool);
    }
    if name == "web_search" {
        return Some(crate::model::search::declaration());
    }
    let target = json!({"type":"string","minLength":1});
    let (description, schema, parallel_read, cacheable, code_direct) = match name {
        "get" => (
            "Read the original actor's current local value.",
            object(json!({"target":target}), &["target"]),
            true,
            false,
            false,
        ),
        "put" => (
            "Commit a local value to the original actor.",
            object(json!({"target":target,"value":{}}), &["target", "value"]),
            false,
            false,
            false,
        ),
        "send" => (
            "Deliver content through its original social route.",
            social::content::schema(),
            false,
            false,
            true,
        ),
        "complete" => (
            "Submit a completion to the registered binding.",
            object(json!({"summary":{"type":"string"}}), &["summary"]),
            false,
            false,
            false,
        ),
        "history_query" => (
            "Read authorized original conversation history.",
            history::query::schema(),
            true,
            false,
            true,
        ),
        "history_directory" => (
            "List observed entries and subjects in the original history scope.",
            object(json!({}), &[]),
            true,
            false,
            false,
        ),
        "time_get_current" => (
            "Read current UTC and local date and time.",
            object(json!({}), &[]),
            true,
            false,
            true,
        ),
        "update_short_state" => (
            "Replace one of three 24-hour slots. Empty text clears it.",
            object(
                json!({"slot":{"type":"integer","minimum":1,"maximum":3},"text":{"type":"string","maxLength":300}}),
                &["slot", "text"],
            ),
            false,
            false,
            true,
        ),
        "read_tool_artifact" => (
            "Continue a specific immutable result at a JSON pointer and character or item offset.",
            object(
                json!({"handle":{"type":"string"},"version":{"type":"string"},"path":{"type":"string"},"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":8000}}),
                &["handle", "version"],
            ),
            true,
            false,
            true,
        ),
        "lookup_tools" => (
            "Look up a frozen schema or page through capability summaries.",
            object(
                json!({"query":{"type":"string","maxLength":128},"name":{"type":"string","minLength":1,"maxLength":256},"limit":{"type":"integer","minimum":1,"maximum":10},"offset":{"type":"integer","minimum":0}}),
                &[],
            ),
            true,
            true,
            true,
        ),
        _ => return None,
    };
    Some(Tool {
        name: name.into(),
        description: description.into(),
        schema,
        parallel_read,
        cacheable,
        code_direct,
        provider: None,
    })
}
#[derive(Deserialize, Serialize)]
pub struct Manifest(pub BTreeMap<String, Tool>);
impl Manifest {
    pub fn freeze(tools: impl IntoIterator<Item = Tool>) -> Result<Self> {
        let mut all = BTreeMap::new();
        for tool in tools {
            let name = tool.name.clone();
            if all.insert(name.clone(), tool).is_some() {
                return Err(format!("duplicate tool name: {name}").into());
            }
        }
        Ok(Self(all))
    }
    pub fn core() -> Self {
        Self::freeze(
            [
                "get",
                "put",
                "send",
                "complete",
                "history_query",
                "history_directory",
                "find_contacts",
                "find_conversations",
                "time_get_current",
                "update_short_state",
                "read_tool_artifact",
                "inspect_conversation_attachment",
                "workspace_inspect",
                "lookup_tools",
                "read_memory",
                "memory_change",
                "automation_query",
                "automation_manage",
                "task_control",
                "subagent_start",
                "subagent_control",
            ]
            .into_iter()
            .map(|name| builtin(name).expect("static core registry")),
        )
        .expect("unique static tools")
    }
    pub fn revision(&self) -> Result<String> {
        Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(self)?)))
    }
    pub fn declarations(&self, code: bool) -> Vec<Value> {
        self.0
            .values()
            .filter(|t| {
                if code {
                    t.code_direct
                } else {
                    t.name != "lookup_tools"
                }
            })
            .map(Tool::wire)
            .collect()
    }
    pub fn lookup(&self, args: &Value, code: bool) -> Result<Value> {
        if let Some(name) = args["name"].as_str() {
            if args.as_object().is_none_or(|a| a.len() != 1) {
                return Err("name cannot be combined with query or paging".into());
            }
            let t = self.0.get(name).ok_or("unknown tool")?;
            return Ok(
                json!({"name":name,"description":t.description,"parameters":t.schema,"script_name":script_name(name),"direct":if code{t.code_direct}else{name!="lookup_tools"},"executed":false}),
            );
        }
        let offset = usize::try_from(args["offset"].as_u64().unwrap_or(0))?;
        let limit = args["limit"].as_u64().unwrap_or(5);
        if !(1..=10).contains(&limit) {
            return Err("lookup limit outside range".into());
        }
        let q = args["query"].as_str().unwrap_or("").to_lowercase();
        let hits: Vec<_> = self
            .0
            .values()
            .filter(|t| {
                format!("{} {}", t.name, t.description)
                    .to_lowercase()
                    .contains(&q)
            })
            .collect();
        let rows: Vec<_> = hits
            .iter()
            .skip(offset)
            .take(limit as usize)
            .map(|t| json!({"name":t.name,"description":t.summary()}))
            .collect();
        Ok(
            json!({"items":rows,"next_offset":offset.checked_add(rows.len()).filter(|n|*n<hits.len()),"executed":false}),
        )
    }
}
fn script_name(name: &str) -> String {
    if !name.is_empty()
        && !name.contains("__")
        && name
            .bytes()
            .enumerate()
            .all(|(i, b)| b == b'_' || b.is_ascii_lowercase() || i > 0 && b.is_ascii_digit())
    {
        return format!("yuki_{name}");
    }
    let hex: String = name.bytes().map(|b| format!("{b:02x}")).collect();
    format!("yuki__x{hex}")
}
pub async fn pinned(
    conn: &mut SqliteConnection,
    exec: &str,
    cfg: &crate::runtime::main::Config,
) -> Result<(Manifest, bool)> {
    if let Some((body,original))=sqlx::query_as::<_,(String,bool)>("SELECT m.body,v.code_projection FROM execution_tool_views v JOIN tool_manifests m USING(revision) WHERE v.execution=?").bind(exec).fetch_optional(&mut *conn).await?{
        return Ok((serde_json::from_str(&body)?,original));
    }
    let code = cfg.code_projection;
    let mut tools: Vec<Tool> = Manifest::core().0.into_values().collect();
    if cfg.bridge_search {
        tools.push(crate::model::search::declaration());
    }
    let installed: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT plugin,revision,tools FROM host_installations WHERE enabled ORDER BY plugin",
    )
    .fetch_all(&mut *conn)
    .await?;
    for (id, revision, raw) in installed {
        for mut tool in serde_json::from_str::<Vec<Tool>>(&raw)? {
            tool.provider = Some(Provider {
                id: id.clone(),
                revision: revision.clone(),
            });
            tools.push(tool);
        }
    }
    let manifest = Manifest::freeze(tools)?;
    let rev = manifest.revision()?;
    sqlx::query("INSERT INTO tool_manifests VALUES(?,?) ON CONFLICT(revision) DO NOTHING")
        .bind(&rev)
        .bind(serde_json::to_string(&manifest)?)
        .execute(&mut *conn)
        .await?;
    sqlx::query("INSERT INTO execution_tool_views VALUES(?,?,?,?,?,?)")
        .bind(exec)
        .bind(rev)
        .bind(code)
        .bind(
            cfg.result_page_budget
                .unwrap_or(super::results::DEFAULT_PAGE) as i64,
        )
        .bind(
            cfg.result_storage_bytes
                .unwrap_or(super::results::DEFAULT_QUOTA),
        )
        .bind(cfg.timezone.as_deref().unwrap_or("Asia/Shanghai"))
        .execute(conn)
        .await?;
    Ok((manifest, code))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sorted_hidden_schemas_are_versioned_and_collisions_fail() {
        let a = builtin("history_directory").unwrap();
        let b = builtin("send").unwrap();
        assert!(
            Manifest::freeze([a.clone(), a.clone()])
                .err()
                .unwrap()
                .to_string()
                .contains("history_directory")
        );
        let m = Manifest::freeze([a.clone(), b.clone()]).unwrap();
        assert_eq!(
            m.revision().unwrap(),
            Manifest::freeze([b.clone(), a.clone()])
                .unwrap()
                .revision()
                .unwrap()
        );
        let mut changed = a;
        changed.schema["properties"] = json!({"different":{"type":"string"}});
        let new = Manifest::freeze([b, changed]).unwrap();
        assert_eq!(m.declarations(true), new.declarations(true));
        assert_ne!(m.revision().unwrap(), new.revision().unwrap());
        assert_eq!(
            m.lookup(&json!({"name":"history_directory"}), true)
                .unwrap()["script_name"],
            "yuki_history_directory"
        );
        assert_eq!(script_name("ext/a"), "yuki__x6578742f61");
    }
}
