use crate::{
    Result,
    context::{
        compact_public::wire,
        protocol_objects::{self as objects, Round},
    },
    runtime::main::Config,
};
use serde_json::Value;

pub(super) struct Plan {
    pub base: Vec<Value>,
    pub original: Vec<Value>,
    pub retained: Vec<Value>,
    pub request: Value,
    pub covered: usize,
}
struct Atom {
    end: usize,
    retained: Vec<Value>,
    cost: wire::Count,
    protected: bool,
}
pub(super) struct Sources<'a> {
    pub rounds: &'a [Round],
    pub public: &'a [Value],
}
/// Cost each complete source round and its portable replacement once. A tool
/// batch contributes exactly one boundary even when a provider flattens its blocks.
pub(super) fn select(
    cfg: &Config,
    body: &Value,
    first: &Value,
    sources: Sources,
    previous: Option<&Value>,
    instruction: &str,
    limit: Option<u64>,
) -> Result<Option<Plan>> {
    let history = sources.rounds;
    let p = cfg.protocol;
    let original = objects::parts(p, body)?;
    let initial = objects::parts(p, first)?;
    let marker = previous.and_then(|m| original.iter().position(|part| part == m));
    let base_count = if let Some(i) = marker {
        i
    } else if original.starts_with(&initial) {
        initial.len()
    } else {
        // An accepted public checkpoint can shorten the initial prefix. Its final
        // original incoming item must still be immediately before the first paid turn.
        let Some(last) = initial.last() else {
            return Ok(None);
        };
        let candidates: Vec<_> = original
            .windows(2)
            .enumerate()
            .filter(|(i, pair)| {
                pair[0] == *last
                    && history
                        .iter()
                        .any(|r| !r.parts.is_empty() && original[i + 1..].starts_with(&r.parts))
            })
            .collect();
        if candidates.len() != 1 {
            return Ok(None);
        }
        candidates[0].0 + 1
    };
    let minimum = marker.map_or(base_count, |i| i + 1);
    let mut atoms = Vec::new();
    let mut i = minimum;
    let mut next_round = 0;
    let last = history.last().ok_or("no original paid normal response")?;
    while i < original.len() {
        let (n, retained, protected) = if let Some((j, r)) = history
            .iter()
            .enumerate()
            .skip(next_round)
            .find(|(_, r)| !r.parts.is_empty() && original[i..].starts_with(&r.parts))
        {
            next_round = j + 1;
            let keep = if p.is_chat() {
                r.parts.clone()
            } else {
                r.portable.clone()
            };
            (r.parts.len(), keep, r.calls && r.logical == last.logical)
        } else {
            let v = &original[i];
            if v["role"] != "user" {
                return Ok(None);
            }
            // On a retired chain, only an exact database-backed portable round
            // can protect the original effect. User-authored JSON grants nothing.
            let keep = last.calls && last.portable.iter().any(|x| x == v) && marker.is_some();
            (1, vec![v.clone()], keep)
        };
        let cost = retained
            .iter()
            .fold(wire::Count::default(), |c, v| c.join(wire::part_cost(p, v)));
        i += n;
        atoms.push(Atom {
            end: i,
            retained,
            cost,
            protected,
        });
    }
    if atoms.is_empty() {
        return Ok(None);
    }
    let keep = atoms
        .iter()
        .position(|a| a.protected)
        .unwrap_or(atoms.len());
    // With only the old checkpoint and its required original tool batch, there is
    // no new execution prefix. Re-running this candidate cannot free capacity.
    if marker.is_some() && keep == 0 {
        return Ok(None);
    }
    let base = original[..base_count].to_vec();
    let mut prefix = wire::start(cfg, body);
    let mut counts = Vec::with_capacity(original.len() + 1);
    counts.push(prefix);
    for v in &original {
        prefix = prefix.join(wire::part_cost(p, v));
        counts.push(prefix);
    }
    let ins = objects::text(p, "user", instruction);
    let ins_cost = wire::part_cost(p, &ins);
    let required = atoms
        .iter()
        .skip(keep)
        .fold(wire::Count::default(), |c, a| c.join(a.cost));
    let fits = |c: wire::Count| limit.is_none_or(|l| c.tokens() <= l);
    if !fits(counts[base_count].join(required)) {
        return Ok(None);
    }
    for index in (0..atoms.len()).rev() {
        let end = atoms[index].end;
        if !fits(counts[end].join(ins_cost)) {
            continue;
        }
        let retain_from = (index + 1).min(keep);
        let suffix: Vec<Value> = atoms[retain_from..]
            .iter()
            .flat_map(|a| a.retained.iter().cloned())
            .collect();
        let extra: Vec<_> = sources
            .public
            .iter()
            .filter(|v| !base.contains(v) && !suffix.contains(v) && original.contains(v))
            .cloned()
            .collect();
        let public_cost = extra
            .iter()
            .fold(wire::Count::default(), |c, v| c.join(wire::part_cost(p, v)));
        if !fits(counts[base_count].join(required).join(public_cost)) {
            continue;
        }
        let mut retained = extra;
        retained.extend(suffix);
        let mut request = objects::body_with(p, body, &original[..end])?;
        p.user(&mut request, instruction)?;
        if p.is_messages() {
            crate::model::messages::cache_last(&mut request)?;
        }
        return Ok(Some(Plan {
            base,
            original,
            retained,
            request,
            covered: end,
        }));
    }
    Ok(None)
}
