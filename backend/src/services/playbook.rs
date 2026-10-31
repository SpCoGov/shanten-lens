use serde_json::Value;
use std::{collections::HashSet, sync::LazyLock};

static LEVELS: LazyLock<Vec<(u64, u64)>> = LazyLock::new(|| {
    let catalog: Value = serde_json::from_str(include_str!("../../assets/levels.json")).unwrap();
    let levels = catalog["levels"].as_array().unwrap();
    let activity = &levels.last().unwrap()["activity_id"];
    levels.iter().filter(|level| &level["activity_id"] == activity).flat_map(|level| {
        level["nodes"].as_array().unwrap().iter().filter(|node| node["type"] != 1)
            .map(|node| (level["level"].as_u64().unwrap(), node["node"].as_u64().unwrap()))
    }).collect()
});
// Local IPC uses one stage per level and node 0, separate from packet map nodes.
pub(crate) static QYZZ_LEVELS: LazyLock<Vec<(u64, u64)>> = LazyLock::new(|| {
    let catalog: Value = serde_json::from_str(include_str!("../../../app/src/lib/qyzzStageCatalog.json")).unwrap();
    catalog["levels"].as_array().unwrap().iter().map(|stage| (stage["id"].as_u64().unwrap(), 0)).collect()
});
static AMULETS: LazyLock<Value> = LazyLock::new(|| serde_json::from_str(super::registry::AMULETS).unwrap());
static QYZZ_AMULETS: LazyLock<Value> = LazyLock::new(|| serde_json::from_str(include_str!("../../assets/qyzz_amulets.json")).unwrap());
static BADGES: LazyLock<Value> = LazyLock::new(|| serde_json::from_str(super::registry::BADGES).unwrap());

fn name(value: &Value) -> bool {
    value.as_str().is_some_and(|s| !s.trim().is_empty() && s.chars().count() <= 80)
}

fn rows<'a>(value: &'a Value, min: usize, max: usize, key: &str) -> Result<&'a Vec<Value>, String> {
    let values = value.as_array().filter(|v| (min..=max).contains(&v.len())).ok_or("invalid playbook list")?;
    let mut ids = HashSet::new();
    if values.iter().any(|v| !v.is_object() || !name(&v[key]) || !ids.insert(v[key].as_str().unwrap())) {
        return Err("invalid or duplicate playbook id".into());
    }
    Ok(values)
}

fn known_amulet(id: &Value, initial: bool) -> bool {
    let Some(id) = id.as_u64().filter(|id| *id > 0 && *id <= 9_007_199_254_740_991) else { return false; };
    QYZZ_AMULETS["amulets"].as_array().unwrap().iter().any(|a| a["data_id"].as_u64().map(|v| v / 10) == Some(id)
        && (!initial || matches!(a["rarity"].as_u64(), Some(3 | 4))))
        || AMULETS["items"].as_array().unwrap().iter().any(|a| a["id"] == id
            && (!initial || (matches!(a["rarity"].as_str(), Some("BLUE" | "GREEN")) && a["metadata"]["deprecated"] != true)))
}

fn positive_integer(value: &Value) -> bool {
    value.as_u64().is_some_and(|n| (1..=9_007_199_254_740_991).contains(&n))
}

fn known_badge(id: &Value) -> bool {
    positive_integer(id) && BADGES["items"].as_array().unwrap().iter().any(|b| b["id"] == *id)
}

pub(crate) fn validate(value: &Value) -> Result<(), String> {
    validate_inner(value, false)
}

pub(super) fn validate_for_editing(value: &Value) -> Result<(), String> {
    // Keep stale level references editable; saving and execution still require validate().
    validate_inner(value, true)
}

fn validate_inner(value: &Value, allow_invalid_levels: bool) -> Result<(), String> {
    if value["version"] != 1 { return Err("unsupported playbook version".into()); }
    let profiles = rows(&value["profiles"], 0, 32, "id")?;
    if !value["selected_profile_id"].is_string()
        || (profiles.is_empty() && value["selected_profile_id"] != "")
        || (!profiles.is_empty() && !profiles.iter().any(|p| p["id"] == value["selected_profile_id"])) {
        return Err("invalid selected playbook profile".into());
    }
    for profile in profiles {
        let node = if profile["strategy"] == "opening" { &profile["cutoff_node"] } else { &profile["stages"][0]["start_node"] };
        let levels = if node == 0 { &QYZZ_LEVELS } else { &LEVELS };
        if !name(&profile["name"]) { return Err("invalid playbook profile name".into()); }
        if profile["strategy"] == "opening" {
            if !positive_integer(&profile["end_count"]) { return Err("invalid opening target value".into()); }
            let valid_cutoff = levels.iter().any(|(level, node)| profile["cutoff_level"] == *level && profile["cutoff_node"] == *node);
            if !allow_invalid_levels && !valid_cutoff {
                return Err("invalid opening cutoff level".into());
            }
            let targets = profile["targets"].as_array().filter(|items| (1..=128).contains(&items.len())).ok_or("invalid opening targets")?;
            for target in targets {
                if target.get("value").is_some_and(|v| !positive_integer(v)) {
                    return Err("invalid opening target value".into());
                }
                let valid = match target["kind"].as_str() {
                    Some("amulet") => known_amulet(&target["id"], false)
                        && target.get("plus").is_none_or(Value::is_boolean)
                        && (target["badge"].is_null() || known_badge(&target["badge"])),
                    Some("badge") => known_badge(&target["id"]),
                    _ => false,
                };
                if !valid { return Err("invalid opening amulet or badge target".into()); }
            }
            continue;
        }
        if profile.get("strategy").is_some_and(|v| v != "full") { return Err("invalid playbook strategy".into()); }
        if !matches!(profile["character_id"].as_u64(), Some(201..=204))
            || !known_amulet(&profile["initial_amulet_id"], true) {
            return Err("invalid playbook opening character or amulet (blue/green only)".into());
        }
        let coin = profile["first_level_min_coin"].as_str().unwrap_or("");
        if coin.is_empty() || coin.len() > 128 || !coin.bytes().all(|b| b.is_ascii_digit()) || (coin.len() > 1 && coin.starts_with('0')) {
            return Err("invalid first-level coin deadline".into());
        }
        let mut previous_start = None;
        for stage in rows(&profile["stages"], 1, 32, "id")? {
            let start = levels.iter().skip(1).position(|(level, node)| stage["start_level"] == *level && stage["start_node"] == *node);
            if !name(&stage["name"]) || (!allow_invalid_levels && (start.is_none() || previous_start.is_some_and(|previous| start.unwrap() <= previous)
                || !levels.iter().skip(1).any(|(level, node)| stage["deadline_level"] == *level && stage["deadline_node"] == *node)))
                || !stage["enter_early_if_unreachable"].is_boolean() {
                return Err("invalid playbook stage levels or order".into());
            }
            previous_start = start;
            for loadout in rows(&stage["loadouts"], 1, 16, "id")? {
                if !name(&loadout["name"]) { return Err("invalid loadout name".into()); }
                let items = rows(&loadout["amulets"], 1, 32, "key")?;
                for item in items {
                    if !known_amulet(&item["id"], false) || !item["plus"].is_boolean()
                        || !(item["badge"].is_null() || item["badge"] == 0
                            || BADGES["items"].as_array().unwrap().iter().any(|b| b["id"] == item["badge"])) {
                        return Err("invalid loadout amulet or badge".into());
                    }
                }
                let keys: HashSet<_> = items.iter().map(|item| item["key"].as_str().unwrap()).collect();
                for field in ["entry_order", "win_order"] {
                    let order = loadout[field].as_array().ok_or("missing amulet order")?;
                    let ordered: HashSet<_> = order.iter().filter_map(Value::as_str).collect();
                    if order.len() != keys.len() || ordered != keys { return Err("amulet order must contain every slot exactly once".into()); }
                }
            }
            if !matches!(stage["hand_selection"].as_str(), Some("most_wins" | "priority")) { return Err("invalid hand selection".into()); }
            for hand in rows(&stage["hands"], 1, 32, "id")? {
                if !name(&hand["name"]) || !hand["exclude_invalid_shapes"].is_boolean()
                    || !crate::recommendations::valid_custom_target(&hand["target_groups"]) {
                    return Err("invalid playbook target hand".into());
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn playbook_rejects_invalid_openings_orders_deadlines_and_hands() {
        let valid: Value = serde_json::from_str(include_str!("../../tests/fixtures/playbook.json")).unwrap();
        assert!(validate(&valid).is_ok());
        for (pointer, replacement) in [
            ("/version", json!(2)),
            ("/selected_profile_id", json!("missing")),
            ("/profiles/0/character_id", json!(999)),
            ("/profiles/0/initial_amulet_id", json!(230)),
            ("/profiles/0/first_level_min_coin", json!(900)),
            ("/profiles/0/first_level_min_coin", json!("-1")),
            ("/profiles/0/first_level_min_coin", json!("01")),
            ("/profiles/0/stages/0/deadline_level", json!(999)),
            ("/profiles/0/stages/0/deadline_node", json!(99)),
            ("/profiles/0/stages/0/start_node", json!(1)),
            ("/profiles/0/stages/1/start_level", json!(102)),
            ("/profiles/0/stages/0/loadouts/0/entry_order", json!(["a", "a"])),
            ("/profiles/0/stages/0/loadouts/0/win_order", json!(["b"])),
            ("/profiles/0/stages/0/loadouts/0/amulets/0/id", json!(999999)),
            ("/profiles/0/stages/0/loadouts/0/amulets/0/badge", json!(-1)),
            ("/profiles/0/stages/0/hands/0/target_groups/0/rule/faces", json!(["10m"])),
            ("/profiles/0/stages/0/hands/0/target_groups/0/kind", json!("single")),
            ("/profiles/0/stages/0/hand_selection", json!("random")),
            ("/profiles/1/strategy", json!("unknown")),
            ("/profiles/1/strategy", Value::Null),
            ("/profiles/1/end_count", json!(0)),
            ("/profiles/1/end_count", json!(9007199254740992_u64)),
            ("/profiles/1/targets", json!([])),
            ("/profiles/1/targets/0/value", json!(1.5)),
            ("/profiles/1/targets/0/value", json!(-1)),
            ("/profiles/1/targets/0/value", Value::Null),
            ("/profiles/1/targets/0/plus", json!(1)),
            ("/profiles/1/targets/0/badge", json!(999999)),
            ("/profiles/1/targets/1/id", json!(999999)),
            ("/profiles/1/targets/1/kind", json!("unknown")),
            ("/profiles/1/cutoff_level", json!(999)),
            ("/profiles/1/cutoff_level", json!(203)),
            ("/profiles/1/cutoff_level", Value::Null),
            ("/profiles/1/cutoff_node", json!(1)),
            ("/profiles/1/cutoff_node", json!(99)),
            ("/profiles/1/cutoff_node", Value::Null),
        ] {
            let mut invalid = valid.clone();
            *invalid.pointer_mut(pointer).unwrap() = replacement;
            assert!(validate(&invalid).is_err(), "accepted invalid field: {pointer}");
            if !pointer.ends_with("_level") && !pointer.ends_with("_node") {
                assert!(validate_for_editing(&invalid).is_err(), "editing bypassed validation of {pointer}");
            }
        }
    }

    #[test]
    fn playbook_requires_opening_cutoff_and_accepts_default_target_values() {
        let mut book: Value = serde_json::from_str(include_str!("../../tests/fixtures/playbook.json")).unwrap();
        book["profiles"][0]["strategy"] = json!("full");
        book["profiles"][1]["targets"] = json!([{"kind":"amulet","id":1},{"kind":"badge","id":600010}]);
        book["profiles"][1]["cutoff_level"] = json!(0);
        book["profiles"][1]["cutoff_node"] = json!(0);
        assert!(validate(&book).is_err());
        assert!(validate_for_editing(&book).is_ok());
        for (level, node) in [(13001, 2), (13001, 3), (13001, 6), (14001, 3)] {
            book["profiles"][1]["cutoff_level"] = json!(level);
            book["profiles"][1]["cutoff_node"] = json!(node);
            assert!(validate(&book).is_ok());
        }
    }

    #[test]
    fn playbook_orders_stages_by_map_and_node() {
        let mut book: Value = serde_json::from_str(include_str!("../../tests/fixtures/playbook.json")).unwrap();
        book["profiles"][0]["stages"][1]["start_level"] = json!(13001);
        book["profiles"][0]["stages"][1]["start_node"] = json!(4);
        assert!(validate(&book).is_ok());
        book["profiles"][0]["stages"][1]["start_node"] = json!(3);
        assert!(validate(&book).is_err());
        book["profiles"][0]["stages"][1]["start_node"] = json!(2);
        assert!(validate(&book).is_err());
    }
    #[test]
    fn native_stage_catalog_drives_validation_and_tactical_boundaries() {
        assert_eq!(QYZZ_LEVELS.len(), 314);
        assert_eq!(QYZZ_LEVELS.first(), Some(&(101, 0)));
        assert_eq!(QYZZ_LEVELS.last(), Some(&(1299, 0)));
        let mut book: Value = serde_json::from_str(include_str!("../../tests/fixtures/playbook.json")).unwrap();
        for (index, stage) in book["profiles"][0]["stages"].as_array_mut().unwrap().iter_mut().enumerate() {
            stage["start_level"] = json!(if index == 0 {102} else {201});
            stage["start_node"] = json!(0);
            stage["deadline_level"] = json!(1299);
            stage["deadline_node"] = json!(0);
        }
        book["profiles"][1]["cutoff_level"] = json!(1299);
        book["profiles"][1]["cutoff_node"] = json!(0);
        assert!(validate(&book).is_ok());
        let profile = &book["profiles"][0];
        for (level, expected) in [(101, None), (102, Some("stage-0")), (103, Some("stage-0")), (201, Some("stage-1")), (1299, Some("stage-1"))] {
            let state = json!({"source":"qyzz","stage":5,"level":level,"node":0});
            assert_eq!(crate::decision::tactical_stage(&state, profile).unwrap().and_then(|stage| stage["id"].as_str()), expected);
        }
        for state in [json!({"source":"qyzz","stage":5,"level":13001,"node":3}),
            json!({"source":"packet","stage":5,"level":102,"node":0})] {
            assert!(crate::decision::tactical_stage(&state, profile).is_err(), "mixed stage source: {state}");
        }
        book["profiles"][1]["cutoff_level"] = json!(1300);
        assert!(validate(&book).is_err());
        book["profiles"][1]["cutoff_level"] = json!(1299);
        book["profiles"][0]["stages"][1]["start_level"] = json!(13001);
        book["profiles"][0]["stages"][1]["start_node"] = json!(3);
        assert!(validate(&book).is_err());
    }

}
