//! Opening-only map gate and the two supplied custom hands.
use crate::{action_provider::LEVELS, recommendations};
use serde_json::{json, Value};
use std::sync::LazyLock;

static HANDS: LazyLock<Vec<Value>> = LazyLock::new(|| {
    let mut hands: Vec<Value> = serde_json::from_str(include_str!("../assets/opening_tanyao.json")).unwrap();
    for hand in &mut hands {
        for group in hand["target_groups"].as_array_mut().unwrap() {
            group.as_object_mut().unwrap().entry("rule").or_insert_with(|| json!({}));
        }
    }
    hands
});

pub fn is_first_battle(game: &Value) -> bool {
    let level = game.get("level").filter(|value| value.as_u64().is_some_and(|n| n > 0))
        .unwrap_or(&game["provider_state"]["map"]["level"]);
    let node = game.get("node").unwrap_or(&game["provider_state"]["map"]["node"]);
    if game["source"] == "qyzz" { return level == 101 && node == 0; }
    node == 2 && LEVELS.as_array().is_some_and(|levels| levels.iter().any(|entry| entry["level"] == *level && entry["label"] == "1"))
}

pub fn map_mismatch(state: &Value) -> Option<bool> {
    if state["source"] != "packet" || !matches!(state["stage"].as_u64(), Some(3..=99)) { return None; }
    let level = state.get("level").filter(|level| level.as_u64().is_some_and(|n| n > 0))
        .unwrap_or(&state["provider_state"]["map"]["level"]);
    if !LEVELS.as_array()?.iter().any(|entry| &entry["level"] == level && entry["label"] == "1") { return None; }
    let owned = state.get("effect_list").unwrap_or(&state["provider_state"]["effect"]["effectList"]);
    if owned.as_array()?.is_empty() { return None; }
    let nodes = state.get("map_nodes").unwrap_or(&state["provider_state"]["map"]["mapNodes"]).as_array()?;
    let subtype = |index: usize| nodes.get(index)?.get("subType").or_else(|| nodes[index].get("subtype")).and_then(Value::as_u64);
    let (second, third) = (subtype(1), subtype(2));
    if second == Some(3003) || third == Some(1) { Some(false) }
    else { second.zip(third).map(|_| true) }
}

pub fn plan(game: &Value, progress: &mut dyn FnMut(Value), stopped: &dyn Fn() -> bool) -> Value {
    if stopped() { return json!({"status":"impossible","reason":"stopped-by-user"}); }
    if game["record"]["levelHuCount"].as_u64().unwrap_or(0) > 0 {
        if let Some(hand) = continuation_tiles(game) {
            return json!({"status":"plan","round_strategy":"opening","custom_steps":[],
                "continuation_tiles":hand,"target_physical_ids":hand,"target_physical_faces":hand.iter()
                    .map(|id| game["deck_map"][id.to_string()].clone()).collect::<Vec<_>>()});
        }
    }
    let wall = game["wall_tiles"].as_array().map_or(0, Vec::len) as u64;
    let first = is_first_battle(game);
    let consumed = game["used_desktop_tiles"].as_array().map_or(0, Vec::len)
        + usize::from(game["hand_tiles"].as_array().is_some_and(|hand| hand.len() % 3 == 2));
    let limit = if first { 36 } else { 18usize.saturating_sub(consumed) };
    let mut best: Option<Value> = None;
    let key = |plan: &Value| (plan["draws_needed"].as_u64().unwrap_or(u64::MAX),
        plan["switch_discards"].as_array().map_or(0, Vec::len));
    let mut failure = "opening-tanyao-unreachable".to_owned();
    for hand in HANDS.iter() {
        if stopped() { return json!({"status":"impossible","reason":"stopped-by-user"}); }
        let mut candidate = recommendations::custom_switch_plan(game, &hand["target_groups"], limit, true, progress, stopped);
        if candidate["reason"] == "stopped-by-user" { return candidate; }
        if candidate["status"] != "plan" {
            if matches!(candidate["reason"].as_str(), Some("custom-search-limit" | "invalid-search-tiles")) {
                failure = candidate["reason"].as_str().unwrap().into();
            }
            continue;
        }
        candidate["round_strategy"] = json!("opening");
        candidate["hand_name"] = hand["name"].clone();
        candidate["win_count"] = json!(wall.saturating_sub(candidate["draws_needed"].as_u64().unwrap_or(0)) + 1);
        if best.as_ref().is_none_or(|current| key(&candidate) < key(current)) { best = Some(candidate); }
    }
    if let Some(plan) = best { return plan; }
    if !first {
        let mut plan = recommendations::opening_round_plan(game, 36, progress, stopped);
        plan["round_strategy"] = json!("opening");
        return plan;
    }
    json!({"status":"impossible","reason":failure,"round_strategy":"opening"})
}

pub fn continuation_tiles(game: &Value) -> Option<Vec<Value>> {
    let hand = game["hand_tiles"].as_array().filter(|hand| hand.len() == 14)?;
    let drawn = game.get("last_drawn_tile").filter(|tile| tile.as_u64().is_some()).or_else(|| hand.last())?;
    let kept = hand.iter().filter(|tile| *tile != drawn).cloned().collect::<Vec<_>>();
    (kept.len() == 13).then_some(kept)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(hand: &[&str], wall: &[&str]) -> Value {
        let deck = hand.iter().chain(wall).enumerate()
            .map(|(index, face)| ((index + 1).to_string(), json!(face))).collect::<serde_json::Map<_, _>>();
        json!({"source":"packet","level":13001,"node":2,"stage":6,"deck_map":deck,"hand_tiles":(1..=hand.len()).collect::<Vec<_>>(),
            "wall_tiles":(hand.len()+1..=hand.len()+wall.len()).collect::<Vec<_>>(),
            "replacement_tiles":[],"total_change_tile_count":0,"next_operation":[{"type":1}]})
    }

    #[test]
    fn searches_both_supplied_hands_and_chooses_the_earliest() {
        for (hand, wall, name) in [
            (vec!["3p","3p","3p","4p","5p","6p","7p","8p","8p","8p","7p","4s","5s"],
                vec!["6s","2p","2p","2p","7p","6p"], "开局断幺九1"),
            (vec!["2p","2p","2p","3p","4p","5p","6p","7p","7p","7p","6p","4s","5s"],
                vec!["6s","3p","3p","8p","8p","8p","7p"], "开局断幺九2"),
        ] {
            let game = state(&hand, &wall);
            for target in HANDS.iter() {
                let candidate = recommendations::custom_switch_plan(&game, &target["target_groups"], 36, true, &mut |_| {}, &|| false);
                assert_eq!(candidate["status"], "plan", "{candidate}");
            }
            let chosen = plan(&game, &mut |_| {}, &|| false);
            assert_eq!(chosen["hand_name"], name, "{chosen}");
            assert_eq!(chosen["draws_needed"], 1);
            assert_eq!(chosen["search_algorithm"], "custom_target");
        }
    }

    #[test]
    fn preserves_the_wildcard_and_does_not_fall_back_to_other_hands() {
        let game = state(&["3p","3p","3p","4p","5p","6p","7p","8p","8p","8p","9p","4s","5s"], &["6s"]);
        let chosen = plan(&game, &mut |_| {}, &|| false);
        assert_eq!(chosen["hand_name"], "开局断幺九1", "{chosen}");
        assert_eq!(chosen["target_groups"][10]["rule"]["faces"], json!([]));
        assert!(chosen["target_physical_faces"].as_array().unwrap().contains(&json!("9p")));
        let other = state(&["1p","1p","1p","2p","2p","2p","3p","3p","3p","4p","4p","4p","5p"], &["5p"]);
        assert_eq!(plan(&other, &mut |_| {}, &|| false)["reason"], "opening-tanyao-unreachable");
        assert_eq!(plan(&game, &mut |_| {}, &|| true)["reason"], "stopped-by-user");
    }

    #[test]
    fn later_rounds_require_the_fixed_target_by_draw_eighteen_and_otherwise_use_pinzu() {
        let hand = ["3p","3p","3p","4p","5p","6p","7p","8p","8p","8p","7p","4s","5s"];
        for (winning_draw, consumed, fixed) in [(18,0,true),(19,0,false),(2,16,true),(3,16,false),(1,18,false)] {
            let mut wall = vec!["9m"; winning_draw - 1];
            wall.push("6s");
            let mut game = state(&hand, &wall);
            game["node"] = json!(4);
            game["used_desktop_tiles"] = json!((200..200+consumed).collect::<Vec<_>>());
            let chosen = plan(&game, &mut |_| {}, &|| false);
            assert_eq!(chosen["status"], "plan", "{chosen}");
            assert_eq!(chosen["hand_name"] == "开局断幺九1", fixed, "draw {winning_draw}, consumed {consumed}: {chosen}");
            assert_eq!(chosen["opening_score"].is_number(), !fixed);
        }
        let mut wall = vec!["9m"; 18];
        wall.push("6s");
        let chosen = plan(&state(&hand, &wall), &mut |_| {}, &|| false);
        assert_eq!(chosen["hand_name"], "开局断幺九1", "1-1 keeps its existing 36-tile search");
        assert_eq!(chosen["draws_needed"], 19);
    }

    #[test]
    fn later_rounds_can_use_other_winning_shapes_and_recognize_both_sources() {
        let mut game = state(&["1p","1p","1p","2p","2p","2p","3p","3p","3p","4p","4p","4p","5p"], &["5p"]);
        assert!(is_first_battle(&game));
        game["node"] = json!(3);
        let chosen = plan(&game, &mut |_| {}, &|| false);
        assert_eq!(chosen["status"], "plan");
        assert_eq!(chosen["pinzu_count"], 14);
        assert!(chosen["hand_name"].is_null());
        game["source"] = json!("qyzz");
        game["level"] = json!(101);
        game["node"] = json!(0);
        assert!(is_first_battle(&game));
        game["level"] = json!(102);
        assert!(!is_first_battle(&game));
    }
}
