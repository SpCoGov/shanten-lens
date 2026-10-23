//! Physical-meld enumeration with reachability checks and preference bounds.
//! Ordinary preferences rank ready hands; coin objectives maximize reward per win times wins.
use super::{
    custom, deck, face, ids, norm, pool, search_limits, souzu, CoinObjective, Entry, MeldType,
    SearchPreferences, SwitchBatches,
};
use serde_json::{json, Map, Value};
use std::{
    cmp::Reverse,
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

#[derive(Debug)]
struct Meld {
    ids: [u64; 3],
    preferred: [usize; 3],
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
    preference_limits: [usize; 3],
    best_preferred: Option<[usize; 3]>,
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

    fn enumerate(
        &mut self,
        entries: &[Entry],
        preferred_ids: &HashSet<u64>,
        preferences: &SearchPreferences,
    ) -> Option<Vec<Meld>> {
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
                    preferred: [
                        usize::from(preferences.preferred_suit.is_some_and(|suit| suit.matches(tile))),
                        usize::from(preferences.preferred_meld_type == Some(if kind == 0 {
                            MeldType::Triplet
                        } else {
                            MeldType::Sequence
                        })),
                        usize::from(ids.iter().any(|id| preferred_ids.contains(id))),
                    ],
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
        result.sort_unstable_by_key(|meld| (Reverse(meld.preferred), meld.key));
        self.preference_limits = std::array::from_fn(|i| {
            result.iter().filter(|meld| meld.preferred[i] != 0).count().min(4)
        });
        Some(result)
    }

    fn visit(
        &mut self,
        start: usize,
        chosen: &mut Vec<u64>,
        preferred: [usize; 3],
        melds: &[Meld],
        seen: &mut HashMap<String, [usize; 3]>,
        check: &mut dyn FnMut(&[u64], String) -> Option<Value>,
    ) -> Option<Value> {
        if (self.stopped)() {
            return Some(stopped());
        }
        self.nodes += 1;
        self.progress(false, false);
        let slots = 4 - chosen.len() / 3;
        // All candidates need zero wall draws; each remaining meld adds at most one match.
        if self.preference_limits != [0; 3]
            && self
                .best_preferred
                .is_some_and(|best| {
                    let upper = std::array::from_fn(|i| (preferred[i] + slots).min(self.preference_limits[i]));
                    upper <= best
                })
        {
            return None;
        }
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
            // The same tiles may form more preferred melds under a different decomposition.
            if seen
                .get(&signature)
                .is_some_and(|&previous| previous >= preferred)
            {
                return None;
            }
            seen.insert(signature.clone(), preferred);
            self.checks += 1;
            let mut plan = check(chosen, signature)?;
            plan["preferred_suit_count"] = json!(preferred[0]);
            plan["preferred_meld_type_count"] = json!(preferred[1]);
            plan["preferred_meld_count"] = json!(preferred[2]);
            self.best_preferred = Some(preferred);
            return Some(plan);
        }
        if melds.len() < slots {
            return None;
        }
        let mut best = None;
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
            let found = self.visit(
                index + 1,
                chosen,
                std::array::from_fn(|i| preferred[i] + melds[index].preferred[i]),
                melds,
                seen,
                check,
            );
            chosen.truncate(chosen.len() - 3);
            if let Some(plan) = found {
                if self.preference_limits == [0; 3] || plan["status"] != "plan" {
                    return Some(plan);
                }
                best = Some(plan);
            }
        }
        best
    }
}

fn stopped() -> Value {
    json!({"status":"impossible","reason":"stopped-by-user"})
}

fn coin_rewards(state: &Value, entries: &[Entry], objective: CoinObjective) -> HashMap<u64, usize> {
    let deck = deck(state);
    let dora = souzu::dora_counts(state);
    let soul = souzu::preferred_tiles(state, &SearchPreferences {
        prefer_soul: true, ..Default::default()
    }, &[]);
    entries.iter().map(|entry| {
        let reward = souzu::tile(&entry.face).map_or(0, |tile| {
            let red = matches!(face(&deck, entry.id), "0m" | "0p" | "0s");
            let hits = dora[tile as usize];
            let soul = usize::from(soul[tile as usize]);
            match objective {
                CoinObjective::HeavenlyBlessing => 2 * (hits + usize::from(red) + soul),
                CoinObjective::ShiningPearl if tile / 9 == 1 && (hits != 0 || red) => 3 + 2 * soul,
                CoinObjective::ShiningPearl => 0,
            }
        });
        (entry.id, reward)
    }).collect()
}

struct CoinSearch<'a, 'p> {
    progress: &'a mut Search<'p>,
    state: &'a Value,
    entries: &'a [Entry],
    melds: Vec<(usize, Meld)>,
    tiles: Vec<(usize, &'a Entry)>,
    seen: HashSet<String>,
    best: Option<Value>,
    best_reward: Option<usize>,
    wall_limit: usize,
    wall_size: usize,
}

impl CoinSearch<'_, '_> {
    fn exchange_prefix(&self, chosen: &[u64]) -> Option<usize> {
        let selected = self.entries.iter().filter(|entry| chosen.contains(&entry.id)).collect::<Vec<_>>();
        // Keeping fewer target tiles gives an optimistic bound on the reachable replacement prefix.
        let mut kept = 1 + selected.iter().filter(|entry| entry.source == "hand").count();
        let size = self.entries.iter().filter(|entry| entry.source == "replacement").count();
        let (rounds, limit) = search_limits(self.state);
        let mut cursor = 0;
        for _ in 0..rounds {
            let end = cursor + limit.min(13 - kept).min(size - cursor);
            if end == cursor { break; }
            kept += selected.iter().filter(|entry| entry.source == "replacement"
                && (cursor..end).contains(&entry.index)).count();
            cursor = end;
        }
        selected.iter().all(|entry| entry.source != "replacement" || entry.index < cursor).then_some(cursor)
    }

    fn visit(&mut self, start: usize, chosen: &mut Vec<u64>, reward: usize) {
        if (self.progress.stopped)() { return; }
        self.progress.nodes += 1;
        self.progress.progress(false, false);
        let slots = 4 - chosen.len() / 3;
        if self.melds.len() - start < slots { return; }
        let Some(prefix) = self.exchange_prefix(chosen) else { return; };
        let available = self.tiles.iter().filter(|(_, tile)| !chosen.contains(&tile.id)
            && (tile.source != "replacement" || tile.index < prefix));
        let tile_upper = reward + available.clone().take(13 - chosen.len()).map(|(coins, _)| coins).sum::<usize>();
        let meld_upper = reward + self.melds[start..].iter().take(slots).map(|(coins, _)| coins).sum::<usize>()
            + available.clone().next().map_or(0, |(coins, _)| *coins);
        // A selected wall tile cannot be won before its draw; include that first win.
        let wins_upper = self.wall_size.saturating_sub(self.entries.iter()
            .filter(|entry| entry.source == "wall" && chosen.contains(&entry.id))
            .map(|entry| entry.index).max().unwrap_or(0));
        if self.best_reward.is_some_and(|best| tile_upper.min(meld_upper) * wins_upper <= best) {
            return;
        }
        if slots == 0 {
            let deck = deck(self.state);
            for (extra_reward, extra) in &self.tiles {
                if (self.progress.stopped)() { return; }
                let coins = reward + extra_reward;
                if self.best_reward.is_some_and(|best| coins * wins_upper <= best) { break; }
                if chosen.contains(&extra.id) || (extra.source == "replacement" && extra.index >= prefix) { continue; }
                let target = candidate_ids(chosen.iter().copied().chain([1000, extra.id]).collect(), &deck);
                if self.entries.iter().filter(|e| e.source != "wall" && target.contains(&e.id)).count() > 13 {
                    continue;
                }
                let mut sorted = target.clone();
                sorted.sort_unstable();
                let signature = format!("wanxiang|coins|{}", sorted.iter().map(u64::to_string).collect::<Vec<_>>().join(","));
                if !self.seen.insert(signature.clone()) { continue; }
                self.progress.checks += 1;
                let Some((steps, discards, incoming, draws)) = custom::simulate(self.state, self.entries, &target, &[], self.wall_limit) else {
                    continue;
                };
                let win_count = self.wall_size.saturating_sub(draws.saturating_sub(1));
                let total_coin_reward = coins * win_count;
                if self.best_reward.is_some_and(|best| total_coin_reward <= best) { continue; }
                let winning = steps.last().unwrap()["tiles"][0].as_u64().unwrap();
                let tenpai_draws = self.entries.iter()
                    .filter(|entry| entry.source == "wall" && entry.id != winning && target.contains(&entry.id))
                    .map(|entry| entry.index + 1).max().unwrap_or(0);
                let pair = [1000, extra.id];
                let groups = chosen.chunks_exact(3).chain(std::iter::once(pair.as_slice())).map(|group| {
                    let mut group = group.to_vec();
                    group.sort_unstable_by_key(|&id| (souzu::tile(face(&deck, id)), id));
                    json!({"quad":false,"tiles":group.iter().map(|&id| json!({"id":id,"face":face(&deck,id)})).collect::<Vec<_>>()})
                }).collect::<Vec<_>>();
                let (rounds, limit) = search_limits(self.state);
                self.best = Some(json!({
                    "status":"plan","mode":"wanxiang-four-meld-switch","search_algorithm":"wanxiang_four_meld_switch",
                    "coin_reward":coins,"win_count":win_count,"total_coin_reward":total_coin_reward,
                    "draws_needed":draws,"tenpai_draws_needed":tenpai_draws,"custom_steps":steps,
                    "initial_hand":ids(self.state.get("hand_tiles")),
                    "target13":target.iter().filter(|&&id| id != winning).map(|&id| norm(face(&deck,id))).collect::<Vec<_>>(),
                    "target14":target.iter().map(|&id| norm(face(&deck,id))).collect::<Vec<_>>(),
                    "target_physical_ids":target,
                    "target_physical_faces":target.iter().map(|&id| face(&deck,id)).collect::<Vec<_>>(),
                    "resolved_groups":groups,
                    "discards":discards.first().cloned().unwrap_or_default(),
                    "switch_batch_sizes":discards.iter().map(Vec::len).collect::<Vec<_>>(),
                    "switch_discards":discards,"switch_in":incoming,
                    "wall_draws":steps.iter().filter(|step| step["kind"] == "draw").map(|step| step["tiles"][0].clone()).collect::<Vec<_>>(),
                    "post_draw_discards":steps.iter().filter(|step| step["kind"] == "discard").map(|step| step["tiles"][0].clone()).collect::<Vec<_>>(),
                    "waits":[face(&deck,winning)],"quad_faces":[],"remaining_changes":rounds,
                    "per_change_limit":limit,"max_change_count":rounds,"considered_tile_count":self.entries.len(),
                    "plan_signature":signature
                }));
                self.best_reward = Some(total_coin_reward);
            }
            return;
        }
        for index in start..=self.melds.len() - slots {
            if (self.progress.stopped)() { return; }
            self.progress.progress(false, false);
            if chosen.is_empty() { self.progress.meld_index = index + 1; }
            let (coins, meld) = &self.melds[index];
            if meld.key.1 > prefix || meld.ids.iter().any(|id| chosen.contains(id)) { continue; }
            let next_reward = reward + coins;
            chosen.extend(meld.ids);
            self.visit(index + 1, chosen, next_reward);
            chosen.truncate(chosen.len() - 3);
        }
    }
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
    wall_limit: usize,
    skip: &[String],
    preferences: &SearchPreferences,
    emit: &mut dyn FnMut(Value),
    should_stop: &dyn Fn() -> bool,
) -> Value {
    if !(1..=36).contains(&wall_limit) {
        return json!({"status":"impossible","reason":"wanxiang-wall-limit"});
    }
    let hand = ids(state.get("hand_tiles"));
    if !hand.contains(&1000) {
        return json!({"status":"impossible","reason":"wanxiang-not-in-hand"});
    }
    let mut entries = pool(state, if preferences.coin_objective.is_some() { wall_limit } else { 0 });
    if preferences.coin_objective.is_some() {
        let (rounds, limit) = search_limits(state);
        // The fixed joker cannot be exchanged, even when the round allows thirteen tiles.
        entries.retain(|entry| entry.source != "replacement" || entry.index < rounds * limit.min(12));
    }
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
        preference_limits: [0; 3],
        best_preferred: None,
    };
    search.progress(true, true);
    if should_stop() {
        return stopped();
    }
    if let Some(objective) = preferences.coin_objective {
        let preferences = SearchPreferences { coin_objective: Some(objective), ..Default::default() };
        let rewards = coin_rewards(state, &entries, objective);
        let Some(melds) = search.enumerate(&entries, &HashSet::new(), &preferences) else { return stopped(); };
        let mut melds = melds.into_iter().map(|meld| (meld.ids.iter().map(|id| rewards[id]).sum::<usize>(), meld)).collect::<Vec<_>>();
        melds.sort_unstable_by_key(|(coins, meld)| (Reverse(*coins), meld.key));
        let mut tiles = entries.iter().filter(|entry| entry.id != 1000 && entry.face != "bd")
            .map(|entry| (rewards[&entry.id], entry)).collect::<Vec<_>>();
        tiles.sort_by_key(|(coins, _)| Reverse(*coins));
        search.progress(false, true);
        let mut coins = CoinSearch { progress: &mut search, state, entries: &entries, melds, tiles,
            seen: skip.iter().cloned().collect(), best: None, best_reward: None, wall_limit,
            wall_size: ids(state.get("wall_tiles")).len() };
        coins.visit(0, &mut Vec::new(), 0);
        let result = coins.best;
        search.progress(false, true);
        if should_stop() { return stopped(); }
        let mut result = result.unwrap_or_else(|| json!({"status":"impossible","reason":"no-reliable-plan-found"}));
        result["search_preferences"] = json!(preferences);
        return result;
    }
    let deck = deck(state);
    let preferred_faces = souzu::preferred_tiles(state, preferences, &[]);
    let preferred_ids = entries
        .iter()
        .filter(|entry| {
            // Red fives are preferred by physical ID, not every normal five of that suit.
            (preferences.prefer_dora && matches!(face(&deck, entry.id), "0m" | "0p" | "0s"))
                || souzu::tile(&entry.face).is_some_and(|tile| preferred_faces[tile as usize])
        })
        .map(|entry| entry.id)
        .collect();
    let Some(melds) = search.enumerate(&entries, &preferred_ids, preferences) else {
        return stopped();
    };
    search.progress(false, true);
    let (rounds, limit) = search_limits(state);
    let replacement = entries
        .iter()
        .filter(|entry| entry.source == "replacement")
        .map(|entry| entry.id)
        .collect::<Vec<_>>();
    let result = search.visit(0, &mut Vec::new(), [0; 3], &melds,
        &mut skip.iter().cloned().map(|signature| (signature, [usize::MAX; 3])).collect(), &mut |meld_ids, signature| {
        let target = candidate_ids(std::iter::once(1000).chain(meld_ids.iter().copied()).collect(), &deck);
        let (discards, incoming) = simulate(&hand, &target, &replacement, rounds, limit, &deck)?;
        let faces = target.iter().map(|&id| norm(face(&deck, id))).collect::<Vec<_>>();
        let resolved = meld_ids.chunks_exact(3).chain(std::iter::once([1000].as_slice())).map(|group| {
            let mut group = group.to_vec();
            group.sort_unstable_by_key(|&id| (souzu::tile(face(&deck, id)), id));
            json!({"quad":false,"tiles":group.iter().map(|&id| json!({"id":id,"face":face(&deck,id)})).collect::<Vec<_>>()})
        }).collect::<Vec<_>>();
        Some(json!({
            "status":"plan","mode":"wanxiang-four-meld-switch","search_algorithm":"wanxiang_four_meld_switch",
            "draws_needed":0,"tenpai_draws_needed":0,"target13":faces,"target14":faces,
            "target_physical_ids":target,"target_physical_faces":faces,
            "resolved_groups":resolved,
            "discards":discards.first().cloned().unwrap_or_default(),
            "switch_batch_sizes":discards.iter().map(Vec::len).collect::<Vec<_>>(),
            "switch_discards":discards,"switch_in":incoming,"wall_draws":[],"post_draw_discards":[],
            "waits":[],"quad_faces":[],"remaining_changes":rounds,"per_change_limit":limit,"plan_signature":signature
        }))
    });
    search.progress(false, true);
    let mut result = result.unwrap_or_else(|| json!({"status":"impossible","reason":"cannot-form-four-melds-with-wanxiang","remaining_changes":rounds}));
    result["search_preferences"] = json!(preferences);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(hand: &[&str; 12], replacement: &[&str]) -> Value {
        let mut deck = Map::new();
        for (index, tile) in hand.iter().chain(replacement).enumerate() {
            deck.insert((index + 1).to_string(), json!(tile));
        }
        deck.insert("1000".into(), json!("bd"));
        json!({"deck_map":deck,"hand_tiles":(1..=12).chain([1000]).collect::<Vec<_>>(),
            "replacement_tiles":(13..13+replacement.len()).collect::<Vec<_>>(),
            "total_change_tile_count":2})
    }

    fn plan(state: &Value, preferences: &SearchPreferences, skip: &[String]) -> Value {
        let result = super::super::switch_plan_with_preferences(
            state,
            36,
            skip,
            Some("wanxiang_four_meld_switch"),
            preferences,
            &mut |_| {},
            &|| false,
        );
        if result["status"] == "plan" {
            let (rounds, limit) = search_limits(state);
            let batches = result["switch_discards"].as_array().unwrap();
            assert!(batches.len() <= rounds);
            let replacement = pool(state, 0).into_iter().filter(|entry| entry.source == "replacement")
                .map(|entry| entry.id).collect::<Vec<_>>();
            let mut actual = ids(state.get("hand_tiles"));
            let mut cursor = 0;
            for (out, incoming) in batches.iter().zip(result["switch_in"].as_array().unwrap()) {
                let out = ids(Some(out));
                let incoming = ids(Some(incoming));
                assert!(!out.is_empty() && out.len() <= limit);
                assert!(out.iter().all(|id| actual.contains(id)));
                assert_eq!(out.len(), incoming.len());
                assert_eq!(incoming, replacement[cursor..cursor + incoming.len()]);
                cursor += incoming.len();
                actual.retain(|id| !out.contains(id));
                actual.extend(incoming);
            }
            if let Some(steps) = result["custom_steps"].as_array() {
                let wall = ids(state.get("wall_tiles"));
                let mut draws = 0;
                for step in steps {
                    let tile = step["tiles"][0].as_u64().unwrap();
                    match step["kind"].as_str().unwrap() {
                        "exchange" => continue,
                        "draw" => {
                            assert_eq!(actual.len(), 13);
                            assert_eq!(tile, wall[draws]);
                            actual.push(tile);
                            draws += 1;
                        }
                        "discard" => {
                            assert_eq!(actual.len(), 14);
                            assert!(actual.contains(&tile));
                            assert_ne!(tile, 1000);
                            actual.retain(|&id| id != tile);
                        }
                        kind => panic!("unexpected step: {kind}"),
                    }
                }
                assert_eq!(draws, result["draws_needed"].as_u64().unwrap() as usize);
                assert_eq!(actual.len(), 14);
            }
            actual.sort_unstable();
            let mut target = ids(result.get("target_physical_ids"));
            target.sort_unstable();
            assert_eq!(actual, target);
        }
        result
    }

    #[test]
    fn wall_limit_accepts_one_through_thirty_six() {
        let snapshot = state(&["1m","2m","3m","1p","2p","3p","1s","2s","3s","1z","1z","1z"], &[]);
        let preferences = SearchPreferences::default();
        for limit in [1,36] {
            assert_eq!(search(&snapshot, limit, &[], &preferences, &mut |_| {}, &||false)["status"], "plan");
        }
        for limit in [0,37,usize::MAX] {
            assert_eq!(search(&snapshot, limit, &[], &preferences, &mut |_| {}, &||false)["reason"], "wanxiang-wall-limit");
        }
    }

    #[test]
    fn coin_rewards_count_each_indicator_but_cap_pearl_per_physical_tile() {
        let snapshot = json!({
            "deck_map":{"1":"5p","2":"0p","3":"0m","4":"0s","5":"3p","6":"1p","7":"9m","8":"1z","9":"bd",
                "101":"4p","102":"4p","103":"1m","104":"9p","105":"7z"},
            "hand_tiles":[1,2,3,4,5,6,7,8,9],"dora_tiles":[101,102,103,104,105],
            "tian_dora_tiles":["0p","5p","0m","3p"]
        });
        let entries = pool(&snapshot, 0);
        let heaven = coin_rewards(&snapshot, &entries, CoinObjective::HeavenlyBlessing);
        let pearl = coin_rewards(&snapshot, &entries, CoinObjective::ShiningPearl);
        for (id, heavenly, shining) in [(1,6,5),(2,8,5),(3,4,0),(4,2,0),(5,2,0),(6,2,3),(7,2,0),(8,2,0),(9,0,0)] {
            assert_eq!(heaven[&id], heavenly, "tile {id}");
            assert_eq!(pearl[&id], shining, "tile {id}");
        }
        let red_only = json!({"deck_map":{"1":"0p","2":"5p"},"hand_tiles":[1,2]});
        assert_eq!(coin_rewards(&red_only, &pool(&red_only,0), CoinObjective::ShiningPearl), HashMap::from([(1,3),(2,0)]));
    }

    #[test]
    fn coin_search_prefers_later_higher_reward_and_replays_the_winning_hand() {
        let mut snapshot = state(&["1m","2m","3m","1s","2s","3s","1z","1z","1z","4p","5p","8p"], &["6p"]);
        snapshot["wall_tiles"] = json!([14,15]);
        for (id, tile) in [(14,"7z"),(15,"0p"),(101,"4p"),(102,"4p")] {
            snapshot["deck_map"][id.to_string()] = json!(tile);
        }
        snapshot["dora_tiles"] = json!([101,102]);
        snapshot["tian_dora_tiles"] = json!(["5p"]);
        for (objective, early_reward, best_reward) in [(CoinObjective::HeavenlyBlessing,6,14),(CoinObjective::ShiningPearl,5,10)] {
            let preferences = SearchPreferences { coin_objective:Some(objective), prefer_dora:true, prefer_soul:true,
                any_waits:true, preferred_suit:Some(super::super::TileSuit::Honor), preferred_meld_type:Some(MeldType::Triplet) };
            let early = search(&snapshot, 1, &[], &preferences, &mut |_| {}, &||false);
            assert_eq!(early["coin_reward"], early_reward, "{early}");
            assert_eq!(early["draws_needed"], 1);
            assert_eq!(early["tenpai_draws_needed"], 0);
            assert_eq!(early["win_count"], 2);
            assert_eq!(early["total_coin_reward"], early_reward * 2);
            let best = plan(&snapshot, &preferences, &[]);
            assert_eq!(best["coin_reward"], best_reward, "{best}");
            assert_eq!(best["draws_needed"], 2);
            assert_eq!(best["tenpai_draws_needed"], 0);
            assert_eq!(best["win_count"], 1);
            assert_eq!(best["total_coin_reward"], best_reward);
            assert_eq!(best["post_draw_discards"], json!([14]));
            assert!(ids(best.get("target_physical_ids")).contains(&15));
            assert_eq!(best["search_preferences"]["prefer_dora"], false);
            assert_eq!(best["search_preferences"]["any_waits"], false);
            assert!(best["search_preferences"]["preferred_suit"].is_null());
            assert!(best["search_preferences"]["preferred_meld_type"].is_null());
            let next = plan(&snapshot, &preferences, &[best["plan_signature"].as_str().unwrap().into()]);
            assert_ne!(best["plan_signature"], next["plan_signature"]);
            assert_eq!(next["coin_reward"], early_reward);
            assert_eq!(next["total_coin_reward"], early_reward * 2);
            assert_eq!(search(&snapshot, 0, &[], &preferences, &mut |_| {}, &||false)["status"], "impossible");
            assert_eq!(search(&snapshot, 36, &[], &preferences, &mut |_| {}, &||true)["reason"], "stopped-by-user");
        }
        snapshot["total_change_tile_count"] = json!(0);
        assert_eq!(plan(&snapshot, &SearchPreferences { coin_objective:Some(CoinObjective::HeavenlyBlessing), ..Default::default() }, &[])["status"], "impossible");
    }

    #[test]
    fn coin_search_maximizes_reward_times_wins_instead_of_one_win() {
        let mut snapshot = state(&["1m","2m","3m","1s","2s","3s","1z","1z","1z","4p","5p","8p"], &["6p"]);
        snapshot["wall_tiles"] = json!([14,15,16,17,18]);
        for (id, tile) in [(14,"7z"),(15,"7z"),(16,"7z"),(17,"7z"),(18,"0p"),(101,"4p"),(102,"4p")] {
            snapshot["deck_map"][id.to_string()] = json!(tile);
        }
        snapshot["dora_tiles"] = json!([101,102]);
        snapshot["tian_dora_tiles"] = json!(["5p"]);
        for (objective, reward) in [(CoinObjective::HeavenlyBlessing,6),(CoinObjective::ShiningPearl,5)] {
            let preferences = SearchPreferences {coin_objective:Some(objective), ..Default::default()};
            for wall_limit in [5,1] {
                let result = search(&snapshot, wall_limit, &[], &preferences, &mut |_| {}, &||false);
                assert_eq!(result["coin_reward"], reward, "{result}");
                assert_eq!(result["draws_needed"], 1, "{result}");
                assert_eq!(result["win_count"], 5, "{result}");
                assert_eq!(result["total_coin_reward"], reward * 5, "{result}");
            }
        }
    }

    #[test]
    fn coin_plan_reports_when_the_target_is_ready_before_the_winning_draw() {
        let mut snapshot = state(&["1m","2m","3m","1s","2s","3s","1z","1z","1z","4p","8p","9p"], &["6p"]);
        snapshot["wall_tiles"] = json!([14,15,16]);
        for (id, tile) in [(14,"7z"),(15,"5p"),(16,"0p"),(101,"4p"),(102,"4p")] {
            snapshot["deck_map"][id.to_string()] = json!(tile);
        }
        snapshot["dora_tiles"] = json!([101,102]);
        snapshot["tian_dora_tiles"] = json!(["5p"]);
        for (objective, reward) in [(CoinObjective::HeavenlyBlessing,14),(CoinObjective::ShiningPearl,10)] {
            let result = plan(&snapshot, &SearchPreferences {coin_objective:Some(objective), ..Default::default()}, &[]);
            assert_eq!(result["coin_reward"], reward, "{result}");
            assert_eq!(result["tenpai_draws_needed"], 2, "{result}");
            assert_eq!(result["draws_needed"], 3, "{result}");
        }
    }

    #[test]
    fn coin_search_matches_exhaustive_physical_hand_scores() {
        let mut snapshot = state(&["1m","2m","3m","1s","2s","3s","1z","1z","1z","4p","5p","8p"], &["6p","7p"]);
        snapshot["wall_tiles"] = json!([15,16,17]);
        for (id, tile) in [(15,"0p"),(16,"6p"),(17,"8p"),(101,"4p"),(102,"4p")] {
            snapshot["deck_map"][id.to_string()] = json!(tile);
        }
        snapshot["dora_tiles"] = json!([101,102]);
        snapshot["tian_dora_tiles"] = json!(["5p","7p"]);
        for (rounds, used) in [(0,vec![]),(1,vec![]),(2,vec![]),(2,vec![13])] {
            snapshot["total_change_tile_count"] = json!(rounds);
            snapshot["switch_used_tiles"] = json!(used);
            snapshot["boss_buff"] = json!([901]);
            let entries = pool(&snapshot, 3);
            let ordinary = entries.iter().filter(|e| e.id != 1000).collect::<Vec<_>>();
            for objective in [CoinObjective::HeavenlyBlessing, CoinObjective::ShiningPearl] {
                let rewards = coin_rewards(&snapshot, &entries, objective);
                let mut expected = None;
                for mask in 0u32..1 << ordinary.len() {
                    if mask.count_ones() != 13 { continue; }
                    let selected = ordinary.iter().enumerate().filter(|(i, _)| mask & (1 << i) != 0)
                        .map(|(_, e)| *e).collect::<Vec<_>>();
                    let reward = selected.iter().map(|e| rewards[&e.id]).sum::<usize>();
                    let mut counts = HashMap::new();
                    for entry in &selected { *counts.entry(entry.face.clone()).or_insert(0) += 1; }
                    let shape = counts.keys().cloned().collect::<Vec<_>>().into_iter().any(|pair| {
                        *counts.get_mut(&pair).unwrap() -= 1;
                        let complete = super::super::consume_melds(&mut counts, 4);
                        *counts.get_mut(&pair).unwrap() += 1;
                        complete
                    });
                    let target = selected.iter().map(|e| e.id).chain([1000]).collect::<Vec<_>>();
                    if shape {
                        if let Some((_, _, _, draws)) = custom::simulate(&snapshot, &entries, &target, &[], 3) {
                            let total = reward * (3 - draws + 1);
                            expected = Some(expected.map_or(total, |best: usize| best.max(total)));
                        }
                    }
                }
                let result = plan(&snapshot, &SearchPreferences {coin_objective:Some(objective), ..Default::default()}, &[]);
                assert_eq!(result["total_coin_reward"].as_u64().map(|n| n as usize), expected, "{result}");
            }
        }
    }

    #[test]
    fn suit_and_meld_type_preferences_support_decomposition_and_fallback() {
        let mut snapshot = state(&[
            "1m", "1m", "1m", "2m", "2m", "2m", "3m", "3m", "3m", "4p", "5p", "6p",
        ], &[]);
        snapshot["tian_dora_tiles"] = json!(["1m"]);
        for (kind, expected, bonus) in [(MeldType::Triplet, 3, 1), (MeldType::Sequence, 4, 3)] {
            let preferences = SearchPreferences {
                preferred_suit: Some(super::super::TileSuit::Man),
                preferred_meld_type: Some(kind), prefer_soul: true, ..Default::default()
            };
            let result = plan(&snapshot, &preferences, &[]);
            assert_eq!(result["draws_needed"], 0);
            assert_eq!(result["preferred_suit_count"], 3);
            assert_eq!(result["preferred_meld_type_count"], expected, "{result}");
            assert_eq!(result["preferred_meld_count"], bonus); // Meld type takes priority over bonus count.
            let groups = result["resolved_groups"].as_array().unwrap();
            assert_eq!(groups.len(), 5);
            assert_eq!(groups[4]["tiles"], json!([{"id":1000,"face":"bd"}]));
            let triplets = groups[..4].iter().filter(|group| {
                let tiles = group["tiles"].as_array().unwrap();
                assert_eq!(tiles.len(), 3);
                tiles.iter().all(|tile| tile["face"] == tiles[0]["face"])
            }).count();
            assert_eq!(if kind == MeldType::Triplet { triplets } else { 4 - triplets }, expected);
            let mut grouped_ids = groups.iter().flat_map(|group| group["tiles"].as_array().unwrap())
                .map(|tile| tile["id"].as_u64().unwrap()).collect::<Vec<_>>();
            grouped_ids.sort_unstable();
            let mut target = ids(result.get("target_physical_ids"));
            target.sort_unstable();
            assert_eq!(grouped_ids, target);
        }
        for (suit, preferred_suit) in [('m', super::super::TileSuit::Man), ('p', super::super::TileSuit::Pin), ('s', super::super::TileSuit::Sou), ('z', super::super::TileSuit::Honor)] {
            let hand = (1..=4).flat_map(|rank| vec![format!("{rank}{suit}"); 3]).collect::<Vec<_>>();
            let hand = hand.iter().map(String::as_str).collect::<Vec<_>>().try_into().unwrap();
            let mut snapshot = state(&hand, &[]);
            if suit == 'p' {
                snapshot["deck_map"]["12"] = json!("0p");
                snapshot["deck_map"]["10"] = json!("5p");
                snapshot["deck_map"]["11"] = json!("5p");
            }
            let result = plan(&snapshot, &SearchPreferences { preferred_suit: Some(preferred_suit), ..Default::default() }, &[]);
            assert_eq!(result["draws_needed"], 0, "{result}");
            assert_eq!(result["preferred_suit_count"], 4, "{result}");
        }

        let mut snapshot = state(&["1m", "2m", "3m", "1p", "2p", "3p", "1s", "2s", "3s", "4m", "6m", "7m"], &["5m", "1z", "1z", "1z"]);
        let preferences = SearchPreferences {
            preferred_suit: Some(super::super::TileSuit::Honor),
            preferred_meld_type: Some(MeldType::Triplet), ..Default::default()
        };
        let result = plan(&snapshot, &preferences, &[]);
        assert_eq!(result["draws_needed"], 0, "{result}");
        assert_eq!(result["preferred_suit_count"], 1);
        assert_eq!(result["preferred_meld_type_count"], 1);
        assert_eq!(result["switch_batch_sizes"], json!([3, 1]));
        assert_ne!(result["plan_signature"], plan(&snapshot, &SearchPreferences::default(), &[])["plan_signature"]);

        // A different decomposition can reach the honor triplet in one larger exchange.
        snapshot["total_change_tile_count"] = json!(1);
        let result = plan(&snapshot, &preferences, &[]);
        assert_eq!(result["preferred_suit_count"], 1, "{result}");
        assert_eq!(result["switch_batch_sizes"], json!([4]));

        // With the three-tile limit, the last honor tile is no longer reachable.
        snapshot["boss_buff"] = json!([901]);
        let result = plan(&snapshot, &preferences, &[]);
        assert_eq!(result["draws_needed"], 0, "{result}");
        assert_eq!(result["preferred_suit_count"], 0);
        assert_eq!(result["preferred_meld_type_count"], 0);
        assert_eq!(result["plan_signature"], plan(&snapshot, &SearchPreferences::default(), &[])["plan_signature"]);
    }

    #[test]
    fn preferred_melds_ignore_exchange_depth_but_preserve_reachability() {
        let hand = [
            "1m", "2m", "3m", "1p", "2p", "3p", "1s", "2s", "3s", "4m", "6m", "7m",
        ];
        let mut snapshot = state(&hand, &["5m"]);
        snapshot["dora_tiles"] = json!([11]); // 6m indicates 7m.
        snapshot["tian_dora_tiles"] = json!(["7m"]);
        let default = SearchPreferences::default();
        let ordinary = plan(&snapshot, &default, &[]);
        assert_eq!(ordinary["status"], "plan");
        assert!(!ids(ordinary.get("target_physical_ids")).contains(&12));
        let mut preferences = SearchPreferences {
            prefer_dora: true,
            prefer_soul: true,
            any_waits: false,
            ..Default::default()
        };
        for (dora, soul) in [(true, false), (false, true), (true, true)] {
            preferences.prefer_dora = dora;
            preferences.prefer_soul = soul;
            let preferred = plan(&snapshot, &preferences, &[]);
            assert_eq!(preferred["draws_needed"], 0);
            assert_eq!(preferred["preferred_meld_count"], 1); // Dora+soul counts once per meld.
            assert!(ids(preferred.get("target_physical_ids")).contains(&12));
            assert_eq!(preferred["search_preferences"]["prefer_dora"], dora);
            let skip = vec![preferred["plan_signature"].as_str().unwrap().to_owned()];
            let next = plan(&snapshot, &preferences, &skip);
            assert_eq!(next["status"], "plan");
            assert_eq!(next["draws_needed"], 0);
            assert_ne!(next["plan_signature"], preferred["plan_signature"]);
        }

        // Extra exchanges do not delay tenpai: both hands need zero wall draws.
        snapshot = state(&hand, &["5m", "8m"]);
        snapshot["tian_dora_tiles"] = json!(["8m"]);
        let later = plan(&snapshot, &preferences, &[]);
        assert_eq!(later["draws_needed"], 0);
        assert_eq!(later["preferred_meld_count"], 1);
        assert_eq!(later["switch_batch_sizes"], json!([1, 1]));
        assert!(ids(later.get("target_physical_ids")).contains(&14));
        assert_ne!(
            plan(&snapshot, &default, &[])["plan_signature"],
            later["plan_signature"]
        );

        snapshot["total_change_tile_count"] = json!(1);
        let single_batch = plan(&snapshot, &preferences, &[]);
        assert_eq!(single_batch["preferred_meld_count"], 1);
        assert_eq!(single_batch["switch_batch_sizes"], json!([2]));

        snapshot = state(&hand, &["5m", "7z", "6z", "8m"]);
        snapshot["tian_dora_tiles"] = json!(["8m"]);
        snapshot["boss_buff"] = json!([901]);
        snapshot["total_change_tile_count"] = json!(1);
        let reachable = plan(&snapshot, &preferences, &[]);
        assert_eq!(reachable["status"], "plan");
        assert_eq!(reachable["draws_needed"], 0);
        assert_eq!(reachable["preferred_meld_count"], 0);

        // Red preference must select the red physical five, not any normal five.
        snapshot = state(
            &[
                "1m", "2m", "3m", "1p", "2p", "3p", "1s", "2s", "3s", "4p", "5p", "0p",
            ],
            &["6p"],
        );
        let red = plan(&snapshot, &preferences, &[]);
        assert_eq!(red["draws_needed"], 0);
        assert_eq!(red["preferred_meld_count"], 1);
        assert!(ids(red.get("target_physical_ids")).contains(&12));
        assert!(!ids(red.get("target_physical_ids")).contains(&11));
        assert!(red["resolved_groups"].as_array().unwrap().iter().any(|group| {
            group["tiles"].as_array().unwrap().iter().map(|tile| tile["face"].as_str().unwrap())
                .collect::<Vec<_>>() == ["4p", "0p", "6p"]
        }));

        // Identical physical tiles can form three preferred sequences or one preferred triplet.
        snapshot = state(
            &[
                "1m", "1m", "1m", "2m", "2m", "2m", "3m", "3m", "3m", "4p", "5p", "6p",
            ],
            &[],
        );
        snapshot["tian_dora_tiles"] = json!(["1m"]);
        let decomposed = plan(&snapshot, &preferences, &[]);
        assert_eq!(decomposed["draws_needed"], 0);
        assert_eq!(decomposed["preferred_meld_count"], 3);
        assert_eq!(
            search(&snapshot, 36, &[], &preferences, &mut |_| {}, &|| true)["reason"],
            "stopped-by-user"
        );

        snapshot = state(&hand, &[]);
        assert_eq!(
            plan(&snapshot, &preferences, &[])["reason"],
            "cannot-form-four-melds-with-wanxiang"
        );
    }
}
