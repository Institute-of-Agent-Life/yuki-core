use super::opencc;
use crate::Result;
use serde::Serialize;
use std::{
    collections::{BTreeSet, HashMap},
    fmt::Write,
    sync::LazyLock,
};
use unicode_normalization::UnicodeNormalization;

static CASEFOLD: LazyLock<HashMap<char, String>> = LazyLock::new(|| {
    let scalar =
        |s: &str| char::from_u32(u32::from_str_radix(s, 16).expect("Unicode scalar")).unwrap();
    include_str!("casefold.txt")
        .lines()
        .map(|line| {
            let (from, to) = line.split_once(' ').unwrap();
            (scalar(from), to.split(',').map(scalar).collect())
        })
        .collect()
});
pub fn fold(raw: &str) -> String {
    let mut case = String::new();
    for c in raw.nfkc() {
        if let Some(replacement) = CASEFOLD.get(&c) {
            case.push_str(replacement);
        } else {
            case.push(c);
        }
    }
    case.nfkc()
        .collect::<String>()
        .split(|c: char| c.is_whitespace() || matches!(c, '\u{1c}'..='\u{1f}'))
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}
#[derive(Serialize)]
pub struct Field {
    pub label: String,
    pub raw: String,
    pub base: String,
    pub simple: String,
}
pub fn field(label: String, raw: String) -> Result<Field> {
    let base = fold(&raw);
    let simple = opencc::t2s(&base)?;
    Ok(Field {
        label,
        raw,
        base,
        simple,
    })
}
fn gram(chars: &[char]) -> String {
    let mut gram = format!("g{}x", chars.len());
    for c in chars {
        for b in c.encode_utf8(&mut [0; 4]).bytes() {
            write!(gram, "{b:02x}").unwrap();
        }
    }
    gram
}
pub fn grams(fields: &[Field]) -> BTreeSet<String> {
    let mut grams = BTreeSet::new();
    for field in fields {
        for text in [&field.base, &field.simple] {
            let chars: Vec<_> = text.chars().collect();
            for width in 1..=3 {
                for g in chars.windows(width) {
                    grams.insert(gram(g));
                }
            }
        }
    }
    grams
}
pub fn candidates(text: &str) -> BTreeSet<String> {
    let chars: Vec<_> = text.chars().collect();
    let width = chars.len().min(3);
    if width == 0 {
        BTreeSet::new()
    } else {
        chars.windows(width).map(gram).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matches_pinned_python_folding_opencc_and_all_grams() {
        let data: serde_json::Value =
            serde_json::from_str(include_str!("../../fixtures/history/v4-literals.json")).unwrap();
        for c in data["cases"].as_array().unwrap() {
            let f = field("content".into(), c["text"].as_str().unwrap().into()).unwrap();
            assert_eq!(f.base, c["base"]);
            assert_eq!(f.simple, c["simplified"]);
            assert_eq!(serde_json::json!(grams(&[f])), c["terms"]);
        }
    }
}
