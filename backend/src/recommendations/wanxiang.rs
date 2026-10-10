//! v2.5.0 physical-meld enumeration and first-reachable DFS, without wall draws.
//! Rank reachable plans by replacement depth, then preferred suit, meld type and bonus tiles.
use super::{
    deck, face, ids, norm, pool, search_limits, souzu, Entry, MeldType, SearchPreferences, SwitchBatches,
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
    best_rank: Option<(usize, Reverse<[usize; 3]>)>,
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
        result.sort_unstable_by_key(|meld| meld.key);
        Some(result)
    }

    fn visit(
        &mut self,
        start: usize,
        chosen: &mut Vec<u64>,
        depth: usize,
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
        // Later replacement tiles cannot improve depth; each remaining meld adds at most one match.
        if self.preference_limits != [0; 3]
            && self
                .best_rank
                .is_some_and(|best| {
                    let upper = std::array::from_fn(|i| preferred[i] + slots * self.preference_limits[i]);
                    (depth, Reverse(upper)) >= best
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
            self.best_rank = Some((depth, Reverse(preferred)));
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
            // Melds are ordered by their latest physical tile, hence by replacement depth.
            if self.preference_limits != [0; 3]
                && self
                    .best_rank
                    .is_some_and(|(depth, _)| melds[index].key.1 > depth)
            {
                break;
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
                depth.max(melds[index].key.1),
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
    preferences: &SearchPreferences,
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
        preference_limits: [
            usize::from(preferences.preferred_suit.is_some()),
            usize::from(preferences.preferred_meld_type.is_some()),
            usize::from(preferences.prefer_dora || preferences.prefer_soul),
        ],
        best_rank: None,
    };
    search.progress(true, true);
    if should_stop() {
        return stopped();
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
    let result = search.visit(0, &mut Vec::new(), 0, [0; 3], &melds,
        &mut skip.iter().cloned().map(|signature| (signature, [usize::MAX; 3])).collect(), &mut |meld_ids, signature| {
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
        super::super::switch_plan_with_preferences(
            state,
            36,
            skip,
            Some("wanxiang_four_meld_switch"),
            preferences,
            &mut |_| {},
            &|| false,
        )
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

        let snapshot = state(&["1m", "2m", "3m", "1p", "2p", "3p", "1s", "2s", "3s", "4m", "6m", "7m"], &["5m", "1z", "1z", "1z"]);
        let preferences = SearchPreferences {
            preferred_suit: Some(super::super::TileSuit::Honor),
            preferred_meld_type: Some(MeldType::Triplet), ..Default::default()
        };
        let result = plan(&snapshot, &preferences, &[]);
        assert_eq!(result["draws_needed"], 1, "{result}");
        assert_eq!(result["preferred_suit_count"], 0);
        assert_eq!(result["preferred_meld_type_count"], 0);
        assert_eq!(result["plan_signature"], plan(&snapshot, &SearchPreferences::default(), &[])["plan_signature"]);
    }

    #[test]
    fn preferred_melds_break_ties_without_delaying_the_plan() {
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
            assert_eq!(preferred["draws_needed"], 1);
            assert_eq!(preferred["preferred_meld_count"], 1); // Dora+soul counts once per meld.
            assert!(ids(preferred.get("target_physical_ids")).contains(&12));
            assert_eq!(preferred["search_preferences"]["prefer_dora"], dora);
            // The executable batches still produce exactly the returned physical hand.
            let mut actual = ids(snapshot.get("hand_tiles"));
            for (out, incoming) in preferred["switch_discards"]
                .as_array()
                .unwrap()
                .iter()
                .zip(preferred["switch_in"].as_array().unwrap())
            {
                actual.retain(|id| !ids(Some(out)).contains(id));
                actual.extend(ids(Some(incoming)));
            }
            actual.sort_unstable();
            let mut target = ids(preferred.get("target_physical_ids"));
            target.sort_unstable();
            assert_eq!(actual, target);
            let skip = vec![preferred["plan_signature"].as_str().unwrap().to_owned()];
            let next = plan(&snapshot, &preferences, &skip);
            assert_eq!(next["status"], "plan");
            assert_eq!(next["draws_needed"], 1);
            assert_ne!(next["plan_signature"], preferred["plan_signature"]);
        }

        // A later preferred sequence must not displace an earlier ordinary one.
        snapshot = state(&hand, &["5m", "8m"]);
        snapshot["tian_dora_tiles"] = json!(["8m"]);
        let early = plan(&snapshot, &preferences, &[]);
        assert_eq!(early["draws_needed"], 1);
        assert_eq!(early["preferred_meld_count"], 0);
        assert_eq!(
            plan(&snapshot, &default, &[])["plan_signature"],
            early["plan_signature"]
        );

        // Red preference must select the red physical five, not any normal five.
        snapshot = state(
            &[
                "1m", "2m", "3m", "1p", "2p", "3p", "1s", "2s", "3s", "4p", "5p", "0p",
            ],
            &["6p"],
        );
        let red = plan(&snapshot, &preferences, &[]);
        assert_eq!(red["draws_needed"], 1);
        assert_eq!(red["preferred_meld_count"], 1);
        assert!(ids(red.get("target_physical_ids")).contains(&12));
        assert!(!ids(red.get("target_physical_ids")).contains(&11));

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
            search(&snapshot, &[], &preferences, &mut |_| {}, &|| true)["reason"],
            "stopped-by-user"
        );

        snapshot = state(&hand, &[]);
        assert_eq!(
            plan(&snapshot, &preferences, &[])["reason"],
            "cannot-form-four-melds-with-wanxiang"
        );
    }
}
