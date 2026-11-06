use crate::Result;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use unicode_normalization::UnicodeNormalization;

pub fn normalize(s: &str) -> String {
    crate::history::normalize::casefold(s.nfkc())
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
pub fn terms(text: &str, limit: usize) -> (Vec<String>, Vec<String>) {
    let (mut long, mut short) = (Vec::new(), Vec::new());
    for word in text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
    {
        let n = word.chars().count();
        if n < 3 {
            if !short.iter().any(|w| w == word) {
                short.push(word.to_string());
            }
        } else if n > 3 && word.chars().any(|c| ('\u{3400}'..='\u{9fff}').contains(&c)) {
            let chars: Vec<_> = word.chars().collect();
            for gram in chars.windows(3) {
                let g = gram.iter().collect::<String>();
                if !long.contains(&g) {
                    long.push(g)
                }
                if long.len() + short.len() >= limit {
                    break;
                }
            }
        } else if !long.iter().any(|w| w == word) {
            long.push(word.to_owned())
        }
        if long.len() + short.len() >= limit {
            break;
        }
    }
    if short.is_empty() && !text.is_empty() && text.chars().count() < 3 {
        short.push(text.to_owned());
    }
    (long, short)
}
pub fn cosine(bytes: &[u8], query: &[f32]) -> Result<f64> {
    if query.is_empty() || bytes.len() != 4 * query.len() {
        return Err("vector dimension mismatch".into());
    }
    let (mut dot, mut a, mut b) = (0.0, 0.0, 0.0);
    for (word, y) in bytes.chunks_exact(4).zip(query) {
        let x = f64::from(f32::from_le_bytes(word.try_into()?));
        let y = f64::from(*y);
        dot += x * y;
        a += x * x;
        b += y * y;
    }
    let n = (a * b).sqrt();
    if !n.is_finite() || n == 0.0 || !dot.is_finite() {
        return Err("invalid stored vector".into());
    }
    Ok((dot / n).clamp(-1., 1.))
}
pub struct Hit {
    pub number: i64,
    pub revision: i64,
    pub id: String,
    pub text: String,
    pub conflict: String,
    pub importance: i64,
    pub key: String,
    pub category: String,
    pub lexical: Option<f64>,
    pub semantic: Option<f64>,
}
pub fn global(hits: BTreeMap<i64, Hit>, query: &str, limit: usize) -> Vec<Value> {
    let order = |which: fn(&Hit) -> Option<f64>| -> BTreeMap<i64, usize> {
        let mut ranks: Vec<_> = hits
            .values()
            .filter_map(|h| which(h).map(|s| (h.number, s)))
            .collect();
        ranks.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        ranks
            .into_iter()
            .enumerate()
            .map(|(r, (id, _))| (id, r + 1))
            .collect()
    };
    let lex = order(|h| h.lexical);
    let sem = order(|h| h.semantic);
    let mut rows = Vec::new();
    for (id, h) in hits {
        let reason = if h.lexical.is_some() && normalize(&h.key) == query {
            "memory_key_exact"
        } else if h.lexical.is_some() && normalize(&h.text) == query {
            "content_exact"
        } else if h.lexical.is_some() && normalize(&h.category) == query {
            "category_exact"
        } else if h.lexical.is_some() && h.semantic.is_some() {
            "hybrid_match"
        } else if h.semantic.is_some() {
            "semantic_match"
        } else {
            "lexical_match"
        };
        let band = if matches!(reason, "memory_key_exact" | "content_exact") {
            21
        } else {
            (h.semantic.unwrap_or(0.).max(0.) * 20.) as i32
        };
        let fusion = [lex.get(&id), sem.get(&id)]
            .into_iter()
            .flatten()
            .map(|rank| 1. / (60 + rank) as f64)
            .sum::<f64>();
        rows.push((band,fusion,h.importance,id,json!({"fact_id":id,"revision":h.revision,"fact":h.id,"text":h.text,"conflict_state":h.conflict,"key":h.key,"lexical_score":h.lexical,"semantic_score":h.semantic,"lexical_rank":lex.get(&id),"semantic_rank":sem.get(&id),"relevance_band":band,"fusion_score":fusion,"selection_reason":reason})));
    }
    rows.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| b.1.total_cmp(&a.1))
            .then(b.2.cmp(&a.2))
            .then(a.3.cmp(&b.3))
    });
    rows.into_iter()
        .take(limit)
        .enumerate()
        .map(|(n, (_, _, _, _, mut v))| {
            v["rank"] = json!(n + 1);
            v
        })
        .collect()
}
