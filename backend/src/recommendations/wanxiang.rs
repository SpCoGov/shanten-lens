//! v2.5.0 physical-meld enumeration and first-reachable DFS, without wall draws.
use super::{deck, face, ids, norm, pool, search_limits, souzu, Entry, SwitchBatches};
use serde_json::{json, Map, Value};
use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

#[derive(Debug)]
struct Meld {
    ids: [u64; 3],
    // Wall time is always zero. Preserve the remaining legacy sort keys exactly.
    key: (usize, usize, u8, u8, [u64; 3]),
}

struct Search<'a> {
    emit: &'a mut dyn FnMut(Value),
    stopped: &'a dyn Fn() -> bool,
    started: Instant,
    last_emit: Instant,
    meld_total: usize,
    meld_index: usize,
    checks: usize,
    nodes: usize,
}

impl Search<'_> {
    fn progress(&mut self, preparing: bool, force: bool) {
        if force || self.last_emit.elapsed() >= Duration::from_millis(150) {
            (self.emit)(json!({
                "phase": if preparing {"wanxiang_preparing"} else {"wanxiang_searching"},
                "meld_total":self.meld_total,"meld_index":self.meld_index,
                "checks":self.checks,"nodes":self.nodes,"elapsed_ms":self.started.elapsed().as_millis()
            }));
            self.last_emit = Instant::now();
        }
    }

    fn enumerate(&mut self, entries: &[Entry]) -> Option<Vec<Meld>> {
        let mut groups = HashMap::<u8, Vec<usize>>::new();
        for (order, entry) in entries.iter().enumerate() {
            if entry.id != 1000 && entry.face != "bd" {
                groups
                    .entry(souzu::tile(&entry.face)?)
                    .or_default()
                    .push(order);
            }
        }
        let mut result = Vec::new();
        let mut seen = HashSet::new();
        let mut add = |orders: [usize; 3], kind, tile| -> Option<()> {
            if (self.stopped)() {
                return None;
            }
            let mut ids = orders.map(|i| entries[i].id);
            ids.sort_unstable();
            if seen.insert(ids) {
                let time = *orders.iter().max().unwrap();
                let replacement = orders
                    .iter()
                    .filter(|&&i| entries[i].source == "replacement")
                    .map(|&i| entries[i].index + 1)
                    .max()
                    .unwrap_or(0);
                result.push(Meld {
                    ids,
                    key: (time, replacement, kind, tile, ids),
                });
                self.meld_total = result.len();
            }
            self.progress(true, false);
            Some(())
        };
        for (&tile, group) in &groups {
            for a in 0..group.len() {
                for b in a + 1..group.len() {
                    for c in b + 1..group.len() {
                        add([group[a], group[b], group[c]], 0, tile)?;
                    }
                }
            }
            if tile < 27 && tile % 9 <= 6 {
                if let (Some(bs), Some(cs)) = (groups.get(&(tile + 1)), groups.get(&(tile + 2))) {
                    for &a in group {
                        for &b in bs {
                            for &c in cs {
                                add([a, b, c], 1, tile)?;
                            }
                        }
                    }
                }
            }
        }
        result.sort_unstable_by_key(|meld| meld.key);
        Some(result)
    }

    fn visit(
        &mut self,
        start: usize,
        chosen: &mut Vec<u64>,
        melds: &[Meld],
        seen: &mut HashSet<String>,
        check: &mut dyn FnMut(&[u64], String) -> Option<Value>,
    ) -> Option<Value> {
        if (self.stopped)() {
            return Some(stopped());
        }
        self.nodes += 1;
        self.progress(false, false);
        if chosen.len() == 12 {
            let mut sorted = chosen.clone();
            sorted.sort_unstable();
            let signature = format!(
                "wanxiang|{}",
                sorted
                    .iter()
                    .map(u64::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            );
            if !seen.insert(signature.clone()) {
                return None;
            }
            self.checks += 1;
            return check(chosen, signature);
        }
        let slots = 4 - chosen.len() / 3;
        if melds.len() < slots {
            return None;
        }
        for index in start..=melds.len() - slots {
            // Check even when every remaining meld overlaps and recursion is skipped.
            if (self.stopped)() {
                return Some(stopped());
            }
            if chosen.is_empty() {
                self.meld_index = index + 1;
            }
            self.progress(false, false);
            if melds[index].ids.iter().any(|id| chosen.contains(id)) {
                continue;
            }
            chosen.extend(melds[index].ids);
            let found = self.visit(index + 1, chosen, melds, seen, check);
            chosen.truncate(chosen.len() - 3);
            if found.is_some() {
                return found;
            }
        }
        None
    }
}

fn stopped() -> Value {
    json!({"status":"impossible","reason":"stopped-by-user"})
}

fn candidate_ids(mut ids: Vec<u64>, deck: &Map<String, Value>) -> Vec<u64> {
    ids.sort_unstable_by_key(|&id| {
        let raw = face(deck, id);
        (
            matches!(raw, "0m" | "0p" | "0s"),
            souzu::tile(raw).unwrap_or(99),
            id,
        )
    });
    ids
}

fn simulate(
    hand: &[u64],
    target: &[u64],
    replacement: &[u64],
    rounds: usize,
    limit: usize,
    deck: &Map<String, Value>,
) -> Option<SwitchBatches> {
    let mut current = hand.to_vec();
    let mut discards = Vec::new();
    let mut incoming = Vec::new();
    let mut cursor = 0;
    for _ in 0..rounds {
        let keep = current.iter().filter(|id| target.contains(id)).count();
        if keep == 13 {
            break;
        }
        let size = (13 - keep).min(limit);
        let batch = candidate_ids(
            current
                .iter()
                .copied()
                .filter(|id| !target.contains(id))
                .collect(),
            deck,
        )
        .into_iter()
        .take(size)
        .collect::<Vec<_>>();
        if size == 0 || batch.len() != size || cursor + size > replacement.len() {
            return None;
        }
        current.retain(|id| !batch.contains(id));
        let next = replacement[cursor..cursor + size].to_vec();
        current.extend(&next);
        cursor += size;
        discards.push(batch);
        incoming.push(next);
        if target.iter().all(|id| current.contains(id)) {
            break;
        }
    }
    target
        .iter()
        .all(|id| current.contains(id))
        .then_some((discards, incoming))
}

pub(super) fn search(
    state: &Value,
    skip: &[String],
    emit: &mut dyn FnMut(Value),
    should_stop: &dyn Fn() -> bool,
) -> Value {
    let hand = ids(state.get("hand_tiles"));
    if !hand.contains(&1000) {
        return json!({"status":"impossible","reason":"wanxiang-not-in-hand"});
    }
    let entries = pool(state, 0);
    let mut unique = HashSet::new();
    if entries.iter().any(|entry| {
        !unique.insert(entry.id) || (entry.face != "bd" && souzu::tile(&entry.face).is_none())
    }) {
        return json!({"status":"impossible","reason":"invalid-search-tiles"});
    }
    let now = Instant::now();
    let mut search = Search {
        emit,
        stopped: should_stop,
        started: now,
        last_emit: now,
        meld_total: 0,
        meld_index: 0,
        checks: 0,
        nodes: 0,
    };
    search.progress(true, true);
    if should_stop() {
        return stopped();
    }
    let Some(melds) = search.enumerate(&entries) else {
        return stopped();
    };
    search.progress(false, true);
    let deck = deck(state);
    let (rounds, limit) = search_limits(state);
    let replacement = entries
        .iter()
        .filter(|entry| entry.source == "replacement")
        .map(|entry| entry.id)
        .collect::<Vec<_>>();
    let result = search.visit(0, &mut Vec::new(), &melds, &mut skip.iter().cloned().collect(), &mut |meld_ids, signature| {
        let target = candidate_ids(std::iter::once(1000).chain(meld_ids.iter().copied()).collect(), &deck);
        let (discards, incoming) = simulate(&hand, &target, &replacement, rounds, limit, &deck)?;
        let faces = target.iter().map(|&id| norm(face(&deck, id))).collect::<Vec<_>>();
        let depth = replacement.iter().enumerate().filter(|(_, id)| meld_ids.contains(id)).map(|(i, _)| i + 1).max().unwrap_or(0);
        Some(json!({
            "status":"plan","mode":"wanxiang-four-meld-switch","search_algorithm":"wanxiang_four_meld_switch",
            "draws_needed":depth,"target13":faces,"target14":faces,
            "target_physical_ids":target,"target_physical_faces":faces,
            "discards":discards.first().cloned().unwrap_or_default(),
            "switch_batch_sizes":discards.iter().map(Vec::len).collect::<Vec<_>>(),
            "switch_discards":discards,"switch_in":incoming,"wall_draws":[],"post_draw_discards":[],
            "waits":[],"quad_faces":[],"remaining_changes":rounds,"per_change_limit":limit,"plan_signature":signature
        }))
    });
    search.progress(false, true);
    result.unwrap_or_else(|| json!({"status":"impossible","reason":"cannot-form-four-melds-with-wanxiang","remaining_changes":rounds}))
}
