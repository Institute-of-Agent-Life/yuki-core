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
    let mut tool = builtin_unprojected(name)?;
    tool.code_direct = super::original_views::direct(name);
    Some(tool)
}
fn builtin_unprojected(name: &str) -> Option<Tool> {
    if let Some(tool) = super::original_views::declaration(name) {
        return Some(tool);
    }
    if name == "get_my_capabilities" {
        return Some(super::permissions::declaration());
    }
    if name == "recall_own_message" {
        return Some(crate::social::recall::declaration());
    }
    if name == "get_group_members" {
        return Some(crate::history::query::members::declaration());
    }
    if name == "query_chat_history" {
        return Some(crate::history::query::original::declaration());
    }
    if let Some(tool) = crate::history::query::contacts::declaration(name) {
        return Some(tool);
    }
    if let Some(tool) = crate::media::declaration(name) {
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
        "read_tool_artifact" => (
            "读取工具产生的短期 Artifact。默认 get 按路径读取 JSON，read_more 中的参数项可原样传入；可用 inspect 查看结构、search 返回关键词命中的完整对象；文本使用 text。get 字符串按字符分页，offset/next_offset 单位为 characters；limit 默认当前有效结果字符预算，实际页受该预算约束。图片 Artifact 使用 image 将原图交给当前主模型原生查看。",
            json!({"type": "object", "properties": {"handle": {"type": "string"}, "operation": {"type": "string", "enum": ["inspect", "get", "search", "text", "image"], "description": "默认 get；JSON 使用 inspect/get/search，文本使用 text，图片使用 image"}, "path": {"type": "array", "items": {"anyOf": [{"type": "string"}, {"type": "integer"}]}, "description": "相对返回中 logical_root 的 JSON 路径，对象键用字符串、数组下标用整数"}, "offset": {"type": "integer", "minimum": 0, "description": "JSON 的键/元素/匹配偏移，或 text 字符偏移"}, "limit": {"type": "integer", "minimum": 1, "description": "JSON 条目数，或 text 最大字符数"}, "query": {"type": "string", "description": "search 关键词，或 text 的字符串定位词"}}, "required": ["handle"], "additionalProperties": false}),
            true,
            false,
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
                "recall_own_message",
                "complete",
                "history_query",
                "query_chat_history",
                "history_directory",
                "get_group_members",
                "get_my_capabilities",
                "find_contacts",
                "find_conversations",
                "time_get_current",
                "update_short_state",
                "read_tool_artifact",
                "inspect_conversation_attachment",
                "workspace_inspect",
                "lookup_tools",
                "read_memory",
                "search_memory",
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
                    !["lookup_tools", "execute_code"].contains(&t.name.as_str())
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
pub(super) fn script_name(name: &str) -> String {
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
    let code = cfg.code_projection || cfg.code_tool.is_some();
    let mut tools: Vec<Tool> = Manifest::core().0.into_values().collect();
    if cfg.bridge_search {
        tools.push(builtin("web_search").expect("built-in configured search"));
    }
    let installed: Vec<(String, String, String, Option<String>)> = sqlx::query_as(
        "SELECT i.plugin,i.revision,i.tools,CASE WHEN coalesce((SELECT value FROM core_meta WHERE key='external-search-enabled'),'false')!='true' OR NOT EXISTS(SELECT 1 FROM json_each(i.grants) WHERE value='search.external') THEN json_extract(m.value,'$.external_search.tool') END FROM host_installations i LEFT JOIN core_meta m ON m.key='plugin-install:'||i.plugin WHERE i.enabled ORDER BY i.plugin",
    )
    .fetch_all(&mut *conn)
    .await?;
    for (id, revision, raw, disabled_search) in installed {
        for mut tool in serde_json::from_str::<Vec<Tool>>(&raw)? {
            if disabled_search.as_deref() == Some(&tool.name) {
                continue;
            }
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
        .execute(&mut *conn)
        .await?;
    crate::host::selected_processor::pin(conn, exec, cfg.attachment_processor.as_deref()).await?;
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
