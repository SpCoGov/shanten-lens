use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
mod souzu;
mod wanxiang;
mod custom;

pub fn custom_switch_plan(state: &Value, target: &Value, wall_limit: usize,
    progress: &mut dyn FnMut(Value), stopped: &dyn Fn() -> bool) -> Value {
    custom::search(state, target, wall_limit, progress, stopped)
}

#[derive(Clone, Copy, serde::Deserialize, serde::Serialize)]
pub enum TileSuit {
    #[serde(rename = "m")]
    Man,
    #[serde(rename = "p")]
    Pin,
    #[serde(rename = "s")]
    Sou,
    #[serde(rename = "z")]
    Honor,
}

impl TileSuit {
    fn matches(self, tile: u8) -> bool {
        tile / 9 == match self {
            Self::Man => 0,
            Self::Pin => 1,
            Self::Sou => 2,
            Self::Honor => 3,
        }
    }
}

#[derive(Clone, Copy, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MeldType {
    Triplet,
    Sequence,
}

#[derive(Clone, Default, serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct SearchPreferences {
    pub prefer_dora: bool,
    pub prefer_soul: bool,
    pub any_waits: bool,
    pub preferred_suit: Option<TileSuit>,
    pub preferred_meld_type: Option<MeldType>,
}

fn ids(value: Option<&Value>) -> Vec<u64> {
    value
        .and_then(Value::as_array)
        .map(|v| v.iter().filter_map(Value::as_u64).collect())
        .unwrap_or_default()
}
fn deck(state: &Value) -> Map<String, Value> {
    state
        .get("deck_map")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default()
}
fn face(deck: &Map<String, Value>, id: u64) -> &str {
    deck.get(&id.to_string())
        .and_then(Value::as_str)
        .unwrap_or("")
}
fn norm(value: &str) -> String {
    match value {
        "0m" => "5m".into(),
        "0p" => "5p".into(),
        "0s" => "5s".into(),
        _ => value.into(),
    }
}

fn chiitoi_ready(faces: &[String]) -> bool {
    let mut counts = HashMap::<&str, u32>::new();
    let mut jokers = 0;
    for tile in faces {
        if tile == "bd" {
            jokers += 1
        } else {
            *counts.entry(tile).or_default() += 1;
        }
    }
    let pairs = counts.values().filter(|&&n| n >= 2).count();
    let singles = counts.values().filter(|&&n| n == 1).count();
    let need = 7usize.saturating_sub(pairs);
    jokers >= need.min(singles) + 2 * need.saturating_sub(singles)
}

pub fn chiitoi(state: &Value) -> Value {
    let deck = deck(state);
    let hand = ids(state.get("hand_tiles"));
    let wall = ids(state.get("wall_tiles"));
    if hand.len() != 14 {
        return json!({"status":"impossible","reason":"hand-must-be-14"});
    }
    let hand_faces = hand
        .iter()
        .map(|id| norm(face(&deck, *id)))
        .collect::<Vec<_>>();
    if chiitoi_ready(&hand_faces) {
        return json!({"status":"win_now","draws_needed":0,"target14":hand_faces,"discards":[]});
    }
    let mut best: Option<(usize, u64)> = None;
    for (index, discard) in hand.iter().enumerate() {
        let mut pool = hand_faces.clone();
        pool.remove(index);
        for (draw_index, id) in wall.iter().enumerate() {
            pool.push(norm(face(&deck, *id)));
            if chiitoi_ready(&pool) {
                let candidate = (draw_index + 1, *discard);
                if best.is_none_or(|old| candidate < old) {
                    best = Some(candidate)
                }
                break;
            }
        }
    }
    best.map_or_else(||json!({"status":"impossible","reason":"no-path-to-chiitoi"}),|(draws,discard)|json!({"status":"plan","draws_needed":draws,"target14":[],"discards":[discard]}))
}

fn suuankou_ready(faces: &[String]) -> bool {
    let mut counts = [0u8; 9];
    let mut jokers = 0u8;
    for tile in faces {
        if tile == "bd" {
            jokers += 1
        } else if let Some(rank) = tile.strip_suffix('p').and_then(|v| v.parse::<usize>().ok()) {
            if (1..=9).contains(&rank) {
                counts[rank - 1] += 1;
            }
        }
    }
    for a in 0..9 {
        for b in a + 1..9 {
            for c in b + 1..9 {
                for d in c + 1..9 {
                    for pair in 0..9 {
                        let mut need = [0u8; 9];
                        for i in [a, b, c, d] {
                            need[i] += 3
                        }
                        need[pair] += 2;
                        let deficit = (0..9)
                            .map(|i| need[i].saturating_sub(counts[i]) as u16)
                            .sum::<u16>();
                        if deficit <= jokers as u16 {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

pub fn suuankou(state: &Value) -> Value {
    let deck = deck(state);
    let hand = ids(state.get("hand_tiles"));
    let wall = ids(state.get("wall_tiles"));
    if hand.len() != 14 {
        return json!({"status":"impossible","reason":"hand-must-be-14"});
    }
    let hand_faces = hand
        .iter()
        .map(|id| norm(face(&deck, *id)))
        .collect::<Vec<_>>();
    if suuankou_ready(&hand_faces) {
        return json!({"status":"win_now","draws_needed":0,"target14":hand_faces,"discards":[]});
    }
    let mut best = None;
    for (index, discard) in hand.iter().enumerate() {
        let mut pool = hand_faces.clone();
        pool.remove(index);
        for (draw_index, id) in wall.iter().enumerate() {
            pool.push(norm(face(&deck, *id)));
            if suuankou_ready(&pool) {
                let candidate = (draw_index + 1, *discard);
                if best.is_none_or(|old| candidate < old) {
                    best = Some(candidate)
                }
                break;
            }
        }
    }
    best.map_or_else(||json!({"status":"impossible","reason":"not-enough-pinzu-or-bd"}),|(draws,discard)|json!({"status":"plan","draws_needed":draws,"target14":[],"discards":[discard]}))
}

#[derive(Clone)]
struct Entry {
    id: u64,
    face: String,
    source: &'static str,
    index: usize,
}
fn pool(state: &Value, wall_limit: usize) -> Vec<Entry> {
    let deck = deck(state);
    let mut out = Vec::new();
    let remaining_changes = state
        .get("total_change_tile_count")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        .saturating_sub(
            state
                .get("change_tile_count")
                .and_then(Value::as_u64)
                .unwrap_or(0),
        ) as usize;
    let per_change = if ids(state.get("boss_buff")).contains(&901) {
        3
    } else {
        13
    };
    let used = ids(state.get("switch_used_tiles")).len();
    for (source, key, skip, limit) in [
        ("hand", "hand_tiles", 0, usize::MAX),
        (
            "replacement",
            "replacement_tiles",
            used,
            remaining_changes * per_change,
        ),
        ("wall", "wall_tiles", 0, wall_limit),
    ] {
        for (index, id) in ids(state.get(key))
            .into_iter()
            .skip(skip)
            .take(limit)
            .enumerate()
        {
            out.push(Entry {
                id,
                face: norm(face(&deck, id)),
                source,
                index,
            })
        }
    }
    out
}

pub fn quad_catalog(state: &Value, wall_limit: usize) -> Value {
    let mut groups = HashMap::<String, Vec<Entry>>::new();
    for entry in pool(state, wall_limit) {
        groups.entry(entry.face.clone()).or_default().push(entry)
    }
    Value::Array(groups.into_iter().filter(|(_,entries)|entries.len()>=4).map(|(face,entries)|{let positions=entries.into_iter().take(4).map(|entry|json!({"tile_id":entry.id,"source":entry.source,"source_index":entry.index})).collect::<Vec<_>>();json!({"face":face,"tile_positions":positions,"reachable":true})}).collect())
}

pub fn switch_plan(
    state: &Value,
    wall_limit: usize,
    skip_signatures: &[String],
    algorithm: Option<&str>,
) -> Value {
    switch_plan_with_progress(
        state,
        wall_limit,
        skip_signatures,
        algorithm,
        &mut |_| {},
        &|| false,
    )
}

pub fn switch_plan_with_progress(
    state: &Value,
    wall_limit: usize,
    skip_signatures: &[String],
    algorithm: Option<&str>,
    progress: &mut dyn FnMut(Value),
    stopped: &dyn Fn() -> bool,
) -> Value {
    switch_plan_with_preferences(state, wall_limit, skip_signatures, algorithm,
        &SearchPreferences::default(), progress, stopped)
}

pub fn switch_plan_with_preferences(
    state: &Value,
    wall_limit: usize,
    skip_signatures: &[String],
    algorithm: Option<&str>,
    preferences: &SearchPreferences,
    progress: &mut dyn FnMut(Value),
    stopped: &dyn Fn() -> bool,
) -> Value {
    let hand = ids(state.get("hand_tiles"));
    if hand.len() != 13 {
        return json!({"status":"impossible","reason":"switch-hand-must-be-13"});
    }
    let has_wanxiang = hand.contains(&1000);
    match algorithm {
        Some("wanxiang_four_meld_switch") => wanxiang::search(state, skip_signatures, preferences, progress, stopped),
        Some("target_enumeration_three_quads") => souzu::search_with_preferences(state, wall_limit, 3, skip_signatures, preferences, progress, stopped),
        Some("target_enumeration_search") => souzu::search_with_preferences(state, wall_limit, 2, skip_signatures, preferences, progress, stopped),
        None if has_wanxiang => wanxiang::search(state, skip_signatures, preferences, progress, stopped),
        None => souzu::search_with_preferences(state, wall_limit, 2, skip_signatures, preferences, progress, stopped),
        Some(_) => json!({"status":"impossible","reason":"unknown-algorithm"}),
    }
}

pub fn validate_manual_plan(
    state: &Value,
    wall_limit: usize,
    quad_groups: &Value,
    structure_groups: &Value,
) -> Value {
    let entries = pool(state, wall_limit);
    let deck = deck(state);
    let pool_ids = entries.iter().map(|entry| entry.id).collect::<HashSet<_>>();
    let Some(quads) = quad_groups.as_array() else {
        return json!({"status":"impossible","reason":"manual-quad-count-invalid","manual_searchable":false,"manual_search_reason":"quad_groups must be an array"});
    };
    if quads.len() != 2 {
        return json!({"status":"impossible","reason":"manual-quad-count-invalid","manual_searchable":false,"manual_search_reason":"exactly two quads are required"});
    }
    let mut target = Vec::new();
    let mut used = HashSet::new();
    let mut quad_faces = Vec::new();
    for (index, group) in quads.iter().enumerate() {
        let Some(group) = group.as_array() else {
            return json!({"status":"impossible","reason":format!("manual-quad-{}-size-invalid",index+1),"manual_searchable":false});
        };
        let values = group.iter().filter_map(Value::as_u64).collect::<Vec<_>>();
        if values.len() != 4
            || values
                .iter()
                .any(|id| !pool_ids.contains(id) || !used.insert(*id))
        {
            return json!({"status":"impossible","reason":format!("manual-quad-{}-invalid",index+1),"manual_searchable":false});
        }
        let faces = values
            .iter()
            .map(|id| norm(face(&deck, *id)))
            .collect::<HashSet<_>>();
        if faces.len() != 1 {
            return json!({"status":"impossible","reason":format!("manual-quad-{}-not-a-quad",index+1),"manual_searchable":false});
        }
        quad_faces.push(faces.into_iter().next().unwrap());
        target.extend(values);
    }
    let Some(structure) = structure_groups.as_object() else {
        return json!({"status":"impossible","reason":"manual-structure-invalid","manual_searchable":false});
    };
    let mut buckets = HashMap::<&str, Vec<u64>>::new();
    for (name, limit) in [("meld1", 3), ("meld2", 3), ("pair", 2)] {
        let values = structure
            .get(name)
            .and_then(Value::as_array)
            .map(|values| values.iter().filter_map(Value::as_u64).collect::<Vec<_>>())
            .unwrap_or_default();
        if values.len() > limit
            || values
                .iter()
                .any(|id| !pool_ids.contains(id) || !used.insert(*id))
        {
            return json!({"status":"impossible","reason":"manual-structure-invalid","manual_searchable":false});
        }
        buckets.insert(name, values);
    }
    if buckets.values().map(Vec::len).sum::<usize>() != 7 {
        return json!({"status":"impossible","reason":"manual-structure-total-invalid","manual_searchable":false});
    }
    let deficits = [
        ("meld1", 3usize - buckets["meld1"].len()),
        ("meld2", 3usize - buckets["meld2"].len()),
        ("pair", 2usize - buckets["pair"].len()),
    ];
    let searchable = deficits.iter().filter(|(_, deficit)| *deficit == 1).count() == 1
        && deficits.iter().all(|(_, deficit)| *deficit <= 1)
        && buckets.iter().all(|(name, values)| {
            let faces = values
                .iter()
                .map(|id| norm(face(&deck, *id)))
                .collect::<Vec<_>>();
            match (*name, values.len()) {
                ("pair", 2) => faces[0] == faces[1],
                ("pair", 1) => true,
                (_, 3) => is_meld(&faces),
                (_, 2) => (1..=9).any(|rank| {
                    let mut completed = faces.clone();
                    completed.push(format!("{rank}s"));
                    is_meld(&completed)
                }),
                _ => false,
            }
        });
    for name in ["meld1", "meld2", "pair"] {
        target.extend(&buckets[name]);
    }
    let mut plan = evaluate_souzu_target(state, &entries, &target)
        .unwrap_or_else(|| json!({"status":"impossible","reason":"manual-plan-not-reachable"}));
    if let Some(object) = plan.as_object_mut() {
        object.insert("quad_faces".into(), json!(quad_faces));
        object.insert("manual_searchable".into(), Value::Bool(searchable));
        object.insert(
            "manual_search_reason".into(),
            Value::String(
                if searchable {
                    "manual-shape-covered-by-search"
                } else {
                    "manual-shape-not-covered-by-search"
                }
                .into(),
            ),
        );
    }
    plan
}

fn is_meld(faces: &[String]) -> bool {
    if faces.len() != 3 {
        return false;
    }
    if faces.iter().all(|face| face == &faces[0]) {
        return true;
    }
    let mut ordered = faces.to_vec();
    ordered.sort_by_key(|face| tile_order_key(face));
    let bytes = ordered[0].as_bytes();
    bytes.len() == 2
        && matches!(bytes[1], b'm' | b'p' | b's')
        && ordered[1] == format!("{}{}", bytes[0] - b'0' + 1, bytes[1] as char)
        && ordered[2] == format!("{}{}", bytes[0] - b'0' + 2, bytes[1] as char)
}

fn search_limits(state: &Value) -> (usize, usize) {
    let total = state
        .get("total_change_tile_count")
        .and_then(Value::as_u64)
        .unwrap_or(0) as usize;
    let used = state
        .get("change_tile_count")
        .and_then(Value::as_u64)
        .unwrap_or(0) as usize;
    (
        total.saturating_sub(used),
        if ids(state.get("boss_buff")).contains(&901) {
            3
        } else {
            13
        },
    )
}

type SwitchBatches = (Vec<Vec<u64>>, Vec<Vec<u64>>);

fn switch_simulation(
    hand: &[u64],
    replacement: &[u64],
    target: &[u64],
    rounds: usize,
    per_change: usize,
) -> Option<SwitchBatches> {
    let target = target.iter().copied().collect::<HashSet<_>>();
    let mut current = hand.to_vec();
    let mut cursor = 0usize;
    let mut discards = Vec::new();
    let mut incoming = Vec::new();
    for _ in 0..rounds {
        if target.iter().all(|id| current.contains(id)) {
            break;
        }
        let last_needed = replacement
            .iter()
            .enumerate()
            .skip(cursor)
            .filter(|(_, id)| target.contains(id) && !current.contains(id))
            .map(|(index, _)| index)
            .max()?;
        let size = per_change.min(last_needed + 1 - cursor);
        let batch = current
            .iter()
            .copied()
            .filter(|id| !target.contains(id))
            .take(size)
            .collect::<Vec<_>>();
        if batch.len() != size || cursor + size > replacement.len() {
            return None;
        }
        current.retain(|id| !batch.contains(id));
        let next = replacement[cursor..cursor + size].to_vec();
        current.extend(&next);
        cursor += size;
        discards.push(batch);
        incoming.push(next);
    }
    target
        .iter()
        .all(|id| current.contains(id))
        .then_some((discards, incoming))
}

fn evaluate_souzu_target(state: &Value, entries: &[Entry], target: &[u64]) -> Option<Value> {
    let deck = deck(state);
    let by_id = entries
        .iter()
        .map(|entry| (entry.id, entry))
        .collect::<HashMap<_, _>>();
    let quad_faces = target[..8]
        .chunks(4)
        .filter_map(|quad| quad.first().map(|id| norm(face(&deck, *id))))
        .collect::<Vec<_>>();
    let concealed = target[8..]
        .iter()
        .map(|id| norm(face(&deck, *id)))
        .collect::<Vec<_>>();
    let waits = souzu_waits(&concealed);
    if waits.is_empty() {
        return None;
    }
    let wall_targets = target
        .iter()
        .filter_map(|id| by_id.get(id).filter(|entry| entry.source == "wall"))
        .copied()
        .collect::<Vec<_>>();
    if wall_targets.len() < 2 {
        return None;
    }
    let essential = target
        .iter()
        .filter(|id| by_id.get(id).is_some_and(|entry| entry.source != "wall"))
        .copied()
        .collect::<Vec<_>>();
    if essential.len() > 13 {
        return None;
    }
    let filler_count = wall_targets.len().saturating_sub(2);
    let filler = ids(state.get("hand_tiles"))
        .into_iter()
        .filter(|id| !essential.contains(id))
        .take(filler_count)
        .collect::<Vec<_>>();
    if filler.len() != filler_count || essential.len() + filler.len() != 13 {
        return None;
    }
    let switch_target = essential.iter().chain(&filler).copied().collect::<Vec<_>>();
    let replacement = entries
        .iter()
        .filter(|entry| entry.source == "replacement")
        .map(|entry| entry.id)
        .collect::<Vec<_>>();
    let (remaining, per_change) = search_limits(state);
    let (switch_discards, switch_in) = switch_simulation(
        &ids(state.get("hand_tiles")),
        &replacement,
        &switch_target,
        remaining,
        per_change,
    )?;
    let mut wall_targets = wall_targets;
    wall_targets.sort_by_key(|entry| entry.index);
    let draws_needed = wall_targets.last().map_or(0, |entry| entry.index + 1);
    let signature = format!(
        "souzu|{}",
        target
            .iter()
            .map(u64::to_string)
            .collect::<Vec<_>>()
            .join(",")
    );
    Some(json!({
        "status":"plan","mode":"quad-first-dfs","draws_needed":draws_needed,
        "target13":concealed,"target14":concealed,"discards":switch_discards.first().cloned().unwrap_or_default(),
        "target_physical_ids":target,
        "target_physical_faces":target.iter().map(|id|norm(face(&deck,*id))).collect::<Vec<_>>(),
        "switch_batch_sizes":switch_discards.iter().map(Vec::len).collect::<Vec<_>>(),
        "switch_discards":switch_discards,"switch_in":switch_in,
        "wall_draws":wall_targets.iter().map(|entry| entry.id).collect::<Vec<_>>(),
        "post_draw_discards":filler,"waits":waits,"quad_faces":quad_faces,
        "remaining_changes":remaining,"per_change_limit":per_change,"plan_signature":signature,
        "considered_tile_count":entries.len()
    }))
}

fn souzu_waits(faces: &[String]) -> Vec<String> {
    (1..=9)
        .map(|rank| format!("{rank}s"))
        .filter(|wait| {
            let mut completed = faces.to_vec();
            completed.push(wait.clone());
            can_form_melds_and_pair(&completed, 2)
        })
        .collect()
}

fn can_form_melds_and_pair(faces: &[String], meld_count: usize) -> bool {
    let mut counts = HashMap::<String, usize>::new();
    for face in faces {
        *counts.entry(face.clone()).or_default() += 1;
    }
    let pair_faces = counts
        .iter()
        .filter(|(_, count)| **count >= 2)
        .map(|(face, _)| face.clone())
        .collect::<Vec<_>>();
    pair_faces.into_iter().any(|pair| {
        *counts.get_mut(&pair).unwrap() -= 2;
        let ok = consume_melds(&mut counts, meld_count);
        *counts.get_mut(&pair).unwrap() += 2;
        ok
    })
}

fn consume_melds(counts: &mut HashMap<String, usize>, remaining: usize) -> bool {
    if remaining == 0 {
        return counts.values().all(|count| *count == 0);
    }
    let Some(face) = counts
        .iter()
        .filter(|(_, count)| **count > 0)
        .map(|(face, _)| face.clone())
        .min_by_key(|face| tile_order_key(face))
    else {
        return false;
    };
    if counts.get(&face).copied().unwrap_or(0) >= 3 {
        *counts.get_mut(&face).unwrap() -= 3;
        if consume_melds(counts, remaining - 1) {
            *counts.get_mut(&face).unwrap() += 3;
            return true;
        }
        *counts.get_mut(&face).unwrap() += 3;
    }
    let bytes = face.as_bytes();
    if bytes.len() == 2 && matches!(bytes[1], b'm' | b'p' | b's') {
        let rank = bytes[0].saturating_sub(b'0');
        if rank <= 7 {
            let second = format!("{}{}", rank + 1, bytes[1] as char);
            let third = format!("{}{}", rank + 2, bytes[1] as char);
            if counts.get(&second).copied().unwrap_or(0) > 0
                && counts.get(&third).copied().unwrap_or(0) > 0
            {
                *counts.get_mut(&face).unwrap() -= 1;
                *counts.get_mut(&second).unwrap() -= 1;
                *counts.get_mut(&third).unwrap() -= 1;
                let ok = consume_melds(counts, remaining - 1);
                *counts.get_mut(&face).unwrap() += 1;
                *counts.get_mut(&second).unwrap() += 1;
                *counts.get_mut(&third).unwrap() += 1;
                if ok {
                    return true;
                }
            }
        }
    }
    false
}

fn tile_order_key(face: &str) -> usize {
    let bytes = face.as_bytes();
    if bytes.len() != 2 {
        return usize::MAX;
    }
    let suit = match bytes[1] {
        b'm' => 0,
        b'p' => 10,
        b's' => 20,
        b'z' => 30,
        _ => 40,
    };
    suit + bytes[0].saturating_sub(b'0') as usize
}

pub fn discard_recommendations(state: &Value) -> Value {
    json!([{"yaku":"chiitoi","data":chiitoi(state)},{"yaku":"suuannkou","data":suuankou(state)}])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wanxiang_must_already_be_in_hand() {
        let faces = [
            "1m", "2m", "3m", "4m", "5m", "6m", "1p", "2p", "3p", "1s", "2s", "3s",
        ];
        let mut deck = Map::new();
        for (index, face) in faces.iter().enumerate() {
            deck.insert((index + 1).to_string(), json!(face));
        }
        deck.insert("1000".into(), json!("bd"));
        let state = json!({
            "deck_map":deck,"hand_tiles":(1..=13).collect::<Vec<_>>(),
            "replacement_tiles":[1000],"wall_tiles":[],
            "change_tile_count":0,"total_change_tile_count":1,"boss_buff":[]
        });
        let plan = switch_plan(&state, 36, &[], Some("wanxiang_four_meld_switch"));
        assert_eq!(plan["reason"], "wanxiang-not-in-hand");
    }
}
