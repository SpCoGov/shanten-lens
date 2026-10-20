//! Match user constraints to distinct physical tiles, then verify every exchange, draw and kan.
use super::{deck, face, ids, norm, pool, search_limits, souzu, Entry, SearchPreferences};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

#[derive(Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Joker {
    #[default]
    Allow,
    Exclude,
    Only,
}

#[derive(Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct Rule {
    faces: Vec<String>,
    suits: Vec<String>,
    ranks: Vec<u8>,
    red: bool,
    dora: bool,
    soul: bool,
    joker: Joker,
}

#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Group {
    #[serde(default)]
    quad: bool,
    #[serde(default)]
    kind: Kind,
    #[serde(default)]
    suit_group: String,
    #[serde(default)]
    distinct_group: String,
    rule: Rule,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Kind {
    #[default]
    Single,
    Pair,
    Sequence,
    Triplet,
    Meld,
    Quad,
}

impl Kind {
    fn size(self) -> usize {
        match self {
            Self::Single => 1,
            Self::Pair => 2,
            Self::Quad => 4,
            _ => 3,
        }
    }

    fn accepts(self, faces: &[u8]) -> bool {
        if faces.len() != self.size() { return false; }
        let same = faces.iter().all(|face| *face == faces[0]);
        let mut sorted = faces.to_vec();
        sorted.sort_unstable();
        let sequence = sorted.len() == 3 && sorted[0] < 27 && sorted[0] % 9 <= 6
            && sorted[1] == sorted[0] + 1 && sorted[2] == sorted[0] + 2;
        match self {
            Self::Single => true,
            Self::Pair | Self::Triplet | Self::Quad => same,
            Self::Sequence => sequence,
            Self::Meld => same || sequence,
        }
    }
}

impl Group {
    fn kind(&self) -> Kind {
        if self.quad { Kind::Quad } else { self.kind }
    }

    fn is_quad(&self) -> bool {
        self.kind() == Kind::Quad
    }
}

impl Rule {
    fn valid(&self) -> bool {
        self.faces.len() <= 37
            && self.faces.iter().all(|f| souzu::tile(f).is_some())
            && self.suits.len() <= 4
            && self
                .suits
                .iter()
                .all(|s| matches!(s.as_str(), "m" | "p" | "s" | "z"))
            && self.ranks.len() <= 9
            && self.ranks.iter().all(|n| (1..=9).contains(n))
    }

    fn ordinary(&self, raw: &str, dora: &[bool; 34], soul: &[bool; 34]) -> bool {
        let Some(tile) = souzu::tile(raw) else {
            return false;
        };
        (self.faces.is_empty()
            || self.faces.iter().any(|f| {
                if f.starts_with('0') {
                    f == raw
                } else {
                    norm(f) == norm(raw)
                }
            }))
            && (self.suits.is_empty() || self.suits.iter().any(|s| raw.ends_with(s)))
            && (self.ranks.is_empty() || self.ranks.contains(&(tile % 9 + 1)))
            && (!self.red || raw.starts_with('0'))
            && (!self.dora || dora[tile as usize])
            && (!self.soul || soul[tile as usize])
    }

    fn matches(&self, raw: &str, dora: &[bool; 34], soul: &[bool; 34]) -> bool {
        if raw != "bd" {
            return self.joker != Joker::Only && self.ordinary(raw, dora, soul);
        }
        if self.joker == Joker::Exclude || self.red || self.dora || self.soul {
            return false;
        }
        self.joker_face().is_some()
    }

    fn joker_face(&self) -> Option<String> {
        // A joker can supply a face, but cannot acquire a physical red/dora/soul property.
        if self.red || self.dora || self.soul {
            return None;
        }
        (0..34)
            .map(|t| format!("{}{}", t % 9 + 1, ['m', 'p', 's', 'z'][t / 9]))
            .find(|f| self.ordinary(f, &[false; 34], &[false; 34]))
    }
}

struct Choices {
    group: usize,
    values: Vec<Vec<usize>>,
}

struct Search<'a> {
    state: &'a Value,
    groups: &'a [Group],
    entries: &'a [Entry],
    exclude_invalid_shapes: bool,
    emit: &'a mut dyn FnMut(Value),
    stopped: &'a dyn Fn() -> bool,
    started: Instant,
    last_emit: Instant,
    nodes: usize,
    prefix: usize,
    aborted: Option<&'static str>,
}

impl Search<'_> {
    fn pulse(&mut self) -> bool {
        self.nodes += 1;
        if (self.stopped)() {
            self.aborted = Some("stopped-by-user");
        }
        if self.nodes > 500_000 || self.started.elapsed() > Duration::from_secs(10) {
            self.aborted.get_or_insert("custom-search-limit");
        }
        if self.last_emit.elapsed() >= Duration::from_millis(150) {
            (self.emit)(json!({"phase":"custom_searching", "nodes":self.nodes,
                "wall_prefix":self.prefix,"elapsed_ms":self.started.elapsed().as_millis()}));
            self.last_emit = Instant::now();
        }
        self.aborted.is_none()
    }

    fn candidates(&mut self, group: usize, domain: &[usize]) -> Vec<Vec<usize>> {
        let group = self.groups[group].clone();
        let kind = group.kind();
        if kind == Kind::Single { return domain.iter().map(|&i| vec![i]).collect(); }
        let mut by_face: [Vec<usize>; 34] = std::array::from_fn(|_| Vec::new());
        let mut joker = None;
        for &i in domain {
            if let Some(tile) = souzu::tile(&self.entries[i].face) {
                by_face[tile as usize].push(i);
            } else if !group.is_quad() {
                joker = Some(i);
            }
        }
        fn expand(search: &mut Search<'_>, pattern: &[u8], by_face: &[Vec<usize>; 34], joker: Option<usize>,
            rule: &Rule, chosen: &mut Vec<usize>, values: &mut Vec<Vec<usize>>) {
            if !search.pulse() { return; }
            let slot = chosen.len();
            if slot == pattern.len() {
                let mut candidate = chosen.clone();
                candidate.sort_unstable();
                values.push(candidate);
                return;
            }
            let tile = pattern[slot];
            let face = format!("{}{}", tile % 9 + 1, ['m', 'p', 's', 'z'][(tile / 9) as usize]);
            for &i in by_face[tile as usize].iter().chain(joker.iter()) {
                if chosen.contains(&i) || (slot > 0 && pattern[slot - 1] == tile && i <= chosen[slot - 1])
                    || (Some(i) == joker && !rule.ordinary(&face, &[false; 34], &[false; 34])) {
                    continue;
                }
                chosen.push(i);
                expand(search, pattern, by_face, joker, rule, chosen, values);
                chosen.pop();
                if search.aborted.is_some() { return; }
            }
        }
        let mut values = Vec::new();
        for tile in 0..34u8 {
            if matches!(kind, Kind::Pair | Kind::Triplet | Kind::Meld | Kind::Quad) {
                expand(self, &vec![tile; kind.size()], &by_face, joker, &group.rule, &mut Vec::new(), &mut values);
            }
            if matches!(kind, Kind::Sequence | Kind::Meld) && tile < 27 && tile % 9 <= 6 {
                expand(self, &[tile, tile + 1, tile + 2], &by_face, joker, &group.rule, &mut Vec::new(), &mut values);
            }
            if self.aborted.is_some() { break; }
        }
        values.sort();
        values.dedup();
        values
    }

    fn groups_match(&self, selected: &[Vec<usize>], joker_as: Option<u8>) -> bool {
        if joker_as.is_none() && self.groups.iter().all(|group| group.suit_group.trim().is_empty() && group.distinct_group.trim().is_empty()) {
            return true;
        }
        let faces = selected.iter().map(|tiles| tiles.iter().filter_map(|&i|
            souzu::tile(&self.entries[i].face).or(joker_as)
        ).collect::<Vec<_>>()).collect::<Vec<_>>();
        for (g, tiles) in selected.iter().enumerate().filter(|(_, tiles)| !tiles.is_empty()) {
            let group = &self.groups[g];
            if faces[g].len() == tiles.len() && !group.kind().accepts(&faces[g]) { return false; }
            for other in 0..g {
                let linked = &self.groups[other];
                if !group.suit_group.trim().is_empty() && group.suit_group.trim() == linked.suit_group.trim()
                    && faces[g].first().zip(faces[other].first()).is_some_and(|(a, b)| a / 9 != b / 9) {
                    return false;
                }
                if !group.distinct_group.trim().is_empty() && group.distinct_group.trim() == linked.distinct_group.trim()
                    && faces[g].iter().any(|face| faces[other].contains(face)) {
                    return false;
                }
            }
        }
        true
    }

    fn visit(
        &mut self,
        choices: &[Choices],
        depth: usize,
        selected: &mut Vec<Vec<usize>>,
        used: &mut HashSet<usize>,
        nonwall: usize,
    ) -> Option<Value> {
        if !self.pulse() {
            return None;
        }
        if depth == choices.len() {
            let hand = ids(self.state.get("hand_tiles"));
            if self
                .entries
                .iter()
                .enumerate()
                .any(|(i, e)| e.source == "hand" && e.face == "bd" && !used.contains(&i))
            {
                return None;
            }
            let target = selected
                .iter()
                .flatten()
                .map(|&i| self.entries[i].id)
                .collect::<Vec<_>>();
            let quads = self
                .groups
                .iter()
                .zip(selected.iter())
                .filter(|(g, _)| g.is_quad())
                .map(|(_, selected)| {
                    selected
                        .iter()
                        .map(|&i| self.entries[i].id)
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
            let joker_group = selected.iter().position(|tiles| tiles.iter().any(|&i| self.entries[i].face == "bd"));
            let mut joker_as = None;
            if self.exclude_invalid_shapes || joker_group.is_some() {
                let mut counts = [0u8; 34];
                for (_, tiles) in self.groups.iter().zip(selected.iter()).filter(|(group, _)| !group.is_quad()) {
                    for &i in tiles {
                        if let Some(tile) = souzu::tile(&self.entries[i].face) {
                            counts[tile as usize] += 1;
                        }
                    }
                }
                if let Some(g) = joker_group {
                    joker_as = (0..34).find_map(|tile| {
                        let face = format!("{}{}", tile % 9 + 1, ['m', 'p', 's', 'z'][tile / 9]);
                        if !self.groups[g].rule.ordinary(&face, &[false; 34], &[false; 34]) { return None; }
                        counts[tile] += 1;
                        let valid = self.groups_match(selected, Some(tile as u8))
                            && (!self.exclude_invalid_shapes || legal_shape(&counts, quads.len()));
                        counts[tile] -= 1;
                        valid.then_some(face)
                    });
                    if joker_as.is_none() { return None; }
                } else if !legal_shape(&counts, quads.len()) {
                    return None;
                }
            }
            let (steps, discards, incoming, draws) =
                simulate(self.state, self.entries, &target, &quads, self.prefix)?;
            let resolved = selected.iter().enumerate().map(|(g, selected)| json!({
                "quad":self.groups[g].is_quad(),"kind":self.groups[g].kind(),
                "tiles":selected.iter().map(|&i| json!({"id":self.entries[i].id,
                    "face":face(&deck(self.state),self.entries[i].id),
                    "joker_as":if self.entries[i].face=="bd" {joker_as.clone()}else{None},
                })).collect::<Vec<_>>()
            })).collect::<Vec<_>>();
            return Some(
                json!({"status":"plan","mode":"custom-target","search_algorithm":"custom_target",
                "draws_needed":draws,"target_physical_ids":target,"resolved_groups":resolved,"custom_steps":steps,
                "target_physical_faces":target.iter().map(|&id|face(&deck(self.state),id).to_owned()).collect::<Vec<_>>(),
                "switch_discards":discards,"switch_in":incoming,"initial_hand":hand,
                "target_groups":self.groups,"search_nodes":self.nodes}),
            );
        }
        let choices_at = &choices[depth];
        let group = choices_at.group;
        let previous = choices[..depth]
            .iter()
            .rev()
            .find(|c| self.groups[c.group] == self.groups[group])
            .map(|c| c.group);
        for candidate in &choices_at.values {
            if self.aborted.is_some() {
                break;
            }
            if candidate.iter().any(|i| used.contains(i))
                || previous.is_some_and(|g| candidate <= &selected[g])
            {
                continue;
            }
            let next_nonwall = nonwall
                + candidate
                    .iter()
                    .filter(|&&i| self.entries[i].source != "wall")
                    .count();
            if next_nonwall > self.state["hand_tiles"].as_array().unwrap().len() {
                continue;
            }
            selected[group] = candidate.clone();
            if !self.groups_match(selected, None) {
                selected[group].clear();
                continue;
            }
            used.extend(candidate);
            if let Some(plan) = self.visit(choices, depth + 1, selected, used, next_nonwall) {
                return Some(plan);
            }
            for i in candidate {
                used.remove(i);
            }
            selected[group].clear();
        }
        None
    }
}

fn legal_shape(counts: &[u8; 34], quads: usize) -> bool {
    if quads == 0 {
        if counts.iter().filter(|&&count| count == 2).count() == 7
            && counts.iter().all(|&count| count == 0 || count == 2) {
            return true;
        }
        let orphans = [0, 8, 9, 17, 18, 26, 27, 28, 29, 30, 31, 32, 33];
        if orphans.iter().all(|&tile| (1..=2).contains(&counts[tile]))
            && orphans.iter().filter(|&&tile| counts[tile] == 2).count() == 1
            && counts.iter().enumerate().all(|(tile, &count)| count == 0 || orphans.contains(&tile)) {
            return true;
        }
    }
    let faces = counts.iter().enumerate().flat_map(|(tile, &count)|
        vec![format!("{}{}", tile % 9 + 1, ['m', 'p', 's', 'z'][tile / 9]); count as usize]
    ).collect::<Vec<_>>();
    super::can_form_melds_and_pair(&faces, 4 - quads)
}

// A necessary matching check prunes overlapping ranges before enumerating physical assignments.
fn can_match(domains: &[Vec<usize>], size: usize) -> bool {
    fn augment(
        slot: usize,
        domains: &[Vec<usize>],
        seen: &mut [bool],
        owner: &mut [Option<usize>],
    ) -> bool {
        for &tile in &domains[slot] {
            if seen[tile] {
                continue;
            }
            seen[tile] = true;
            if owner[tile].is_none_or(|previous| augment(previous, domains, seen, owner)) {
                owner[tile] = Some(slot);
                return true;
            }
        }
        false
    }
    let mut owner = vec![None; size];
    (0..domains.len()).all(|slot| augment(slot, domains, &mut vec![false; size], &mut owner))
}

type Simulation = (Vec<Value>, Vec<Vec<u64>>, Vec<Vec<u64>>, usize);

fn simulate(
    state: &Value,
    entries: &[Entry],
    target: &[u64],
    quads: &[Vec<u64>],
    wall_limit: usize,
) -> Option<Simulation> {
    let deck = deck(state);
    let mut hand = ids(state.get("hand_tiles"));
    let essential = entries
        .iter()
        .filter(|e| e.source != "wall" && target.contains(&e.id))
        .map(|e| e.id)
        .collect::<HashSet<_>>();
    let replacement = entries
        .iter()
        .filter(|e| e.source == "replacement")
        .map(|e| e.id)
        .collect::<Vec<_>>();
    let wall = entries
        .iter()
        .filter(|e| e.source == "wall")
        .take(wall_limit)
        .map(|e| e.id)
        .collect::<Vec<_>>();
    let (rounds, limit) = search_limits(state);
    let (mut cursor, mut steps, mut out, mut incoming) = (0, Vec::new(), Vec::new(), Vec::new());
    for _ in 0..rounds {
        if essential.iter().all(|id| hand.contains(id)) {
            break;
        }
        let last = replacement
            .iter()
            .rposition(|id| essential.contains(id) && !hand.contains(id))?;
        let discardable = hand
            .iter()
            .copied()
            .filter(|id| !essential.contains(id) && face(&deck, *id) != "bd")
            .collect::<Vec<_>>();
        let size = limit
            .min(discardable.len())
            .min(last.checked_add(1)?.checked_sub(cursor)?);
        if size == 0 {
            return None;
        }
        let discards = discardable[..size].to_vec();
        let draws = replacement.get(cursor..cursor + size)?.to_vec();
        hand.retain(|id| !discards.contains(id));
        hand.extend(&draws);
        cursor += size;
        steps.push(json!({"kind":"exchange","out":discards,"tiles":draws}));
        out.push(discards);
        incoming.push(draws);
    }
    if !essential.iter().all(|id| hand.contains(id)) {
        return None;
    }
    let mut opened = Vec::<u64>::new();
    let mut pending = quads.to_vec();
    let mut draws = 0;
    loop {
        let effective = hand.len() + opened.len() / 4 * 3;
        if effective == 14 {
            if let Some(i) = pending
                .iter()
                .position(|quad| quad.iter().all(|id| hand.contains(id)))
            {
                let quad = pending.remove(i);
                hand.retain(|id| !quad.contains(id));
                opened.extend(&quad);
                steps.push(json!({"kind":"kan","tiles":quad}));
                continue;
            }
            if pending.is_empty()
                && target
                    .iter()
                    .all(|id| hand.contains(id) || opened.contains(id))
            {
                return Some((steps, out, incoming, draws));
            }
            let discard = hand
                .iter()
                .copied()
                .find(|id| !target.contains(id) && face(&deck, *id) != "bd")?;
            hand.retain(|id| *id != discard);
            steps.push(json!({"kind":"discard","tiles":[discard]}));
        } else if effective != 13 {
            return None;
        }
        let draw = *wall.get(draws)?;
        hand.push(draw);
        draws += 1;
        steps.push(json!({"kind":"draw","tiles":[draw]}));
    }
}

pub(super) fn search(
    state: &Value,
    target: &Value,
    wall_limit: usize,
    exclude_invalid_shapes: bool,
    emit: &mut dyn FnMut(Value),
    stopped: &dyn Fn() -> bool,
) -> Value {
    let fail =
        |reason| json!({"status":"impossible","reason":reason,"search_algorithm":"custom_target"});
    if target
        .as_array()
        .is_none_or(|groups| groups.is_empty() || groups.len() > 14)
    {
        return fail("custom-invalid-target");
    }
    let Ok(groups) = serde_json::from_value::<Vec<Group>>(target.clone()) else {
        return fail("custom-invalid-target");
    };
    if groups.is_empty()
        || groups.len() > 14
        || groups.iter().filter(|g| g.is_quad()).count() > 4
        || groups
            .iter()
            .map(|g| if g.is_quad() { 3 } else { g.kind().size() })
            .sum::<usize>()
            != 14
        || groups
            .iter()
            .any(|g| !g.rule.valid() || (g.is_quad() && g.rule.joker == Joker::Only)
                || (g.quad && !matches!(g.kind, Kind::Single | Kind::Quad))
                || g.suit_group.chars().count() > 32 || g.distinct_group.chars().count() > 32)
    {
        return fail("custom-invalid-target");
    }
    let hand = ids(state.get("hand_tiles"));
    if !(13..=14).contains(&hand.len()) || state["ming"].as_array().is_some_and(|m| !m.is_empty()) {
        return fail("custom-hand-size");
    }
    if wall_limit > 36 {
        return fail("custom-wall-limit");
    }
    let mut state = state.clone();
    if state["next_operation"]
        .as_array()
        .is_some_and(|ops| !ops.iter().any(|op| op["type"] == 101))
    {
        state["total_change_tile_count"] = json!(0);
    }
    let (rounds, _) = search_limits(&state);
    state["total_change_tile_count"] = json!(rounds.min(256));
    state["change_tile_count"] = json!(0);
    let entries = pool(&state, wall_limit);
    let mut unique = HashSet::new();
    if entries.len() > 256
        || entries
            .iter()
            .any(|e| !unique.insert(e.id) || (e.face != "bd" && souzu::tile(&e.face).is_none()))
    {
        return fail("invalid-search-tiles");
    }
    if entries.iter().filter(|e| e.face == "bd").count() > 1
        || entries.iter().any(|e| e.face == "bd" && e.source != "hand")
    {
        return fail("custom-joker-state");
    }
    let deck = deck(&state);
    let dora = souzu::preferred_tiles(
        &state,
        &SearchPreferences {
            prefer_dora: true,
            ..Default::default()
        },
        &[],
    );
    let soul = souzu::preferred_tiles(
        &state,
        &SearchPreferences {
            prefer_soul: true,
            ..Default::default()
        },
        &[],
    );
    let now = Instant::now();
    let mut search = Search {
        state: &state,
        groups: &groups,
        entries: &entries,
        exclude_invalid_shapes,
        emit,
        stopped,
        started: now,
        last_emit: now,
        nodes: 0,
        prefix: 0,
        aborted: None,
    };
    let minimum_draws = (14 + groups.iter().filter(|g| g.is_quad()).count()).saturating_sub(hand.len());
    for prefix in minimum_draws..=entries.iter().filter(|e| e.source == "wall").count() {
        search.prefix = prefix;
        if !search.pulse() {
            break;
        }
        let domains = groups
            .iter()
            .map(|g| {
                entries
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| {
                        (e.source != "wall" || e.index < prefix)
                            && (!g.is_quad() || e.face != "bd")
                            && g.rule.matches(face(&deck, e.id), &dora, &soul)
                    })
                    .map(|(i, _)| i)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let slots = groups
            .iter()
            .zip(&domains)
            .flat_map(|(g, d)| std::iter::repeat_n(d.clone(), g.kind().size()))
            .collect::<Vec<_>>();
        if !can_match(&slots, entries.len()) {
            continue;
        }
        let mut choices = Vec::new();
        for (group, domain) in domains.iter().enumerate() {
            let values = search.candidates(group, domain);
            if let Some(reason) = search.aborted { return fail(reason); }
            choices.push(Choices { group, values });
        }
        choices.sort_by_key(|c| c.values.len());
        if let Some(plan) = search.visit(
            &choices,
            0,
            &mut vec![Vec::new(); groups.len()],
            &mut HashSet::new(),
            0,
        ) {
            return plan;
        }
        if search.aborted.is_some() {
            break;
        }
    }
    fail(search.aborted.unwrap_or("custom-not-reachable"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(hand: &[&str], replacement: &[&str], wall: &[&str], rounds: usize) -> Value {
        let deck = hand
            .iter()
            .chain(replacement)
            .chain(wall)
            .enumerate()
            .map(|(i, face)| ((i + 1).to_string(), json!(face)))
            .collect::<serde_json::Map<_, _>>();
        json!({"deck_map":deck,"hand_tiles":(1..=hand.len()).collect::<Vec<_>>(),
            "replacement_tiles":(hand.len()+1..=hand.len()+replacement.len()).collect::<Vec<_>>(),
            "wall_tiles":(hand.len()+replacement.len()+1..=hand.len()+replacement.len()+wall.len()).collect::<Vec<_>>(),
            "total_change_tile_count":rounds,"stage":4})
    }
    fn exact(faces: &[&str]) -> Value {
        json!(faces
            .iter()
            .map(|face| json!({"rule":{"faces":[face],"joker":"exclude"}}))
            .collect::<Vec<_>>())
    }
    fn run(state: &Value, target: &Value) -> Value {
        search(state, target, 36, false, &mut |_| {}, &|| false)
    }

    #[test]
    fn configurable_pairs_require_distinct_faces_only_when_linked() {
        let hand = ["1m","1m","1m","1m","3m","3m","5p","0p","7p","7p","2s","2s","4s","4s"];
        let pairs = json!(vec![json!({"kind":"pair","distinct_group":"A","rule":{}}); 7]);
        let missing = state(&hand, &[], &[], 0);
        assert_eq!(run(&missing, &pairs)["reason"], "custom-not-reachable");
        let mut unlinked = pairs.clone();
        for group in unlinked.as_array_mut().unwrap() { group["distinct_group"] = json!(""); }
        replay(&missing, &run(&missing, &unlinked));
        let reachable = state(&hand, &["7z", "7z"], &[], 1);
        let plan = run(&reachable, &pairs);
        replay(&reachable, &plan);
        let faces = plan["resolved_groups"].as_array().unwrap().iter()
            .map(|group| norm(group["tiles"][0]["face"].as_str().unwrap())).collect::<HashSet<_>>();
        assert_eq!(faces.len(), 7);
        assert_eq!(plan["draws_needed"], 0);
    }

    #[test]
    fn configurable_sequences_share_suits_and_keep_per_group_ranges() {
        let hand = ["1m","2m","3m","4p","5p","6p","7s","8s","9s","2z","2z","2z","7z","7z"];
        let mut groups = [[1,2,3],[4,5,6],[7,8,9]].iter().map(|ranks|
            json!({"kind":"sequence","suit_group":"A","rule":{"ranks":ranks}})).collect::<Vec<_>>();
        groups.push(json!({"kind":"meld","rule":{"faces":["2z"]}}));
        groups.push(json!({"kind":"pair","rule":{"faces":["7z"]}}));
        let target = json!(groups);
        let missing = state(&hand, &[], &[], 0);
        assert_eq!(run(&missing, &target)["reason"], "custom-not-reachable");
        let reachable = state(&hand, &["4m","0m","6m","7m","8m","9m"], &[], 1);
        let plan = search(&reachable, &target, 36, true, &mut |_| {}, &|| false);
        replay(&reachable, &plan);
        for group in plan["resolved_groups"].as_array().unwrap().iter().take(3) {
            assert_eq!(group["kind"], "sequence");
            assert!(group["tiles"].as_array().unwrap().iter().all(|tile| tile["face"].as_str().unwrap().ends_with('m')));
        }
        assert!(!Kind::Sequence.accepts(&[7, 8, 9]));
        assert!(!Kind::Sequence.accepts(&[27, 28, 29]));
    }

    #[test]
    fn structured_groups_resolve_fixed_jokers_and_declared_quads() {
        let hand = ["bd","9m","3m","3m","5p","0p","7p","7p","2s","2s","4s","4s","7z","7z"];
        let pairs = json!(vec![json!({"kind":"pair","distinct_group":"A","rule":{}}); 7]);
        let s = state(&hand, &[], &[], 0);
        let plan = run(&s, &pairs);
        replay(&s, &plan);
        let joker = plan["resolved_groups"].as_array().unwrap().iter().flat_map(|g| g["tiles"].as_array().unwrap())
            .find(|tile| tile["face"] == "bd").unwrap();
        assert_eq!(joker["joker_as"], "9m");
        let hand = ["1m","1m","1m","1m","2p","3p","4p","5s","6s","7s","2z","2z","2z"];
        let s = state(&hand, &[], &["7z","7z"], 0);
        let target = json!([
            {"kind":"quad","rule":{"faces":["1m"]}},
            {"kind":"meld","rule":{"faces":["2p","3p","4p"]}},
            {"kind":"sequence","rule":{"ranks":[5,6,7]}},
            {"kind":"triplet","rule":{"faces":["2z"]}},
            {"kind":"pair","rule":{"faces":["7z"]}}
        ]);
        let plan = search(&s, &target, 36, true, &mut |_| {}, &|| false);
        replay(&s, &plan);
        assert_eq!(plan["resolved_groups"][0]["quad"], true);
        assert!(plan["custom_steps"].as_array().unwrap().iter().any(|step| step["kind"] == "kan"));
    }

    #[test]
    fn malformed_group_constraints_are_rejected() {
        let s = state(&["1m"; 14], &[], &[], 0);
        for invalid in [json!({"kind":"seven_pairs","rule":{}}), json!({"kind":"pair","quad":true,"rule":{}}),
            json!({"suit_group":"A".repeat(33),"rule":{}}), json!({"distinct_group":42,"rule":{}})] {
            let mut target = vec![json!({"rule":{}}); 14];
            target[0] = invalid;
            assert_eq!(run(&s, &json!(target))["reason"], "custom-invalid-target");
        }
    }

    #[test]
    fn legal_shape_filter_accepts_three_forms_and_rejects_nonwinning_tiles() {
        let cases = [
            (["1m","2m","3m","4m","0m","6m","7p","8p","9p","2s","2s","2s","7z","7z"], true),
            (["1m","1m","3m","3m","5p","0p","7p","7p","2s","2s","4s","4s","7z","7z"], true),
            (["1m","9m","1p","9p","1s","9s","1z","2z","3z","4z","5z","6z","7z","7z"], true),
            (["1m","2m","3m","4m","5m","6m","7m","8m","9m","1p","2p","3p","4p","5p"], false),
            (["1m","2m","3m","1p","2p","3p","1s","2s","3s","1z","2z","3z","7z","7z"], false),
            (["8m","9m","1p","2p","3p","4p","5s","6s","7s","1z","1z","1z","7z","7z"], false),
            (["1z","1z","1z","1z","2z","2z","3z","3z","4z","4z","5z","5z","6z","6z"], false),
        ];
        for (hand, valid) in cases {
            let s = state(&hand, &[], &[], 0);
            let target = exact(&hand);
            assert_eq!(run(&s, &target)["status"], "plan");
            let filtered = search(&s, &target, 36, true, &mut |_| {}, &|| false);
            assert_eq!(filtered["status"] == "plan", valid, "{hand:?}: {filtered}");
        }
    }

    #[test]
    fn legal_shape_filter_counts_declared_quads_as_melds() {
        for quads in 0..=4 {
            let mut faces = (1..=quads).flat_map(|rank| vec![format!("{rank}m"); 4]).collect::<Vec<_>>();
            faces.extend((0..4-quads).flat_map(|_| ["5p", "6p", "7p"].map(str::to_owned)));
            faces.extend(["7z".to_owned(), "7z".to_owned()]);
            let refs = faces.iter().map(String::as_str).collect::<Vec<_>>();
            let s = state(&refs[..13], &[], &refs[13..], 0);
            let mut target = (1..=quads).map(|rank| json!({"quad":true,"rule":{"faces":[format!("{rank}m")]}})).collect::<Vec<_>>();
            target.extend(refs[quads*4..].iter().map(|face| json!({"rule":{"faces":[face],"joker":"exclude"}})));
            let plan = search(&s, &json!(target), 36, true, &mut |_| {}, &|| false);
            replay(&s, &plan);
            if quads == 4 {
                let last = faces.len() - 1;
                faces[last] = "6z".into();
                let refs = faces.iter().map(String::as_str).collect::<Vec<_>>();
                let s = state(&refs[..13], &[], &refs[13..], 0);
                target.last_mut().unwrap()["rule"]["faces"] = json!(["6z"]);
                assert_eq!(search(&s, &json!(target), 36, true, &mut |_| {}, &|| false)["reason"], "custom-not-reachable");
            }
        }
    }

    #[test]
    fn legal_shape_filter_tries_allowed_joker_faces_and_continues_searching() {
        let hand = ["bd","1m","2m","3m","4m","0m","6m","7p","8p","9p","2s","2s","2s","7z"];
        let s = state(&hand, &[], &[], 0);
        let mut target = exact(&hand);
        target[0]["rule"] = json!({"faces":["1m","7z"],"joker":"allow"});
        assert_eq!(run(&s, &target)["resolved_groups"][0]["tiles"][0]["joker_as"], "1m");
        let filtered = search(&s, &target, 36, true, &mut |_| {}, &|| false);
        replay(&s, &filtered);
        assert_eq!(filtered["resolved_groups"][0]["tiles"][0]["joker_as"], "7z");
        target[0]["rule"]["faces"] = json!(["1m"]);
        assert_eq!(search(&s, &target, 36, true, &mut |_| {}, &|| false)["reason"], "custom-not-reachable");
        let hand = ["1m","2m","3m","4m","5m","6m","7p","8p","9p","2s","2s","2s","7z"];
        let s = state(&hand, &[], &["6z","7z"], 0);
        let mut target = exact(&hand);
        target.as_array_mut().unwrap().push(json!({"rule":{"suits":["z"]}}));
        assert_eq!(run(&s, &target)["draws_needed"], 1);
        let filtered = search(&s, &target, 36, true, &mut |_| {}, &|| false);
        replay(&s, &filtered);
        assert_eq!(filtered["draws_needed"], 2);
        assert!(filtered["custom_steps"].as_array().unwrap().iter().any(|step| step["kind"] == "discard" && step["tiles"] == json!([14])));
    }
    fn replay(state: &Value, plan: &Value) {
        assert_eq!(plan["status"], "plan", "{plan}");
        let deck = deck(state);
        let mut hand = ids(state.get("hand_tiles"));
        let replacement = ids(state.get("replacement_tiles"));
        let wall = ids(state.get("wall_tiles"));
        let (mut cursor, mut drawn, mut rounds, mut opened) =
            (ids(state.get("switch_used_tiles")).len(), 0, 0, Vec::new());
        for step in plan["custom_steps"].as_array().unwrap() {
            let tiles = ids(step.get("tiles"));
            match step["kind"].as_str().unwrap() {
                "exchange" => {
                    let out = ids(step.get("out"));
                    assert!(out.len() <= search_limits(state).1 && !out.is_empty());
                    assert!(out
                        .iter()
                        .all(|id| hand.contains(id) && face(&deck, *id) != "bd"));
                    assert_eq!(tiles, replacement[cursor..cursor + out.len()]);
                    cursor += out.len();
                    rounds += 1;
                    hand.retain(|id| !out.contains(id));
                    hand.extend(tiles);
                }
                "draw" => {
                    assert_eq!(hand.len() + opened.len() / 4 * 3, 13);
                    assert_eq!(tiles, vec![wall[drawn]]);
                    drawn += 1;
                    hand.extend(tiles);
                }
                "discard" => {
                    assert_eq!(hand.len() + opened.len() / 4 * 3, 14);
                    assert_eq!(tiles.len(), 1);
                    assert!(hand.contains(&tiles[0]) && face(&deck, tiles[0]) != "bd");
                    hand.retain(|id| !tiles.contains(id));
                }
                "kan" => {
                    assert_eq!(hand.len() + opened.len() / 4 * 3, 14);
                    assert_eq!(tiles.len(), 4);
                    assert!(tiles.iter().all(|id| hand.contains(id)
                        && norm(face(&deck, *id)) == norm(face(&deck, tiles[0]))));
                    hand.retain(|id| !tiles.contains(id));
                    opened.extend(tiles);
                }
                _ => panic!("invalid step"),
            }
        }
        assert!(rounds <= search_limits(state).0);
        assert_eq!(drawn as u64, plan["draws_needed"]);
        assert_eq!(hand.len() + opened.len() / 4 * 3, 14);
        hand.extend(opened);
        hand.sort();
        let mut target = ids(plan.get("target_physical_ids"));
        target.sort();
        assert_eq!(hand, target);
    }

    #[test]
    fn arbitrary_ranges_choose_distinct_tiles_and_preserve_red_properties() {
        let hand = [
            "1m", "2m", "3m", "4m", "0m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "4p",
        ];
        let mut s = state(&hand, &[], &["5p"], 0);
        s["dora_tiles"] = json!([4]); // 4m indicator -> both normal and red 5m.
        s["tian_dora_tiles"] = json!(["5m"]);
        let mut target = exact(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "4p", "5p",
        ]);
        target[0]["rule"] = json!({"suits":["m"],"ranks":[1,2]});
        target[4]["rule"] = json!({"red":true,"dora":true,"soul":true});
        let plan = run(&s, &target);
        replay(&s, &plan);
        assert_eq!(plan["resolved_groups"][4]["tiles"][0]["face"], "0m");
        target[1]["rule"] = json!({"faces":["1m"],"joker":"exclude"});
        replay(&s, &run(&s, &target)); // The first range must use 2m, not the unique 1m.
        target[4]["rule"]["suits"] = json!(["z"]);
        assert_eq!(run(&s, &target)["reason"], "custom-not-reachable");
    }

    #[test]
    fn joker_substitutes_faces_but_never_bonus_tiles_or_discards() {
        let hand = [
            "bd", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "4p",
        ];
        let s = state(&hand, &["1m"], &["5p"], 1);
        let mut target = exact(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "4p", "5p",
        ]);
        target[0]["rule"]["joker"] = json!("allow");
        let plan = run(&s, &target);
        replay(&s, &plan);
        assert_eq!(plan["resolved_groups"][0]["tiles"][0]["joker_as"], "1m");
        target[0]["rule"]["dora"] = json!(true);
        assert_eq!(run(&s, &target)["reason"], "custom-not-reachable");
        target[0]["rule"] = json!({"joker":"only"});
        replay(&s, &run(&s, &target));
        target[0]["rule"] = json!({"faces":["1m"],"joker":"exclude"});
        assert_eq!(run(&s, &target)["reason"], "custom-not-reachable");
    }

    #[test]
    fn one_fixed_joker_survives_exchanges_discards_and_kan() {
        let hand = [
            "bd", "1m", "1m", "1m", "1m", "2p", "3p", "4p", "5p", "6p", "7p", "8p", "9p", "7z",
        ];
        let s = state(&hand, &["2z"], &["6z", "3z"], 1);
        let mut target = exact(&[
            "2p", "3p", "4p", "5p", "6p", "7p", "8p", "9p", "1z", "2z", "3z",
        ]);
        target[8]["rule"].as_object_mut().unwrap().remove("joker");
        target
            .as_array_mut()
            .unwrap()
            .push(json!({"quad":true,"rule":{"faces":["1m"]}}));
        let plan = run(&s, &target);
        replay(&s, &plan);
        assert_eq!(plan["resolved_groups"][8]["tiles"][0]["id"], 1);
        assert_eq!(plan["resolved_groups"][8]["tiles"][0]["joker_as"], "1z");
        let steps = plan["custom_steps"].as_array().unwrap();
        for kind in ["exchange", "draw", "discard", "kan"] {
            assert!(steps.iter().any(|step| step["kind"] == kind));
        }
        for step in steps {
            assert!(!ids(step.get("out")).contains(&1));
            assert!(!ids(step.get("tiles")).contains(&1));
        }
        assert_eq!(
            plan["target_physical_faces"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|f| *f == "bd")
                .count(),
            1
        );

        let mut no_joker = s.clone();
        no_joker["deck_map"]["1"] = json!("1z");
        let plan = run(&no_joker, &target);
        replay(&no_joker, &plan);
        assert!(!plan["target_physical_faces"]
            .as_array()
            .unwrap()
            .contains(&json!("bd")));
    }

    #[test]
    fn rejects_multiple_jokers_and_jokers_from_exchange_or_wall() {
        let target = json!(vec![json!({"rule":{}}); 14]);
        let mut multiple = state(&["1m"; 14], &[], &[], 0);
        multiple["deck_map"]["1"] = json!("bd");
        multiple["deck_map"]["2"] = json!("bd");
        for invalid in [
            multiple,
            state(&["1m"; 14], &["bd"], &[], 1),
            state(&["1m"; 13], &[], &["bd"], 0),
        ] {
            assert_eq!(run(&invalid, &target)["reason"], "custom-joker-state");
        }
    }

    #[test]
    fn exchanges_follow_prefix_round_limits_and_used_tiles() {
        let hand = [
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "7z", "6z",
        ];
        let mut s = state(&hand, &["7z", "6z", "5z", "3p", "4p"], &["5p"], 3);
        s["switch_used_tiles"] = json!([14]);
        s["boss_buff"] = json!([901]);
        let target = exact(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "4p", "5p",
        ]);
        let plan = run(&s, &target);
        replay(&s, &plan);
        assert_eq!(plan["switch_discards"].as_array().unwrap().len(), 2);
        s["total_change_tile_count"] = json!(1);
        assert_eq!(run(&s, &target)["reason"], "custom-not-reachable");
        s["total_change_tile_count"] = json!(3);
        s["next_operation"] = json!([{"type":1}]);
        assert_eq!(run(&s, &target)["reason"], "custom-not-reachable");
    }

    #[test]
    fn quads_use_four_physical_tiles_and_simulate_replacement_draws() {
        let hand = [
            "1m", "1m", "1m", "1m", "2p", "2p", "2p", "2p", "3s", "4s", "5s", "6z", "7z",
        ];
        let s = state(&hand, &[], &["7z", "6z", "6z"], 0);
        let mut target = exact(&["3s", "4s", "5s", "6z", "6z", "6z", "7z", "7z"]);
        target.as_array_mut().unwrap().extend([
            json!({"quad":true,"rule":{"faces":["1m"]}}),
            json!({"quad":true,"rule":{"faces":["2p"]}}),
        ]);
        let plan = run(&s, &target);
        replay(&s, &plan);
        assert_eq!(plan["target_physical_ids"].as_array().unwrap().len(), 16);
        assert_eq!(
            plan["custom_steps"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|s| s["kind"] == "kan")
                .count(),
            2
        );
        assert_eq!(plan["draws_needed"], 3);
    }

    #[test]
    fn later_duplicate_tiles_can_unlock_a_deeper_exchange_and_wall_discards_are_replayed() {
        let hand = [
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "7z",
        ];
        let s = state(&hand, &["1m", "2m", "4p"], &["6z", "5p"], 1);
        let target = exact(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "4p", "5p",
        ]);
        let plan = run(&s, &target);
        replay(&s, &plan);
        assert_eq!(plan["switch_discards"], json!([[1, 2, 13]]));
        assert_eq!(plan["draws_needed"], 2);
        assert!(plan["custom_steps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|step| step["kind"] == "discard" && step["tiles"] == json!([17])));
    }

    #[test]
    fn broad_targets_fast_path_validation_and_cancellation() {
        let hand = ["1m"; 13];
        let s = state(&hand, &["2p"; 26], &["3s"], 2);
        let target = json!(vec![json!({"rule":{}}); 14]);
        let plan = run(&s, &target);
        replay(&s, &plan);
        assert!(plan["search_nodes"].as_u64().unwrap() < 100);
        assert_eq!(
            search(&s, &target, 36, false, &mut |_| {}, &|| true)["reason"],
            "stopped-by-user"
        );
        for invalid in [
            json!([]),
            json!([{"rule":{}}]),
            json!(vec![json!({"rule":{"ranks":[0]}}); 14]),
            json!(vec![json!({"rule":{"suits":["x"]}}); 14]),
            json!(vec![json!({"rule":{"unexpected":true}}); 14]),
        ] {
            assert_eq!(run(&s, &invalid)["reason"], "custom-invalid-target");
        }
        let s = state(&["1m"; 14], &[], &[], 0);
        let plan = run(&s, &target);
        replay(&s, &plan);
        assert_eq!(plan["custom_steps"], json!([]));
    }
}
