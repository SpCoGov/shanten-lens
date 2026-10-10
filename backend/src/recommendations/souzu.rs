//! Port of v2.4.1's _run_exact_target_enumeration_search and its _tes_* helpers.
//! Keep iteration order, wall-first matching, and legacy slicing rules: they decide
//! which first solution is returned. Tile indices only replace normalized strings.
use super::{ids, norm, search_limits, SearchPreferences, TileSuit};
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

type Tile = u8;
type Counts = [usize; 34];

pub(super) fn tile(face: &str) -> Option<Tile> {
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
                if key == "plan_signature" {
                    // 3.1 signatures append the quad decomposition to distinguish alternatives.
                    let signature = actual[key].as_str().unwrap().split("|quads:").next().unwrap();
                    assert_eq!(signature, expected.as_str().unwrap(), "case {}", case["name"]);
                } else {
                    assert_eq!(&actual[key], expected, "case {}, field {key}", case["name"]);
                }
            }
        }
    }

    #[test]
    fn legacy_target_order_is_preserved_with_distinct_quad_decompositions() {
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
                any_waits: false,
                bonus_tiles: [false; 34],
                preferred_suit: None,
            };
            let mut digest = Sha256::new();
            let mut legacy_targets = HashSet::new();
            generate(&tiles, rounds, limit, n, &mut search, &mut |target, _| {
                // v2.4.1 collapsed different quad decompositions of the same tiles.
                if legacy_targets.insert(counts(target)) {
                    digest.update(serde_json::to_vec(&faces(target)).unwrap());
                    digest.update(b"\n");
                }
                None
            });
            assert_eq!(
                legacy_targets.len() as u64,
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
fn taatsu(tiles: &[Tile], any_waits: bool) -> Vec<Vec<Tile>> {
    let available = counts(tiles);
    let mut result = Vec::new();
    for base in if any_waits {
        &[0, 9, 18][..]
    } else {
        &[18][..]
    } {
        for gap in [1, 2] {
            for t in *base..*base + 9 - gap {
                if available[t as usize] > 0 && available[(t + gap) as usize] > 0 {
                    result.push(vec![t, t + gap]);
                }
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
    any_waits: bool,
    bonus_tiles: [bool; 34],
    preferred_suit: Option<TileSuit>,
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

// Match the game's indicator mapping used by app/src/lib/tileHighlights.ts.
pub(super) fn preferred_tiles(
    state: &Value,
    preferences: &SearchPreferences,
    available_ids: &[u64],
) -> [bool; 34] {
    let mut preferred = [false; 34];
    if preferences.prefer_dora {
        for id in available_ids {
            if let Some(raw @ ("0m" | "0p" | "0s")) = state["deck_map"][id.to_string()].as_str() {
                preferred[tile(raw).unwrap() as usize] = true;
            }
        }
        for id in ids(state.get("dora_tiles")) {
            let Some(t) = state["deck_map"][id.to_string()].as_str().and_then(tile) else {
                continue;
            };
            let next = match t {
                0 => 8, // The game's 1m indicator points to 9m.
                8 => 0,
                17 => 9,
                26 => 18,
                33 => 27,
                _ => t + 1,
            };
            preferred[next as usize] = true;
        }
    }
    if preferences.prefer_soul {
        for face in state["tian_dora_tiles"].as_array().into_iter().flatten() {
            if let Some(t) = face.as_str().and_then(tile) {
                preferred[t as usize] = true;
            }
        }
    }
    preferred
}

fn quad_groups(tiles: &[Tile], count: usize, search: &mut Search<'_>) -> Vec<Vec<Tile>> {
    let quads = repeated(tiles, 4);
    let mut groups = Vec::new();
    for a in 0..quads.len() {
        for b in a + 1..quads.len() {
            if !search.pulse(false) {
                return Vec::new();
            }
            if count == 2 {
                groups.push([quads[a].as_slice(), quads[b].as_slice()].concat());
            } else {
                for c in b + 1..quads.len() {
                    groups.push(
                        [
                            quads[a].as_slice(),
                            quads[b].as_slice(),
                            quads[c].as_slice(),
                        ]
                        .concat(),
                    );
                }
            }
        }
    }
    let score = |group: &Vec<Tile>| {
        (
            group.chunks_exact(4)
                .filter(|quad| search.preferred_suit.is_some_and(|suit| suit.matches(quad[0])))
                .count(),
            group.chunks_exact(4)
                .filter(|quad| search.bonus_tiles[quad[0] as usize])
                .count(),
        )
    };
    // Stable sorting keeps legacy order for equally preferred quad combinations.
    groups.sort_by_key(|group| std::cmp::Reverse(score(group)));
    search.pairs = groups.len();
    search.pair_index = 0;
    groups
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
    let groups = quad_groups(tiles, 2, search);
    let any_waits = search.any_waits;
    let mut seen = HashSet::new();
    for prefix in groups {
        search.pair_index += 1;
        if !search.pulse(false) {
            return None;
        }
        if impossible_quads(tiles, &prefix, rounds, limit, n) {
            search.pruned += 1;
            continue;
        }
        let remaining = remove(tiles, &prefix)?;
        let melds = melds(&remaining);
        let pairs = repeated(&remaining, 2);
        let taatsu = taatsu(&remaining, any_waits);
        let mut consider = |parts: &[&[Tile]], search: &mut Search<'_>| {
            if !search.pulse(false) {
                return None;
            }
            let target = [prefix.as_slice(), &parts.concat()].concat();
            // Identical total counts can have different waits when different quads are declared.
            if seen.insert((prefix.clone(), counts(&target))) {
                search.targets += 1;
                visit(&target, search)
            } else {
                None
            }
        };
        for i in 0..melds.len() {
            for j in i + usize::from(!any_waits)..melds.len() {
                if !search.pulse(false) {
                    return None;
                }
                let Some(temp) = remove(&remaining, &melds[i]).and_then(|v| remove(&v, &melds[j]))
                else {
                    continue;
                };
                for t in temp.into_iter().filter(|&t| any_waits || bamboo(t)) {
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
                if !any_waits && (!bamboo(pairs[i][0]) || !bamboo(pairs[j][0])) {
                    continue;
                }
                let Some(temp) = remove(&remaining, &pairs[i]).and_then(|v| remove(&v, &pairs[j]))
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
    None
}

struct Round {
    hand: Vec<Tile>,
    keep: Vec<Tile>,
    replace: Vec<Tile>,
}

fn generate_three(
    tiles: &[Tile],
    rounds: usize,
    limit: usize,
    n: usize,
    search: &mut Search<'_>,
    visit: &mut dyn FnMut(&[Tile], &mut Search<'_>) -> Option<Value>,
) -> Option<Value> {
    let _ = (rounds, limit, n);
    let groups = quad_groups(tiles, 3, search);
    let mut seen = HashSet::new();
    for prefix in groups {
        search.pair_index += 1;
        if !search.pulse(false) {
            return None;
        }
        let Some(remaining) = remove(tiles, &prefix) else {
            continue;
        };
        let pairs = repeated(&remaining, 2);
        let mut consider = |tail: &[Tile], search: &mut Search<'_>| {
            let target = [prefix.as_slice(), tail].concat();
            if seen.insert((prefix.clone(), counts(&target))) {
                search.targets += 1;
                visit(&target, search)
            } else {
                None
            }
        };
        for meld in melds(&remaining) {
            let Some(rest) = remove(&remaining, &meld) else {
                continue;
            };
            for single in rest.into_iter() {
                if !search.pulse(false) {
                    return None;
                }
                if let Some(plan) = consider(&[meld.as_slice(), &[single]].concat(), search) {
                    return Some(plan);
                }
            }
        }
        for pair in &pairs {
            let Some(rest) = remove(&remaining, pair) else {
                continue;
            };
            for wait in (0..3).flat_map(|suit| {
                let available = counts(&rest);
                (1..=2).flat_map(move |gap| {
                    (0..9 - gap).filter_map(move |rank| {
                        let t = suit * 9 + rank;
                        (available[t as usize] > 0 && available[(t + gap) as usize] > 0)
                            .then_some(vec![t, t + gap])
                    })
                })
            }) {
                if !search.pulse(false) {
                    return None;
                }
                if let Some(plan) = consider(&[pair.as_slice(), wait.as_slice()].concat(), search) {
                    return Some(plan);
                }
            }
        }
        for i in 0..pairs.len() {
            for j in i + 1..pairs.len() {
                if !search.pulse(false) {
                    return None;
                }
                {
                    if let Some(plan) =
                        consider(&[pairs[i].as_slice(), pairs[j].as_slice()].concat(), search)
                    {
                        return Some(plan);
                    }
                }
            }
        }
    }
    None
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
    if target.len() - needed.len() < target.len() - 13 {
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
        if (0..34).any(|t| available[t] < wanted[t])
            || (target.len() == 15 && !search.any_waits && excluded(target))
        {
            return None;
        }
        let quad_count = target.len() - 13;
        let concealed_start = quad_count * 4;
        let mut waits = waits(&target[concealed_start..]);
        if search.any_waits {
            waits.retain(|face| tile(face).is_some_and(|t| wanted[t as usize] < 4));
        }
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
            "wall_draws":wall_draws,"post_draw_discards":[],"waits":waits,"quad_faces":faces(&(0..quad_count).map(|i|target[i*4]).collect::<Vec<_>>()),"quad_count":quad_count,
            "target13":faces(&target[concealed_start..]),"target14":faces(target),"remaining_changes":self.rounds,
            "preferred_quad_count":target[..concealed_start].chunks_exact(4).filter(|quad|search.bonus_tiles[quad[0] as usize]).count(),
            "preferred_suit_count":target[..concealed_start].chunks_exact(4).filter(|quad|search.preferred_suit.is_some_and(|suit|suit.matches(quad[0]))).count(),
            "plan_signature":format!("target-enum|{}|{}|quads:{}",n+1,sorted.join(","),faces(&target[..concealed_start]).join(",")),
            "switch_rounds":logs.iter().map(|log|json!({"hand":faces(&log.hand),"keep":faces(&log.keep),"replace":faces(&log.replace)})).collect::<Vec<_>>() }),
        )
    }
}

#[cfg(test)]
fn search(
    state: &Value,
    wall_limit: usize,
    emit: &mut dyn FnMut(Value),
    stopped: &dyn Fn() -> bool,
) -> Value {
    search_with_quads(state, wall_limit, 2, emit, stopped)
}

#[cfg(test)]
fn search_with_quads(
    state: &Value,
    wall_limit: usize,
    quad_count: usize,
    emit: &mut dyn FnMut(Value),
    stopped: &dyn Fn() -> bool,
) -> Value {
    search_with_preferences(
        state,
        wall_limit,
        quad_count,
        &[],
        &SearchPreferences::default(),
        emit,
        stopped,
    )
}

pub(super) fn search_with_preferences(
    state: &Value,
    wall_limit: usize,
    quad_count: usize,
    skip: &[String],
    preferences: &SearchPreferences,
    emit: &mut dyn FnMut(Value),
    stopped: &dyn Fn() -> bool,
) -> Value {
    if !(2..=3).contains(&quad_count) {
        return json!({"status":"impossible","reason":"invalid-quad-count"});
    }
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
    if wall_ids.is_empty() || (quad_count == 3 && wall_ids.len() < 3) {
        return json!({"status":"impossible","reason":if quad_count==3 {"wall-less-than-three-draws"} else {"wall-less-than-two-draws"}});
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
        any_waits: preferences.any_waits || quad_count == 3,
        preferred_suit: preferences.preferred_suit,
        bonus_tiles: preferred_tiles(
            state,
            preferences,
            &[
                input.hand_ids.as_slice(),
                input.change_ids.as_slice(),
                input.wall_ids.as_slice(),
            ]
            .concat(),
        ),
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
        if repeated(&all, 4).len() < quad_count {
            return json!({"status":"impossible","reason":if quad_count==3 {"cannot-form-three-quads"} else {"cannot-form-two-quads"}});
        }
        search.phase = "enumerating";
        for n in quad_count..=input.wall.len() {
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
            let generator = if quad_count == 3 {
                generate_three
            } else {
                generate
            };
            if let Some(plan) = generator(
                &tiles,
                rounds,
                limit,
                n,
                &mut search,
                &mut |target, search| {
                    let plan = input.plan(target, n, search)?;
                    (!skip
                        .iter()
                        .any(|signature| plan["plan_signature"] == *signature))
                    .then_some(plan)
                },
            ) {
                return plan;
            }
            if stopped() {
                break;
            }
        }
        if stopped() {
            json!({"status":"impossible","reason":"stopped-by-user"})
        } else {
            json!({"status":"impossible","reason":"no-reliable-plan-found"})
        }
    };
    let mut result = run();
    result["remaining_changes"] = json!(rounds);
    result["max_change_count"] = json!(rounds);
    result["per_change_limit"] = json!(limit);
    result["considered_tile_count"] =
        json!(input.hand.len() + input.changes.len() + input.wall.len());
    result["search_preferences"] = json!(SearchPreferences {
        any_waits: preferences.any_waits || quad_count == 3,
        ..preferences.clone()
    });
    result
}

#[cfg(test)]
mod three_quad_tests {
    use super::*;
    fn state(hand: &[&str], wall: &[&str]) -> Value {
        let mut deck = serde_json::Map::new();
        for (i, face) in hand.iter().chain(wall).enumerate() {
            deck.insert((i + 1).to_string(), json!(face));
        }
        json!({"deck_map":deck,"hand_tiles":(1..=13).collect::<Vec<u64>>(),"wall_tiles":(14..14+wall.len() as u64).collect::<Vec<_>>(),"replacement_tiles":[],"change_tile_count":0,"total_change_tile_count":0})
    }
    #[test]
    fn three_quads_accepts_all_wait_suits_and_honors() {
        for (tail, wall, expected) in [
            ("4m", ["5m", "6m", "7m"], "7m"),
            ("4p", ["5p", "6p", "7p"], "7p"),
            ("4s", ["5s", "6s", "7s"], "7s"),
            ("7p", ["8p", "9p", "1z"], "1z"),
            ("4m", ["5m", "1z", "1z"], "6m"),
            ("1z", ["1z", "2z", "2z"], "2z"),
        ] {
            let hand = [
                "1m", "1m", "1m", "1m", "2p", "2p", "2p", "2p", "3s", "3s", "3s", "3s", tail,
            ];
            let plan = search_with_quads(&state(&hand, &wall), 36, 3, &mut |_| {}, &|| false);
            assert_eq!(plan["status"], "plan", "{plan}");
            assert_eq!(plan["quad_faces"].as_array().unwrap().len(), 3);
            assert_eq!(plan["target14"].as_array().unwrap().len(), 16);
            assert_eq!(plan["target13"].as_array().unwrap().len(), 4);
            assert!(
                plan["waits"].as_array().unwrap().contains(&json!(expected)),
                "{plan}"
            );
        }
    }
    #[test]
    fn three_quads_can_exchange_to_the_target() {
        let hand = [
            "1m", "1m", "1m", "1m", "2p", "2p", "2p", "2p", "3s", "3s", "3s", "9m", "4p",
        ];
        let mut s = state(&hand, &["5p", "6p", "7p"]);
        s["deck_map"]["17"] = json!("3s");
        s["replacement_tiles"] = json!([17]);
        s["total_change_tile_count"] = json!(1);
        let plan = search_with_quads(&s, 36, 3, &mut |_| {}, &|| false);
        assert_eq!(plan["status"], "plan", "{plan}");
        assert_eq!(plan["switch_discards"], json!([[12]]));
        assert_eq!(plan["switch_in"], json!([[17]]));
    }
    #[test]
    fn three_quad_limits_and_cancellation() {
        let hand = [
            "1m", "1m", "1m", "1m", "2p", "2p", "2p", "2p", "3s", "3s", "3s", "3s", "4p",
        ];
        let s = state(&hand, &["5p", "6p", "7p"]);
        assert_eq!(
            search_with_quads(&s, 2, 3, &mut |_| {}, &|| false)["reason"],
            "wall-less-than-three-draws"
        );
        assert_eq!(
            search_with_quads(&s, 36, 3, &mut |_| {}, &|| true)["reason"],
            "stopped-by-user"
        );
        let no = state(
            &[
                "1m", "1m", "1m", "1m", "2p", "2p", "2p", "2p", "3s", "3s", "4s", "5s", "6s",
            ],
            &["7s", "8s", "9s"],
        );
        assert_eq!(
            search_with_quads(&no, 36, 3, &mut |_| {}, &|| false)["reason"],
            "cannot-form-three-quads"
        );
    }
    #[test]
    fn two_quad_mode_keeps_two_quad_output() {
        let s = state(
            &[
                "1m", "1m", "1m", "1m", "2p", "2p", "2p", "2p", "3s", "4s", "5s", "6s", "7s",
            ],
            &["8s", "9s"],
        );
        let plan = search(&s, 36, &mut |_| {}, &|| false);
        assert_eq!(plan["status"], "plan", "{plan}");
        assert_eq!(plan["quad_faces"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn unrestricted_two_quads_accept_all_suits_and_repeated_sequences() {
        let preferences = SearchPreferences {
            any_waits: true,
            ..Default::default()
        };
        for suit in ['m', 'p', 's'] {
            let mut hand = vec!["1z".to_owned(); 4];
            hand.extend(vec!["2z".to_owned(); 4]);
            hand.extend((1..=5).map(|rank| format!("{rank}{suit}")));
            let wall = [format!("6{suit}"), format!("7{suit}")];
            let s = state(
                &hand.iter().map(String::as_str).collect::<Vec<_>>(),
                &wall.iter().map(String::as_str).collect::<Vec<_>>(),
            );
            let plan =
                search_with_preferences(&s, 36, 2, &[], &preferences, &mut |_| {}, &|| false);
            assert_eq!(plan["status"], "plan", "{plan}");
            assert_eq!(plan["draws_needed"], 2);
            assert!(plan["waits"]
                .as_array()
                .unwrap()
                .iter()
                .all(|v| v.as_str().unwrap().ends_with(suit)));
            if suit != 's' {
                assert_eq!(
                    search(&s, 36, &mut |_| {}, &|| false)["status"],
                    "impossible"
                );
            }
        }
        for (hand, wall, expected) in [
            (
                [
                    "1m", "1m", "1m", "1m", "2p", "2p", "2p", "2p", "4s", "5s", "6s", "1z", "1z",
                ],
                ["2z", "2z"],
                "2z",
            ),
            (
                [
                    "1z", "1z", "1z", "1z", "2z", "2z", "2z", "2z", "1m", "2m", "3m", "1m", "2m",
                ],
                ["3m", "4m"],
                "4m",
            ),
        ] {
            let plan = search_with_preferences(
                &state(&hand, &wall),
                36,
                2,
                &[],
                &preferences,
                &mut |_| {},
                &|| false,
            );
            assert_eq!(plan["status"], "plan", "{plan}");
            assert!(
                plan["waits"].as_array().unwrap().contains(&json!(expected)),
                "{plan}"
            );
        }
        let fifth_tile = state(
            &[
                "1m", "1m", "1m", "1m", "2p", "2p", "2p", "2p", "3s", "3s", "3s", "3s", "1z",
            ],
            &["1z", "1z"],
        );
        assert_eq!(
            search_with_preferences(&fifth_tile, 36, 2, &[], &preferences, &mut |_| {}, &|| {
                false
            })["status"],
            "impossible"
        );
    }

    #[test]
    fn suit_preferences_rank_quads_and_keep_earlier_plans() {
        for (suit, preferred_suit) in [('m', TileSuit::Man), ('p', TileSuit::Pin), ('s', TileSuit::Sou), ('z', TileSuit::Honor)] {
            let others = ['m', 'p', 's', 'z'].into_iter().filter(|&other| other != suit).collect::<Vec<_>>();
            let mut hand = vec![format!("1{}", others[0]); 4];
            hand.extend(vec![format!("2{}", others[1]); 4]);
            hand.extend((3..=7).map(|rank| format!("{rank}s")));
            let mut snapshot = state(&hand.iter().map(String::as_str).collect::<Vec<_>>(), &["8s", "9s"]);
            for id in 100..104 {
                snapshot["deck_map"][id.to_string()] = json!(format!("1{suit}"));
            }
            snapshot["replacement_tiles"] = json!([100, 101, 102, 103]);
            snapshot["total_change_tile_count"] = json!(1);
            snapshot["tian_dora_tiles"] = json!([hand[0], hand[4]]);
            let mut preferences = SearchPreferences { any_waits: true, prefer_soul: true, ..Default::default() };
            let ordinary = search_with_preferences(&snapshot, 36, 2, &[], &preferences, &mut |_| {}, &|| false);
            assert_eq!(ordinary["status"], "plan", "{ordinary}");
            preferences.preferred_suit = Some(preferred_suit);
            let preferred = search_with_preferences(&snapshot, 36, 2, &[], &preferences, &mut |_| {}, &|| false);
            assert_eq!(preferred["draws_needed"], ordinary["draws_needed"]);
            assert_eq!(preferred["preferred_suit_count"], 1, "{preferred}");
            assert_eq!(preferred["preferred_quad_count"], 1); // Suit preference wins over two bonus quads.
            assert!(preferred["quad_faces"].as_array().unwrap().contains(&json!(format!("1{suit}"))));
            assert_ne!(preferred["plan_signature"], ordinary["plan_signature"]);
        }

        let mut snapshot = state(&["1m", "1m", "1m", "1m", "2p", "2p", "2p", "2p", "3s", "3s", "3s", "3s", "4s"], &["5s", "6s", "7s"]);
        let mut preferences = SearchPreferences { any_waits: true, preferred_suit: Some(TileSuit::Sou), ..Default::default() };
        let early = search_with_preferences(&snapshot, 36, 2, &[], &preferences, &mut |_| {}, &|| false);
        assert_eq!(early["draws_needed"], 2);
        assert_eq!(early["preferred_suit_count"], 0); // Bamboo quads must not delay tenpai.
        preferences.preferred_suit = Some(TileSuit::Honor);
        let fallback = search_with_preferences(&snapshot, 36, 2, &[], &preferences, &mut |_| {}, &|| false);
        assert_eq!(fallback["plan_signature"], early["plan_signature"]);
        for id in 100..104 {
            snapshot["deck_map"][id.to_string()] = json!("1z");
        }
        snapshot["replacement_tiles"] = json!([100, 101, 102, 103]);
        snapshot["total_change_tile_count"] = json!(1);
        let three = search_with_preferences(&snapshot, 36, 3, &[], &preferences, &mut |_| {}, &|| false);
        assert_eq!(three["quad_count"], 3, "{three}");
        assert_eq!(three["draws_needed"], 3);
        assert_eq!(three["preferred_suit_count"], 1);
    }

    #[test]
    fn preferences_break_ties_without_delaying_tenpai_and_fall_back() {
        let mut s = state(
            &[
                "4m", "4m", "4m", "4m", "7m", "7m", "7m", "7m", "1p", "1p", "1p", "1p", "6m",
            ],
            &["5m", "1z"],
        );
        s["deck_map"]["500"] = json!("6m");
        s["dora_tiles"] = json!([500]);
        s["tian_dora_tiles"] = json!(["7m"]);
        let mut preferences = SearchPreferences {
            any_waits: true,
            ..Default::default()
        };
        let base = search_with_preferences(&s, 36, 2, &[], &preferences, &mut |_| {}, &|| false);
        assert_eq!(base["quad_faces"], json!(["4m", "1p"]), "{base}");
        for (dora, soul) in [(true, false), (false, true), (true, true)] {
            preferences.prefer_dora = dora;
            preferences.prefer_soul = soul;
            let plan =
                search_with_preferences(&s, 36, 2, &[], &preferences, &mut |_| {}, &|| false);
            assert_eq!(plan["quad_faces"], json!(["7m", "1p"]), "{plan}");
            assert_eq!(plan["preferred_quad_count"], 1); // A soul+dora quad counts only once.
            assert_eq!(plan["draws_needed"], base["draws_needed"]);
        }
        let mut later = state(
            &[
                "1m", "1m", "1m", "1m", "2p", "2p", "2p", "2p", "3s", "3s", "3s", "3s", "4s",
            ],
            &["5s", "6s", "2m", "3m"],
        );
        later["tian_dora_tiles"] = json!(["3s"]);
        let early =
            search_with_preferences(&later, 36, 2, &[], &preferences, &mut |_| {}, &|| false);
        assert_eq!(early["draws_needed"], 2, "{early}");
        assert_eq!(early["preferred_quad_count"], 0);
        let skip = vec![early["plan_signature"].as_str().unwrap().to_owned()];
        let next =
            search_with_preferences(&later, 36, 2, &skip, &preferences, &mut |_| {}, &|| false);
        assert_eq!(next["status"], "plan");
        assert_ne!(next["plan_signature"], early["plan_signature"]);
        assert_eq!(
            search_with_preferences(&later, 36, 2, &[], &preferences, &mut |_| {}, &|| true)
                ["reason"],
            "stopped-by-user"
        );
        // Indicator wrapping, red fives and soul fives use the same normalization as the UI.
        let tiles = json!({"deck_map":{"1":"1m","2":"9p","3":"7z","4":"0s"},"dora_tiles":[1,2,3],"tian_dora_tiles":["0m"]});
        let preferred = preferred_tiles(&tiles, &preferences, &[4]);
        for face in ["9m", "1p", "1z", "5s", "5m"] {
            assert!(preferred[tile(face).unwrap() as usize], "{face}");
        }
    }
}
