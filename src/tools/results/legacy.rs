//! Original v4 artifact operations. All offsets refer to immutable input characters/items.
use super::ordered::Node;
use crate::Result;
use serde_json::{Value, json};

pub(super) fn fold(s: &str) -> String {
    crate::history::normalize::casefold(s.chars())
}
fn merged(base: &Value, fields: Value) -> Value {
    let mut out = base.clone();
    out.as_object_mut()
        .unwrap()
        .extend(fields.as_object().unwrap().clone());
    out
}
fn error(code: &str, detail: &str, metadata: Value) -> Value {
    merged(&metadata, json!({"error_code":code,"detail":detail}))
}
// Python's original ensure_ascii=False JSON uses a space after commas and colons.
fn json_size(v: &Value) -> usize {
    match v {
        Value::Array(a) => {
            2 + a.iter().map(json_size).sum::<usize>() + a.len().saturating_sub(1) * 2
        }
        Value::Object(o) => {
            2 + o
                .iter()
                .map(|(k, v)| json_size(&json!(k)) + 2 + json_size(v))
                .sum::<usize>()
                + o.len().saturating_sub(1) * 2
        }
        _ => v.to_string().chars().count(),
    }
}
fn page_size(v: &Value) -> usize {
    let envelope = if v["error_code"].is_string() {
        let mut data = v.clone();
        let map = data.as_object_mut().unwrap();
        map.remove("error_code");
        map.remove("detail");
        let mut out = json!({"ok":false,"retryable":false,"provider_id":"artifacts","tool_name":"read_tool_artifact","error_code":v["error_code"],"public_message":v["detail"]});
        if !map.is_empty() {
            out["data"] = data;
        }
        out
    } else {
        json!({"ok":true,"data":v,"retryable":false,"mutation_committed":false,"provider_id":"artifacts","tool_name":"read_tool_artifact"})
    };
    json_size(&envelope)
}
fn oversized(node: &Node, path: &Value, base: &Value, budget: usize) -> Value {
    let mut out = error(
        "artifact_value_too_large",
        "目标值无法完整放入当前工具结果预算，请读取更深层路径",
        merged(
            base,
            json!({"path":path,"value_shape":node.shape(),"estimated_characters":0}),
        ),
    );
    if page_size(&out) <= budget {
        out["estimated_characters"] = json!(node.size(usize::MAX));
    }
    if page_size(&out) > budget {
        out.as_object_mut().unwrap().remove("estimated_characters");
    }
    out
}
struct Selection<'a> {
    path: &'a Value,
    offset: usize,
    limit: usize,
    base: &'a Value,
    budget: usize,
}
fn string_page(
    content: &str,
    total: usize,
    offset: usize,
    base: &Value,
    budget: usize,
    matched: Option<bool>,
) -> Value {
    let text = base["mode"] == "text";
    let make = |part: &str| {
        let end = offset.saturating_add(part.chars().count());
        let mut out = merged(
            base,
            json!({"offset":offset,"next_offset":(end<total).then_some(end),"total_characters":total}),
        );
        out[if text { "content" } else { "value" }] = json!(part);
        if text {
            out["query_matched"] = json!(matched);
        } else {
            out["offset_unit"] = json!("characters");
            out["truncated"] = json!(end < total);
        }
        out
    };
    // Escaped characters are monotone except the EOF cursor, which can shrink.
    let reference = make("");
    let mut current = String::new();
    let mut best = String::new();
    let terminal = offset.saturating_add(content.chars().count()) == total;
    let mut used = 0;
    let mut reference_null = reference.clone();
    reference_null["next_offset"] = Value::Null;
    if !text {
        reference_null["truncated"] = json!(false);
    }
    let fixed = page_size(&reference_null) - 4 - if text { 0 } else { 5 };
    for (index, c) in content.chars().enumerate() {
        current.push(c);
        used += json!(c.to_string()).to_string().chars().count() - 2;
        let end = offset + index + 1;
        let metadata = if end < total {
            end.to_string().len() + if text { 0 } else { 4 }
        } else {
            4 + if text { 0 } else { 5 }
        };
        if fixed + used + metadata <= budget {
            best.clone_from(&current);
        }
        let integer = end.to_string().len() + if text { 0 } else { 4 };
        let floor = if terminal {
            integer.min(4 + if text { 0 } else { 5 })
        } else {
            integer
        };
        if fixed + used + floor > budget {
            break;
        }
    }
    let out = make(&best);
    if page_size(&out) > budget || (best.is_empty() && offset < total) {
        let mut metadata = if text {
            json!({"handle":base["handle"]})
        } else {
            base.clone()
        };
        metadata.as_object_mut().unwrap().remove("type");
        return error(
            "artifact_budget_too_small",
            "预算不足以返回一个字符",
            merged(&metadata, json!({"offset":offset,"next_offset":offset})),
        );
    }
    out
}
fn inspect(node: &Node, s: &Selection<'_>) -> Value {
    let mut out = merged(s.base, json!({"path":s.path,"type":node.kind()}));
    let children: Vec<Value> = match node {
        Node::Object(o) => {
            out["total_children"] = json!(o.len());
            node.keys()
                .into_iter()
                .skip(s.offset)
                .take(s.limit)
                .map(|(k, v)| merged(&v.shape(), json!({"key":k})))
                .collect()
        }
        Node::Array(a) => {
            out["length"] = json!(a.len());
            a.iter()
                .enumerate()
                .skip(s.offset)
                .take(s.limit)
                .map(|(i, v)| merged(&v.shape(), json!({"index":i})))
                .collect()
        }
        Node::Scalar(Value::String(text)) => {
            out["characters"] = json!(text.chars().count());
            return out;
        }
        Node::Scalar(v) => {
            out["value"] = v.clone();
            return out;
        }
    };
    let total = out["total_children"]
        .as_u64()
        .or_else(|| out["length"].as_u64())
        .unwrap() as usize;
    out["children"] = json!([]);
    out["next_offset"] = Value::Null;
    if s.offset >= total {
        return out;
    }
    let count = prefix(
        children.iter().map(json_size),
        &out,
        s.offset,
        total,
        s.limit,
        s.budget,
    );
    if count == 0 {
        return error(
            "artifact_budget_too_small",
            "无法放入一个子项，请增大预算",
            merged(
                s.base,
                json!({"path":s.path,"offset":s.offset,"next_offset":s.offset}),
            ),
        );
    }
    out["children"] = json!(&children[..count]);
    out["next_offset"] = json!((s.offset + count < total).then_some(s.offset + count));
    out
}
fn prefix(
    sizes: impl Iterator<Item = usize>,
    empty: &Value,
    offset: usize,
    total: usize,
    limit: usize,
    budget: usize,
) -> usize {
    let fixed = page_size(empty) - 4;
    let mut used = 0;
    let mut best = 0;
    let terminal = offset.saturating_add(limit.min(total.saturating_sub(offset))) == total;
    for (index, size) in sizes.take(limit).enumerate() {
        used += size + if index > 0 { 2 } else { 0 };
        let end = offset + index + 1;
        if fixed
            + used
            + if end < total {
                end.to_string().len()
            } else {
                4
            }
            <= budget
        {
            best = index + 1;
        }
        let floor = if terminal {
            end.to_string().len().min(4)
        } else {
            end.to_string().len()
        };
        if fixed + used + floor > budget {
            break;
        }
    }
    best
}
fn get(node: &Node, s: &Selection<'_>) -> Value {
    if let Node::Scalar(Value::String(text)) = node {
        return string_page(
            &text
                .chars()
                .skip(s.offset)
                .take(s.limit.min(s.budget))
                .collect::<String>(),
            text.chars().count(),
            s.offset,
            &merged(s.base, json!({"path":s.path,"type":"string"})),
            s.budget,
            None,
        );
    }
    let total = match node {
        Node::Object(o) => o.len(),
        Node::Array(a) => a.len(),
        _ => 0,
    };
    if s.offset == 0 && total <= s.limit {
        let mut direct = merged(
            s.base,
            json!({"path":s.path,"type":node.kind(),"value":null}),
        );
        let fixed = page_size(&direct) - 4;
        if fixed <= s.budget && node.size(s.budget - fixed).is_some() {
            direct["value"] = node.value();
            return direct;
        }
    }
    if matches!(node, Node::Scalar(_)) {
        return oversized(node, s.path, s.base, s.budget);
    }
    let object = matches!(node, Node::Object(_));
    let mut out = merged(
        s.base,
        json!({"path":s.path,"type":node.kind(),"total_items":total,"offset":s.offset,"value":if object{json!({})}else{json!([])},"next_offset":null}),
    );
    if s.offset >= total {
        return out;
    }
    let items: Vec<_> = if object {
        node.keys()
            .into_iter()
            .skip(s.offset)
            .take(s.limit)
            .map(|(k, v)| (Some(k), v))
            .collect()
    } else if let Node::Array(a) = node {
        a.iter()
            .skip(s.offset)
            .take(s.limit)
            .map(|v| (None, v))
            .collect()
    } else {
        unreachable!()
    };
    let sizes = items.iter().map(|(k, v)| {
        v.size(s.budget).unwrap_or(s.budget.saturating_add(1))
            + k.map_or(0, |k| json_size(&json!(k)) + 2)
    });
    let count = prefix(sizes, &out, s.offset, total, s.limit, s.budget);
    if count == 0 {
        return oversized(node, s.path, s.base, s.budget);
    }
    out["value"] = if object {
        Value::Object(
            items[..count]
                .iter()
                .map(|(k, v)| (k.unwrap().into(), v.value()))
                .collect(),
        )
    } else {
        Value::Array(items[..count].iter().map(|(_, v)| v.value()).collect())
    };
    out["next_offset"] = json!((s.offset + count < total).then_some(s.offset + count));
    out
}
struct Hit<'a> {
    path: Vec<Value>,
    matched: Vec<Value>,
    node: &'a Node,
}
fn search(node: &Node, s: &Selection<'_>, query: &str) -> Value {
    fn walk<'a>(
        node: &'a Node,
        path: &mut Vec<Value>,
        key_match: bool,
        query: &str,
        hits: &mut Vec<Hit<'a>>,
        scanned: &mut usize,
    ) {
        *scanned += 1;
        let mut recorded = key_match;
        if key_match {
            hits.push(Hit {
                path: path.clone(),
                matched: path.clone(),
                node,
            });
        }
        match node {
            Node::Object(_) => {
                for (key, child) in node.keys() {
                    let matched = fold(key).contains(query);
                    if matches!(child, Node::Scalar(_)) {
                        if !recorded && (matched || fold(&child.scalar_text()).contains(query)) {
                            let mut at = path.clone();
                            at.push(json!(key));
                            hits.push(Hit {
                                path: path.clone(),
                                matched: at,
                                node,
                            });
                            recorded = true;
                        }
                    } else {
                        path.push(json!(key));
                        walk(child, path, matched, query, hits, scanned);
                        path.pop();
                    }
                }
            }
            Node::Array(a) => {
                for (index, child) in a.iter().enumerate() {
                    path.push(json!(index));
                    if matches!(child, Node::Scalar(_)) {
                        if fold(&child.scalar_text()).contains(query) {
                            hits.push(Hit {
                                path: path.clone(),
                                matched: path.clone(),
                                node: child,
                            });
                        }
                    } else {
                        walk(child, path, false, query, hits, scanned);
                    }
                    path.pop();
                }
            }
            _ => {
                if !recorded && fold(&node.scalar_text()).contains(query) {
                    hits.push(Hit {
                        path: path.clone(),
                        matched: path.clone(),
                        node,
                    });
                }
            }
        }
    }
    let mut hits = vec![];
    let mut scanned = 0;
    walk(
        node,
        &mut s.path.as_array().unwrap().clone(),
        false,
        &fold(query),
        &mut hits,
        &mut scanned,
    );
    hits.sort_by_cached_key(|hit| {
        hit.path
            .iter()
            .map(|v| {
                if let Some(k) = v.as_str() {
                    format!("str:{k}")
                } else {
                    format!("int:{v}")
                }
            })
            .collect::<Vec<_>>()
    });
    let mut out = merged(
        s.base,
        json!({"query":query,"matches":[],"next_offset":null,"scanned_nodes":scanned}),
    );
    let mut candidates = vec![];
    let sizes = hits.iter().skip(s.offset).take(s.limit).map(|hit| {
        let mut item = json!({"path":hit.path,"matched_path":hit.matched,"value":null});
        let fixed = page_size(&item) - 4;
        if fixed > s.budget || hit.node.size(s.budget.saturating_sub(fixed)).is_none() {
            item.as_object_mut().unwrap().remove("value");
            item["value_omitted"] = json!(true);
            item["value_shape"] = hit.node.shape();
            item["instruction"] = json!("匹配对象过大，请对 path 执行 inspect 或 get");
        } else {
            item["value"] = hit.node.value();
        }
        let size = json_size(&item);
        candidates.push(item);
        size
    });
    let count = prefix(sizes, &out, s.offset, hits.len(), s.limit, s.budget);
    if !candidates.is_empty() && count == 0 {
        return error(
            "artifact_budget_too_small",
            "无法放入一个匹配，请增大预算或读取更深path",
            merged(
                s.base,
                json!({"path":s.path,"offset":s.offset,"next_offset":s.offset}),
            ),
        );
    }
    out["matches"] = json!(&candidates[..count]);
    out["next_offset"] = json!((s.offset + count < hits.len()).then_some(s.offset + count));
    out
}
fn text(
    content: &str,
    handle: &Value,
    offset: usize,
    limit: usize,
    query: &str,
    budget: usize,
) -> Value {
    let mut start = offset;
    if !query.is_empty() {
        let suffix: String = content.chars().skip(offset).collect();
        let folded = fold(&suffix);
        let Some(byte) = folded.find(&fold(query)) else {
            return json!({"handle":handle,"mode":"text","offset":offset,"next_offset":null,"total_characters":content.chars().count(),"content":"","query_matched":false});
        };
        let found = folded[..byte].chars().count();
        let mut units = 0;
        for (index, c) in suffix.chars().enumerate() {
            units += fold(&c.to_string()).chars().count();
            if units > found {
                start = offset + index;
                break;
            }
        }
    }
    string_page(
        &content
            .chars()
            .skip(start)
            .take(limit.min(budget))
            .collect::<String>(),
        content.chars().count(),
        start,
        &json!({"handle":handle,"mode":"text"}),
        budget,
        (!query.is_empty()).then_some(true),
    )
}
pub(super) fn page(raw: &str, args: &Value, budget: usize) -> Result<Value> {
    let operation = args["operation"].as_str().unwrap_or("get");
    let offset = args["offset"].as_u64().unwrap_or(0) as usize;
    let limit = (args["limit"].as_u64().unwrap_or(budget as u64) as usize).max(1);
    let query = args["query"].as_str().unwrap_or("");
    if operation == "image" {
        return Ok(error(
            "artifact_not_image",
            "该 Artifact 没有原生图片结果",
            json!({}),
        ));
    }
    if operation == "text" {
        return Ok(text(raw, &args["handle"], offset, limit, query, budget));
    }
    let root: Node = serde_json::from_str(raw)?;
    let (mut node, logical) = if root.get("data").is_some()
        && ["provider_id", "tool_name", "ok"]
            .iter()
            .any(|k| root.get(k).is_some())
    {
        (root.get("data").unwrap(), "data")
    } else if root.get("result").is_some() && root.get("state").is_some() {
        (root.get("result").unwrap(), "result")
    } else {
        (&root, "$")
    };
    let path = args.get("path").cloned().unwrap_or(json!([]));
    let parts = path.as_array().ok_or("artifact path requires array")?;
    let mut traversed = vec![];
    for part in parts {
        let (next, detail) = match node {
            Node::Object(_) => (
                part.as_str().and_then(|k| node.get(k)),
                "Artifact 对象路径不存在",
            ),
            Node::Array(a) => (
                part.as_u64().and_then(|n| a.get(n as usize)),
                "Artifact 数组下标不存在",
            ),
            _ => (None, "Artifact 路径穿过了标量值"),
        };
        let Some(next) = next else {
            return Ok(error(
                "artifact_path_not_found",
                detail,
                json!({"path":traversed,"failed_part":part}),
            ));
        };
        node = next;
        traversed.push(part.clone());
    }
    let base = json!({"handle":args["handle"],"mode":"json","logical_root":logical,"provider_id":root.get("provider_id").map(Node::value).unwrap_or(json!("core")),"tool_name":root.get("tool_name").map(Node::value).unwrap_or_else(||args.get("_tool_name").cloned().unwrap_or(json!("")))});
    let selection = Selection {
        path: &path,
        offset,
        limit,
        base: &base,
        budget,
    };
    Ok(match operation {
        "get" => get(node, &selection),
        "inspect" => inspect(node, &selection),
        "search" if !query.is_empty() => search(node, &selection, query),
        "search" => error(
            "artifact_query_required",
            "search 操作必须提供 query",
            json!({}),
        ),
        _ => error(
            "artifact_operation_invalid",
            "Artifact operation 必须是 inspect、get、search、text 或 image",
            json!({}),
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn literal_original_pages() {
        let fixtures: Value = serde_json::from_str(include_str!(
            "../../../fixtures/tool-results/v4-original-pages.json"
        ))
        .unwrap();
        for row in fixtures["cases"].as_array().unwrap() {
            let node: Node = serde_json::from_str(row["raw_value"].as_str().unwrap()).unwrap();
            let selection = Selection {
                path: &row["path"],
                offset: row["offset"].as_u64().unwrap() as usize,
                limit: row["limit"].as_u64().unwrap() as usize,
                base: &row["base"],
                budget: row["budget"].as_u64().unwrap() as usize,
            };
            let actual = match row["operation"].as_str().unwrap() {
                "get" => get(&node, &selection),
                "inspect" => inspect(&node, &selection),
                "search" => search(&node, &selection, row["query"].as_str().unwrap()),
                "text" => text(
                    row["value"].as_str().unwrap(),
                    &json!("h"),
                    selection.offset,
                    selection.limit,
                    row["query"].as_str().unwrap(),
                    selection.budget,
                ),
                _ => unreachable!(),
            };
            assert_eq!(actual, row["result"], "{}", row["name"]);
        }
    }
}
