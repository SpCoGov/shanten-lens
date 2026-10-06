//! Port of v2.4.1's _run_exact_target_enumeration_search and its _tes_* helpers.
//! Keep iteration order, wall-first matching, and legacy slicing rules: they decide
//! which first solution is returned. Tile indices only replace normalized strings.
use super::{ids, norm, search_limits};
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

type Tile = u8;
type Counts = [usize; 34];

fn tile(face: &str) -> Option<Tile> {
    let face = norm(face);
    let b = face.as_bytes();
    if b.len() != 2 {
        return None;
    }
    let base = match b[1] {
        b'm' => 0,
        b'p' => 9,
        b's' => 18,
        b'z' => 27,
        _ => return None,
    };
    let rank = b[0].checked_sub(b'1')?;
    (rank < if base == 27 { 7 } else { 9 }).then_some(base + rank)
}

fn face(t: Tile) -> String {
    format!("{}{}", t % 9 + 1, ['m', 'p', 's', 'z'][(t / 9) as usize])
}
fn faces(tiles: &[Tile]) -> Vec<String> {
    tiles.iter().map(|&t| face(t)).collect()
}
fn bamboo(t: Tile) -> bool {
    (18..27).contains(&t)
}
fn counts(tiles: &[Tile]) -> Counts {
    let mut result = [0; 34];
    for &t in tiles {
        result[t as usize] += 1;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn fixtures() -> Value {
        serde_json::from_str(include_str!("../../tests/fixtures/souzu_v241.json")).unwrap()
    }

    #[test]
    fn matches_unmodified_v241_results() {
        for case in fixtures()["cases"].as_array().unwrap() {
            let actual = super::super::switch_plan(
                &case["state"],
                case["wall_limit"].as_u64().unwrap() as usize,
                &[],
                Some("target_enumeration_search"),
            );
            for (key, expected) in case["expected"].as_object().unwrap() {
                assert_eq!(&actual[key], expected, "case {}, field {key}", case["name"]);
            }
        }
    }

    #[test]
    fn target_generation_order_matches_v241() {
        use sha2::{Digest, Sha256};
        for case in fixtures()["cases"].as_array().unwrap() {
            let Some(expected) = case.get("generation") else {
                continue;
            };
            let state = &case["state"];
            let (rounds, limit) = search_limits(state);
            let entries = super::super::pool(state, case["wall_limit"].as_u64().unwrap() as usize);
            let tiles = entries
                .iter()
                .map(|entry| tile(&entry.face).unwrap())
                .collect::<Vec<_>>();
            let n = entries
                .iter()
                .filter(|entry| entry.source == "wall")
                .count();
            let now = Instant::now();
            let mut search = Search {
                emit: &mut |_| {},
                stopped: &|| false,
                started: now,
                last_emit: now,
                prefix: n,
                prefix_total: n,
                targets: 0,
                checks: 0,
                pairs: 0,
                pair_index: 0,
                pruned: 0,
                phase: "enumerating",
            };
            let mut digest = Sha256::new();
            generate(&tiles, rounds, limit, n, &mut search, &mut |target, _| {
                digest.update(serde_json::to_vec(&faces(target)).unwrap());
                digest.update(b"\n");
                None
            });
            assert_eq!(
                search.targets as u64,
                expected["count"].as_u64().unwrap(),
                "{}",
                case["name"]
            );
            assert_eq!(
                format!("{:x}", digest.finalize()),
                expected["sha256"].as_str().unwrap(),
                "{}",
                case["name"]
            );
        }
    }

    #[test]
    fn reports_progress_and_stops_inside_enumeration() {
        let cases = fixtures();
        let case = cases["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["name"] == "bamboo-2")
            .unwrap();
        let checks = Cell::new(0);
        let mut updates = Vec::new();
        let result = search(
            &case["state"],
            36,
            &mut |value| updates.push(value),
            &|| {
                checks.set(checks.get() + 1);
                checks.get() >= 12
            },
        );
        assert_eq!(result["reason"], "stopped-by-user");
        assert!(updates
            .iter()
            .any(|update| update["phase"] == "enumerating"));
        assert!(
            checks.get() < 30,
            "cancelled search must stop visiting candidates"
        );
    }
}
fn remove(tiles: &[Tile], wanted: &[Tile]) -> Option<Vec<Tile>> {
    let mut result = tiles.to_vec();
    for t in wanted {
        let index = result.iter().position(|item| item == t)?;
        result.remove(index);
    }
    Some(result)
}
fn repeated(tiles: &[Tile], size: usize) -> Vec<Vec<Tile>> {
    let mut available = counts(tiles);
    let mut result = Vec::new();
    // Python Counter preserves the order of first occurrence.
    for &t in tiles {
        if available[t as usize] >= size {
            result.push(vec![t; size]);
        }
        available[t as usize] = 0;
    }
    result
}
fn melds(tiles: &[Tile]) -> Vec<Vec<Tile>> {
    let mut result = repeated(tiles, 3);
    let available = counts(tiles);
    for base in [0, 9, 18] {
        for rank in 0..7 {
            let t = base + rank;
            if (t..t + 3).all(|t| available[t as usize] > 0) {
                result.push(vec![t, t + 1, t + 2]);
            }
        }
    }
    result
}
fn taatsu(tiles: &[Tile]) -> Vec<Vec<Tile>> {
    let available = counts(tiles);
    let mut result = Vec::new();
    for gap in [1, 2] {
        for t in 18..27 - gap {
            if available[t as usize] > 0 && available[(t + gap) as usize] > 0 {
                result.push(vec![t, t + gap]);
            }
        }
    }
    result
}

fn last_indices(tiles: &[Tile], target: &[Tile]) -> Option<Vec<usize>> {
    let mut positions: [Vec<usize>; 34] = std::array::from_fn(|_| Vec::new());
    for (index, &t) in tiles.iter().enumerate() {
        positions[t as usize].push(index);
    }
    let mut selected = Vec::new();
    for &t in target {
        selected.push(positions[t as usize].pop()?);
    }
    selected.sort_unstable();
    Some(selected)
}
fn reachable_end(selected: &[usize], rounds: usize, limit: usize) -> usize {
    let mut end = 13;
    for _ in 0..rounds {
        end += (13 - selected.partition_point(|&index| index < end)).min(limit);
    }
    end
}
fn impossible_quads(tiles: &[Tile], quads: &[Tile], rounds: usize, limit: usize, n: usize) -> bool {
    let Some(selected) = last_indices(tiles, quads) else {
        return true;
    };
    let end = reachable_end(&selected, rounds, limit);
    let nonwall = selected.partition_point(|&index| index < tiles.len() - n);
    selected[if nonwall == 0 {
        selected.len()
    } else {
        nonwall
    } - 1]
        > end
}

struct Search<'a> {
    emit: &'a mut dyn FnMut(Value),
    stopped: &'a dyn Fn() -> bool,
    started: Instant,
    last_emit: Instant,
    prefix: usize,
    prefix_total: usize,
    targets: usize,
    checks: usize,
    pairs: usize,
    pair_index: usize,
    pruned: usize,
    phase: &'static str,
}
impl Search<'_> {
    fn pulse(&mut self, force: bool) -> bool {
        if (self.stopped)() {
            return false;
        }
        if force || self.last_emit.elapsed() >= Duration::from_millis(150) {
            self.last_emit = Instant::now();
            (self.emit)(json!({"phase":self.phase,"wall_prefix":self.prefix,
                "wall_total":self.prefix_total,"targets":self.targets,"checks":self.checks,
                "quad_pairs":self.pairs,"quad_pair_index":self.pair_index,
                "pruned":self.pruned,"elapsed_ms":self.started.elapsed().as_millis() as u64}));
        }
        true
    }
}

// Stream targets in exactly the original generator's order, instead of retaining
// every target before testing the first one. Canonical counts preserve its dedup.
fn generate(
    tiles: &[Tile],
    rounds: usize,
    limit: usize,
    n: usize,
    search: &mut Search<'_>,
    visit: &mut dyn FnMut(&[Tile], &mut Search<'_>) -> Option<Value>,
) -> Option<Value> {
    let quads = repeated(tiles, 4);
    search.pairs = quads.len() * quads.len().saturating_sub(1) / 2;
    search.pair_index = 0;
    let mut seen = HashSet::new();
    for a in 0..quads.len() {
        for b in a + 1..quads.len() {
            search.pair_index += 1;
            if !search.pulse(false) {
                return None;
            }
            let prefix = [quads[a].as_slice(), quads[b].as_slice()].concat();
            if impossible_quads(tiles, &prefix, rounds, limit, n) {
                search.pruned += 1;
                continue;
            }
            let remaining = remove(tiles, &prefix)?;
            let melds = melds(&remaining);
            let pairs = repeated(&remaining, 2);
            let taatsu = taatsu(&remaining);
            let mut consider = |parts: &[&[Tile]], search: &mut Search<'_>| {
                if !search.pulse(false) {
                    return None;
                }
                let target = [prefix.as_slice(), &parts.concat()].concat();
                if seen.insert(counts(&target)) {
                    search.targets += 1;
                    visit(&target, search)
                } else {
                    None
                }
            };
            for i in 0..melds.len() {
                for j in i + 1..melds.len() {
                    if !search.pulse(false) {
                        return None;
                    }
                    let Some(temp) =
                        remove(&remaining, &melds[i]).and_then(|v| remove(&v, &melds[j]))
                    else {
                        continue;
                    };
                    for t in temp.into_iter().filter(|&t| bamboo(t)) {
                        if let Some(plan) = consider(&[&melds[i], &melds[j], &[t]], search) {
                            return Some(plan);
                        }
                    }
                }
            }
            for meld in &melds {
                let Some(temp) = remove(&remaining, meld) else {
                    continue;
                };
                for pair in &pairs {
                    if !search.pulse(false) {
                        return None;
                    }
                    if remove(&temp, pair).is_none() {
                        continue;
                    }
                    // v2.4.1 deliberately does not remove the taatsu here.
                    for wait in &taatsu {
                        if let Some(plan) = consider(&[meld, wait, pair], search) {
                            return Some(plan);
                        }
                    }
                }
            }
            for i in 0..pairs.len() {
                for j in i + 1..pairs.len() {
                    if !search.pulse(false) {
                        return None;
                    }
                    if !bamboo(pairs[i][0]) || !bamboo(pairs[j][0]) {
                        continue;
                    }
                    let Some(temp) =
                        remove(&remaining, &pairs[i]).and_then(|v| remove(&v, &pairs[j]))
                    else {
                        continue;
                    };
                    for meld in &melds {
                        if remove(&temp, meld).is_none() {
                            continue;
                        }
                        if let Some(plan) = consider(&[meld, &pairs[i], &pairs[j]], search) {
                            return Some(plan);
                        }
                    }
                }
            }
        }
    }
    None
}

struct Round {
    hand: Vec<Tile>,
    keep: Vec<Tile>,
    replace: Vec<Tile>,
}

fn simulate(
    hand: &[Tile],
    changes: &[Tile],
    wall: &[Tile],
    target: &[Tile],
    rounds: usize,
    limit: usize,
    search: &mut Search<'_>,
) -> Option<(Vec<Tile>, Vec<Round>)> {
    let mut needed = target.to_vec();
    for t in wall {
        if let Some(index) = needed.iter().position(|v| v == t) {
            needed.remove(index);
        }
    }
    if target.len() - needed.len() < 2 {
        return None;
    }
    let original = [hand, changes].concat();
    let mut all = original.as_slice();
    let selected = loop {
        if !search.pulse(false) {
            return None;
        }
        let selected = last_indices(all, &needed)?;
        let end = reachable_end(&selected, rounds, limit);
        if all.len() <= end {
            if selected.partition_point(|&i| i < end) != needed.len() {
                return None;
            }
            break selected;
        }
        all = &original[..end];
    };
    let replaceable = all
        .iter()
        .enumerate()
        .filter(|(i, _)| !selected.contains(i))
        .map(|(_, &t)| t)
        .collect::<Vec<_>>();
    let mut end = 13;
    let mut cursor = 0;
    let mut logs = Vec::new();
    let mut hand = hand.to_vec();
    for _ in 0..rounds {
        if !search.pulse(false) {
            return None;
        }
        let keep = selected
            .iter()
            .filter(|&&i| i < end)
            .map(|&i| all[i])
            .collect::<Vec<_>>();
        let size = (13 - keep.len()).min(limit);
        let old_replace =
            &replaceable[cursor.min(replaceable.len())..(cursor + size).min(replaceable.len())];
        cursor += size;
        hand = [
            keep.as_slice(),
            &original[end.min(original.len())..(end + size).min(original.len())],
        ]
        .concat();
        // Preserve Python's [:min(size, 62-end)], including a negative slice end.
        let stop = (size as isize).min(62 - end as isize);
        let stop = if stop < 0 {
            (old_replace.len() as isize + stop).max(0) as usize
        } else {
            stop as usize
        };
        let replace = old_replace[..stop.min(old_replace.len())].to_vec();
        end = (end + size).min(original.len());
        logs.push(Round {
            hand: hand.clone(),
            keep,
            replace,
        });
    }
    Some((hand, logs))
}

fn excluded(target: &[Tile]) -> bool {
    let patterns = (0..7)
        .flat_map(|n| [11123 + 11111 * n, 12333 + 11111 * n, 12223 + 11111 * n])
        .chain((0..9).map(|n| 1111 + 1111 * n))
        .collect::<Vec<u32>>();
    let code = |tiles: &[Tile]| tiles.iter().fold(0, |v, &t| v * 10 + (t % 9 + 1) as u32);
    if target[8..13].iter().all(|&t| bamboo(t)) && !bamboo(target[13]) && target[13] == target[14] {
        let mut first = target[8..13].to_vec();
        first.sort_unstable();
        if patterns.contains(&code(&first)) {
            return true;
        }
    }
    if bamboo(target[14]) {
        for start in [8, 11] {
            if bamboo(target[start])
                && patterns
                    .contains(&(code(&target[start..start + 3]) * 10 + (target[14] % 9 + 1) as u32))
            {
                return true;
            }
        }
    }
    false
}

fn consume(mut c: Counts) -> bool {
    let Some(first) = c.iter().position(|&n| n > 0) else {
        return true;
    };
    if c[first] >= 3 {
        let mut next = c;
        next[first] -= 3;
        if consume(next) {
            return true;
        }
    }
    if first < 27 && first % 9 < 7 && c[first + 1] > 0 && c[first + 2] > 0 {
        for n in &mut c[first..first + 3] {
            *n -= 1;
        }
        return consume(c);
    }
    false
}
fn waits(hand: &[Tile]) -> Vec<String> {
    let mut result = Vec::new();
    for t in 0..34 {
        let mut c = counts(hand);
        c[t] += 1;
        if (0..34).any(|pair| {
            if c[pair] < 2 {
                return false;
            }
            let mut next = c;
            next[pair] -= 2;
            consume(next)
        }) {
            result.push(face(t as Tile));
        }
    }
    result
}

struct Input {
    hand_ids: Vec<u64>,
    change_ids: Vec<u64>,
    wall_ids: Vec<u64>,
    hand: Vec<Tile>,
    changes: Vec<Tile>,
    wall: Vec<Tile>,
    rounds: usize,
    limit: usize,
}
impl Input {
    fn plan(&self, target: &[Tile], n: usize, search: &mut Search<'_>) -> Option<Value> {
        search.checks += 1;
        let (final_hand, logs) = simulate(
            &self.hand,
            &self.changes,
            &self.wall[..n],
            target,
            self.rounds,
            self.limit,
            search,
        )?;
        let available = counts(&[final_hand.as_slice(), &self.wall[..n]].concat());
        let wanted = counts(target);
        if (0..34).any(|t| available[t] < wanted[t]) || excluded(target) {
            return None;
        }
        let waits = waits(&target[8..]);
        if waits.is_empty() {
            return None;
        }
        let mut wanted = wanted;
        let final_counts = counts(&final_hand);
        let draw_faces = self
            .wall
            .iter()
            .take(n + 1)
            .copied()
            .filter(|&t| {
                if wanted[t as usize] > final_counts[t as usize] {
                    wanted[t as usize] -= 1;
                    true
                } else {
                    false
                }
            })
            .collect::<Vec<_>>();
        let mut wanted_draws = counts(&draw_faces);
        let wall_draws = self
            .wall_ids
            .iter()
            .zip(&self.wall)
            .take(n + 1)
            .filter_map(|(&id, &t)| {
                if wanted_draws[t as usize] == 0 {
                    None
                } else {
                    wanted_draws[t as usize] -= 1;
                    Some(id)
                }
            })
            .collect::<Vec<_>>();
        let mut current = self
            .hand_ids
            .iter()
            .copied()
            .zip(self.hand.iter().copied())
            .collect::<Vec<_>>();
        let mut cursor = 0;
        let mut discards = Vec::new();
        let mut incoming = Vec::new();
        for log in &logs {
            let mut keep = counts(&log.keep);
            let mut out = Vec::new();
            current.retain(|&(id, t)| {
                if keep[t as usize] > 0 {
                    keep[t as usize] -= 1;
                    true
                } else if out.len() < log.replace.len() {
                    out.push(id);
                    false
                } else {
                    true
                }
            });
            let end = (cursor + log.replace.len()).min(self.change_ids.len());
            let next = self.change_ids[cursor.min(end)..end].to_vec();
            current.extend(
                self.change_ids[cursor.min(end)..end]
                    .iter()
                    .copied()
                    .zip(self.changes[cursor.min(end)..end].iter().copied()),
            );
            cursor += log.replace.len();
            discards.push(out);
            incoming.push(next);
        }
        let mut sorted = faces(target);
        sorted.sort(); // Python sorted(strings), not suit order.
        Some(
            json!({"status":"plan","mode":"target-enumeration-search","draws_needed":n,
            "switch_discards":discards,"switch_in":incoming,"switch_batch_sizes":discards.iter().map(Vec::len).collect::<Vec<_>>(),
            "wall_draws":wall_draws,"post_draw_discards":[],"waits":waits,"quad_faces":faces(&[target[0],target[4]]),
            "target13":faces(&target[8..]),"target14":faces(target),"remaining_changes":self.rounds,
            "plan_signature":format!("target-enum|{}|{}",n+1,sorted.join(",")),
            "switch_rounds":logs.iter().map(|log|json!({"hand":faces(&log.hand),"keep":faces(&log.keep),"replace":faces(&log.replace)})).collect::<Vec<_>>() }),
        )
    }
}

pub(super) fn search(
    state: &Value,
    wall_limit: usize,
    emit: &mut dyn FnMut(Value),
    stopped: &dyn Fn() -> bool,
) -> Value {
    let hand_ids = ids(state.get("hand_tiles"));
    if hand_ids.len() != 13 {
        return json!({"status":"impossible","reason":"switch-hand-must-be-13"});
    }
    let (rounds, limit) = search_limits(state);
    let change_ids = ids(state.get("replacement_tiles"))
        .into_iter()
        .skip(ids(state.get("switch_used_tiles")).len())
        .take(rounds.saturating_mul(limit))
        .collect::<Vec<_>>();
    let wall_ids = ids(state.get("wall_tiles"))
        .into_iter()
        .take(if wall_limit >= 36 {
            usize::MAX
        } else {
            wall_limit.max(2)
        })
        .collect::<Vec<_>>();
    if wall_ids.is_empty() {
        return json!({"status":"impossible","reason":"wall-less-than-two-draws"});
    }
    let decode = |ids: &[u64]| {
        ids.iter()
            .map(|id| tile(state.get("deck_map")?.get(id.to_string())?.as_str()?))
            .collect::<Option<Vec<_>>>()
    };
    let (Some(hand), Some(changes), Some(wall)) =
        (decode(&hand_ids), decode(&change_ids), decode(&wall_ids))
    else {
        return json!({"status":"impossible","reason":"invalid-search-tiles"});
    };
    let input = Input {
        hand_ids,
        change_ids,
        wall_ids,
        hand,
        changes,
        wall,
        rounds,
        limit,
    };
    let started = Instant::now();
    let mut search = Search {
        emit,
        stopped,
        started,
        last_emit: started,
        prefix: 0,
        prefix_total: input.wall.len(),
        targets: 0,
        checks: 0,
        pairs: 0,
        pair_index: 0,
        pruned: 0,
        phase: "preparing",
    };
    let mut run = || {
        if !search.pulse(true) {
            return json!({"status":"impossible","reason":"stopped-by-user"});
        }
        let all = [
            input.hand.as_slice(),
            input.changes.as_slice(),
            input.wall.as_slice(),
        ]
        .concat();
        if repeated(&all, 4).len() < 2 {
            return json!({"status":"impossible","reason":"cannot-form-two-quads"});
        }
        search.phase = "enumerating";
        for n in 2..=input.wall.len() {
            search.prefix = n;
            if !search.pulse(true) {
                break;
            }
            let tiles = [
                input.hand.as_slice(),
                input.changes.as_slice(),
                &input.wall[..n],
            ]
            .concat();
            if let Some(plan) = generate(
                &tiles,
                rounds,
                limit,
                n,
                &mut search,
                &mut |target, search| input.plan(target, n, search),
            ) {
                return plan;
            }
            if stopped() {
                break;
            }
        }
        json!({"status":"impossible","reason":if stopped(){"stopped-by-user"}else{"no-reliable-plan-found"}})
    };
    let mut result = run();
    result["remaining_changes"] = json!(rounds);
    result["max_change_count"] = json!(rounds);
    result["per_change_limit"] = json!(limit);
    result["considered_tile_count"] =
        json!(input.hand.len() + input.changes.len() + input.wall.len());
    result
}
