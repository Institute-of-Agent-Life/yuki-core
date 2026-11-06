//! Original model discovery contract. Host SDK code.lookup keeps its own wire version.
use super::registry::{Manifest, Tool, script_name};
use crate::Result;
use serde_json::{Value, json};
use std::sync::LazyLock;

static ORIGINAL: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!(
        "../../fixtures/tool-results/v4-tool-views.json"
    ))
    .expect("fixed original model tool views")
});
pub(super) fn direct(name: &str) -> bool {
    ORIGINAL["direct_names"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v == name)
}
pub(super) fn declaration(name: &str) -> Option<Tool> {
    let d = ORIGINAL["declarations"]
        .as_array()?
        .iter()
        .find(|d| d["name"] == name)?;
    Some(Tool {
        name: name.into(),
        description: d["description"].as_str()?.into(),
        schema: d["parameters"].clone(),
        parallel_read: name != "update_short_state",
        cacheable: name == "lookup_tools",
        code_direct: direct(name),
        provider: None,
    })
}
fn invalid() -> Value {
    json!({"ok":false,"executed":false,"error":"invalid_lookup_arguments"})
}
impl Manifest {
    /// The full original discovery body is nested in the ordinary core effect receipt.
    pub fn model_lookup(&self, args: &Value, code: bool) -> Result<Value> {
        let Some(a) = args.as_object() else {
            return Ok(invalid());
        };
        if a.keys()
            .any(|k| !["query", "name", "offset", "limit"].contains(&k.as_str()))
        {
            return Ok(invalid());
        }
        let query = a.get("query").unwrap_or(&Value::Null);
        let query = if a.contains_key("query") {
            let Some(s) = query.as_str().filter(|s| s.chars().count() <= 128) else {
                return Ok(invalid());
            };
            s
        } else {
            ""
        };
        let offset = match a.get("offset") {
            None => 0,
            Some(v) => match v.as_u64().and_then(|n| usize::try_from(n).ok()) {
                Some(n) => n,
                None => return Ok(invalid()),
            },
        };
        let limit = match a.get("limit") {
            None => 5,
            Some(v) => match v.as_u64().filter(|n| (1..=10).contains(n)) {
                Some(n) => n as usize,
                None => return Ok(invalid()),
            },
        };
        let visible = |t: &Tool| {
            if code {
                t.code_direct
            } else {
                !["lookup_tools", "execute_code"].contains(&t.name.as_str())
            }
        };
        let data = if let Some(raw) = a.get("name") {
            let Some(name) = raw
                .as_str()
                .filter(|s| (1..=256).contains(&s.chars().count()))
            else {
                return Ok(invalid());
            };
            if a.len() != 1 {
                return Ok(invalid());
            }
            let Some(t) = self
                .0
                .get(name)
                .filter(|_| !["lookup_tools", "execute_code"].contains(&name))
            else {
                return Ok(json!({"ok":false,"executed":false,"error":"unknown_capability"}));
            };
            json!({"name":name,"description":t.description,"parameters":t.schema,"script_name":script_name(name),"direct":visible(t)})
        } else {
            let folded = crate::history::normalize::casefold(query.chars());
            // Python str.split also treats the four ASCII information separators as whitespace.
            let words: Vec<_> = folded
                .split(|c: char| c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c))
                .filter(|s| !s.is_empty())
                .collect();
            let hits: Vec<_> = self
                .0
                .values()
                .filter(|t| !["lookup_tools", "execute_code"].contains(&t.name.as_str()))
                .filter(|t| {
                    let haystack = crate::history::normalize::casefold(
                        format!("{} {}", t.name, t.description).chars(),
                    );
                    words.iter().all(|w| haystack.contains(w))
                })
                .collect();
            let selected:Vec<_> = hits.iter().skip(offset).take(limit).map(|t|json!({"name":t.name,"description":t.description.chars().take(240).collect::<String>(),"direct":visible(t)})).collect();
            json!({"tools":selected,"total":hits.len(),"next_offset":offset.checked_add(selected.len()).filter(|n|*n<hits.len())})
        };
        Ok(
            json!({"ok":true,"data":data,"manifest_revision":self.revision()?,"authorization":"Execution still checks current source, permissions and budget."}),
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_discovery_golden_and_fixed_core_projection() {
        let m = Manifest::freeze(
            ORIGINAL["fixture_tools"]
                .as_array()
                .unwrap()
                .iter()
                .map(|d| {
                    let name = d["name"].as_str().unwrap();
                    Tool {
                        name: name.into(),
                        description: d["description"].as_str().unwrap().into(),
                        schema: d["parameters"].clone(),
                        parallel_read: false,
                        cacheable: false,
                        code_direct: direct(name),
                        provider: None,
                    }
                }),
        )
        .unwrap();
        for case in ORIGINAL["cases"].as_array().unwrap() {
            let mut expected = case["result"].clone();
            if expected["ok"] == true {
                expected["manifest_revision"] = json!(m.revision().unwrap());
            }
            assert_eq!(
                m.model_lookup(&case["args"], case["code"].as_bool().unwrap())
                    .unwrap(),
                expected,
                "{:?}",
                case["args"]
            );
        }
        assert!(
            !m.declarations(false)
                .iter()
                .any(|t| t["function"]["name"] == "execute_code")
        );
        assert!(
            m.declarations(true)
                .iter()
                .any(|t| t["function"]["name"] == "execute_code")
        );
        let core = Manifest::core();
        for t in core.0.values() {
            assert_eq!(t.code_direct, direct(&t.name), "{}", t.name);
        }
        for d in ORIGINAL["declarations"].as_array().unwrap() {
            assert_eq!(core.0[d["name"].as_str().unwrap()].wire()["function"], *d);
        }
        assert!(!core.0.contains_key("terminal_exec"));
        assert!(
            core.declarations(true)
                .iter()
                .any(|t| t["function"]["name"] == "get_group_members")
        );
        assert!(
            !core
                .declarations(true)
                .iter()
                .any(|t| t["function"]["name"] == "recall_own_message")
        );
    }
}
