use crate::model::wire::Protocol;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
fn public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_multicast()
        || a == 0
        || a >= 240
        || (a == 100 && (64..=127).contains(&b))
        || (a == 198 && matches!(b, 18 | 19))
        || (a == 192 && b == 0 && c == 0))
}
fn public_v6(ip: Ipv6Addr) -> bool {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return public_v4(v4);
    }
    let s = ip.segments();
    (s[0] & 0xe000) == 0x2000 && !(s[0] == 0x2001 && (s[1] == 0xdb8 || s[1] <= 0x01ff))
}
fn public(raw: &str) -> Option<(String, String)> {
    let mut url = reqwest::Url::parse(raw).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return None;
    }
    let host = url
        .host_str()?
        .trim_matches(['[', ']'])
        .trim_end_matches('.')
        .to_lowercase();
    if let Ok(ip) = host.parse::<IpAddr>() {
        if !match ip {
            IpAddr::V4(a) => public_v4(a),
            IpAddr::V6(a) => public_v6(a),
        } {
            return None;
        }
    } else if !host.contains('.')
        || host.ends_with(".localhost")
        || host.ends_with(".local")
        || host.ends_with(".internal")
    {
        return None;
    }
    url.set_fragment(None);
    Some((url.to_string(), host))
}
struct Proof {
    sources: Vec<Value>,
    seen: BTreeSet<String>,
    limit: usize,
    partial: bool,
    events: u64,
}
impl Proof {
    fn add(&mut self, node: &Value, href: &str) {
        if let Some((url, domain)) = node[href].as_str().and_then(public)
            && self.sources.len() < self.limit
            && self.seen.insert(url.clone())
        {
            let id = format!("{:x}", Sha256::digest(url.as_bytes()));
            self.sources.push(json!({"source_id":format!("native-{}",&id[..16]),"url":url,"domain":domain,"title":node["title"].as_str().unwrap_or(""),"snippet":""}));
        }
    }
    fn responses(&mut self, v: &Value) {
        self.partial |= v["status"] != "completed";
        for i in v["output"].as_array().into_iter().flatten() {
            if i["type"] == "web_search_call" {
                self.events += 1;
                if i["status"] != "completed" {
                    self.partial = true;
                    continue;
                }
                for source in i["action"]["sources"].as_array().into_iter().flatten() {
                    self.add(source, "url")
                }
                if i["action"]["type"] == "open_page" {
                    self.add(&i["action"], "url")
                }
            } else if i["type"] == "message" {
                for part in i["content"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|p| p["type"] == "output_text")
                {
                    for a in part["annotations"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter(|p| p["type"] == "url_citation")
                    {
                        self.add(a, "url")
                    }
                }
            } else if i["type"] == "function_call" {
                self.partial = true
            }
        }
    }
    fn messages(&mut self, v: &Value) {
        self.partial |= v["stop_reason"] != "end_turn";
        let mut active = BTreeSet::new();
        for block in v["content"].as_array().into_iter().flatten() {
            match block["type"].as_str() {
                Some("server_tool_use") if block["name"] == "web_search" => {
                    if let Some(id) = block["id"].as_str() {
                        if !active.insert(id) {
                            self.partial = true
                        }
                        self.events += 1;
                    }
                }
                Some("web_search_tool_result") => {
                    if block["tool_use_id"]
                        .as_str()
                        .is_none_or(|id| !active.remove(id))
                    {
                        self.partial = true;
                        continue;
                    }
                    if !block["content"].is_array() {
                        self.partial = true;
                        continue;
                    }
                    for s in block["content"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter(|s| s["type"] == "web_search_result")
                    {
                        self.add(s, "url")
                    }
                }
                // Text citations cannot vouch for an unattached server result. The
                // matched server_tool_use/result pair above owns the public source.
                Some("tool_use") => self.partial = true,
                _ => {}
            }
        }
        self.partial |= !active.is_empty();
    }
    fn gemini(&mut self, v: &Value) {
        let candidate = &v["candidates"][0];
        self.partial |= candidate["finishReason"] != "STOP";
        let metadata = &candidate["groundingMetadata"];
        if metadata.is_object() {
            self.events += 1
        }
        for part in metadata["groundingChunks"].as_array().into_iter().flatten() {
            self.add(&part["web"], "uri")
        }
    }
}
pub fn collect(p: Protocol, raw: &Value, request: &super::Query) -> Value {
    let mut proof = Proof {
        sources: vec![],
        seen: BTreeSet::new(),
        limit: request.max_results,
        partial: request.start_date.is_some()
            || request.end_date.is_some()
            || request.time_range.is_some(),
        events: 0,
    };
    if p.is_messages() {
        proof.messages(raw)
    } else if p.is_gemini() {
        proof.gemini(raw)
    } else {
        proof.responses(raw)
    }
    let no_sources = proof.sources.is_empty();
    json!({"provider":"native_search_bridge","query":request.query,"status":if no_sources{"no_search_evidence"}else if proof.partial{"partial"}else{"complete"},"partial_failure":proof.partial||no_sources,"sources":proof.sources,"native_events":proof.events})
}
