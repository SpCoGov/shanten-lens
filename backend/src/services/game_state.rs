use super::Services;
use crate::pipeline::Packet;
use serde_json::{json, Map, Value};

pub(super) fn empty_game_state() -> Value {
    json!({
        "stage": -1, "session_id": 0, "revision": 0, "flow_id": 0, "deck_map": {}, "hand_tiles": [], "dora_tiles": [], "tian_dora_tiles": [],
        "ming": [], "replacement_tiles": [], "wall_tiles": [], "switch_used_tiles": [], "ended": false,
        "opening_hand_tiles": [], "used_desktop_tiles": [],
        "desktop_remain": 0, "locked_tiles": [], "coin": "0", "point": "0", "target_point": "0",
        "level": 0, "node": 0, "map_nodes": [], "effect_list": [], "candidate_effect_list": [],
        "record": {}, "ting_list": [], "character_id": 0, "hp": 0, "max_hp": 0,
        "next_operation": [], "goods": [], "refresh_price": 0, "change_tile_count": 0,
        "total_change_tile_count": 0, "max_effect_volume": 0, "boss_buff": [], "shop_buff_list": {},
        "tile_score_map": {}, "fan_value_map": {}, "update_reason": []
    })
}

impl Services {
    pub fn game_state(&self) -> Value {
        self.game_state.lock().unwrap().clone()
    }

    pub fn update_game_state(&self, packet: &Packet) {
        self.update_game_state_from_flow(packet, 0);
    }

    pub fn active_flow_id(&self) -> u64 {
        self.game_state.lock().unwrap()["flow_id"]
            .as_u64()
            .unwrap_or(0)
    }

    pub fn disconnect_game_flow(&self, flow_id: u64) {
        let mut sources = self.data_sources.lock().unwrap();
        if sources.packet.as_ref().is_some_and(|state| state["flow_id"].as_u64() == Some(flow_id)) {
            sources.packet = None;
            self.publish_selected_source(&mut sources, None);
        }
    }

    pub fn update_game_state_from_flow(&self, packet: &Packet, flow_id: u64) {
        if packet.direction != crate::pipeline::Direction::Inbound
            || packet.data.get("error").is_some()
        {
            return;
        }
        let fetch = packet.method == ".lq.Lobby.fetchAmuletActivityData";
        let giveup = packet.method == ".lq.Lobby.amuletActivityGiveup";
        let events = packet.data.get("events").and_then(Value::as_array);
        if !fetch && !giveup && events.is_none_or(Vec::is_empty) {
            return;
        }
        let new_game = events.is_some_and(|events| {
            events
                .iter()
                .any(|event| event.pointer("/result/newGameResult").is_some())
        });
        let mut sources = self.data_sources.lock().unwrap();
        let mut packet_state = sources.packet.clone().unwrap_or_else(empty_game_state);
        let target = packet_state.as_object_mut().unwrap();
        let old_flow = target.get("flow_id").and_then(Value::as_u64).unwrap_or(0);
        if old_flow != 0 && old_flow != flow_id && !fetch && !new_game {
            return;
        }
        let previous = target.clone();
        let revision = target.get("revision").and_then(Value::as_u64).unwrap_or(0);
        let session = target
            .get("session_id")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        target.insert("flow_id".into(), json!(flow_id));
        if fetch || new_game || giveup || old_flow != flow_id {
            target.insert("session_id".into(), json!(session + 1));
        }
        if giveup {
            *target = empty_game_state().as_object().unwrap().clone();
            target.insert("session_id".into(), json!(session + 1));
            target.insert("revision".into(), json!(revision + 1));
            target.insert("flow_id".into(), json!(flow_id));
            target.insert("ended".into(), Value::Bool(true));
            target.insert("update_reason".into(), json!([packet.method]));
            sources.packet = Some(Value::Object(target.clone()));
            self.publish_selected_source(&mut sources, Some(crate::ipc::DataSource::Packet));
            return;
        }
        if fetch {
            apply_fetch_state(target, &packet.data);
        }
        if let Some(events) = packet.data.get("events").and_then(Value::as_array) {
            for item in events {
                apply_event_state(target, item);
            }
        }
        if *target == previous {
            return;
        }
        target.insert("revision".into(), json!(revision + 1));
        target.insert("update_reason".into(), json!([packet.method]));
        sources.packet = Some(Value::Object(target.clone()));
        self.publish_selected_source(&mut sources, Some(crate::ipc::DataSource::Packet));
    }
}

fn unwrapped(value: &Value) -> Value {
    value.get("value").cloned().unwrap_or_else(|| value.clone())
}

fn set_known(target: &mut Map<String, Value>, source: &Value, source_key: &str, target_key: &str) {
    if let Some(value) = source.get(source_key) {
        target.insert(target_key.into(), unwrapped(value));
    }
}

pub(super) fn apply_fetch_state(target: &mut Map<String, Value>, response: &Value) {
    let game = response
        .get("data")
        .and_then(|v| v.get("game"))
        .or_else(|| response.get("game"));
    let Some(game) = game else { return };
    target.insert("ended".into(), Value::Bool(false));
    let round = game.get("round").unwrap_or(&Value::Null);
    for (source, dest) in [
        ("hands", "hand_tiles"),
        ("dora", "dora_tiles"),
        ("tianDora", "tian_dora_tiles"),
        ("ming", "ming"),
        ("lockedTile", "locked_tiles"),
        ("used", "switch_used_tiles"),
        ("usedDesktop", "used_desktop_tiles"),
        ("desktopRemain", "desktop_remain"),
        ("tingList", "ting_list"),
        ("nextOperation", "next_operation"),
        ("changeTileCount", "change_tile_count"),
        ("totalChangeTileCount", "total_change_tile_count"),
    ] {
        set_known(target, round, source, dest);
    }
    let effect = game.get("effect").unwrap_or(&Value::Null);
    for (source, dest) in [
        ("effectList", "effect_list"),
        ("packCandidates", "candidate_effect_list"),
        ("maxEffectVolume", "max_effect_volume"),
        ("shopBuffList", "shop_buff_list"),
    ] {
        set_known(target, effect, source, dest);
    }
    let shop = game.get("shop").unwrap_or(&Value::Null);
    set_known(target, shop, "goods", "goods");
    set_known(target, shop, "refreshPrice", "refresh_price");
    let map = game.get("map").unwrap_or(&Value::Null);
    set_known(target, map, "level", "level");
    set_known(target, map, "node", "node");
    set_known(target, map, "mapNodes", "map_nodes");
    if let Some(current) = game.get("state").and_then(|v| v.get("current")) {
        target.insert("stage".into(), current.clone());
    }
    if let Some(enemy) = round.get("enemy") {
        set_known(target, enemy, "hp", "point");
        set_known(target, enemy, "maxHp", "target_point");
    }
    if let Some(character) = game.get("character") {
        set_known(target, character, "characterId", "character_id");
        set_known(target, character, "id", "character_id");
        set_known(target, character, "hp", "hp");
        set_known(target, character, "maxHp", "max_hp");
    }
    if let Some(values) = game.get("game") {
        set_known(target, values, "coin", "coin");
        set_known(target, values, "tileScoreMap", "tile_score_map");
        set_known(target, values, "fanValueMap", "fan_value_map");
    }
    set_known(target, game, "record", "record");
    set_known(target, game, "ended", "ended");
    normalize_maps(target);
    update_boss_buff(target);
    rebuild_pool_sections(target, round, false);
}

fn apply_event_state(target: &mut Map<String, Value>, item: &Value) {
    if let Some(game) = item
        .get("result")
        .and_then(|value| value.get("newGameResult"))
    {
        apply_fetch_state(target, &json!({"data":{"game":game}}));
    }
    if let Some(current) = item
        .get("state")
        .or_else(|| {
            item.get("valueChanges")
                .and_then(|value| value.get("state"))
        })
        .and_then(|value| value.get("current"))
    {
        target.insert("stage".into(), current.clone());
    }
    match item.get("type").and_then(Value::as_u64) {
        Some(1 | 4) => {
            target.insert("ended".into(), Value::Bool(false));
        }
        Some(100) => {
            target.insert("ended".into(), Value::Bool(true));
        }
        _ => {}
    }
    let Some(changes) = item.get("valueChanges") else {
        return;
    };
    if let Some(round) = changes.get("round") {
        for (source, dest) in [
            ("hands", "hand_tiles"),
            ("dora", "dora_tiles"),
            ("tianDora", "tian_dora_tiles"),
            ("ming", "ming"),
            ("lockedTile", "locked_tiles"),
            ("used", "switch_used_tiles"),
            ("usedDesktop", "used_desktop_tiles"),
            ("desktopRemain", "desktop_remain"),
            ("tingList", "ting_list"),
            ("nextOperation", "next_operation"),
            ("changeTileCount", "change_tile_count"),
            ("totalChangeTileCount", "total_change_tile_count"),
        ] {
            set_known(target, round, source, dest);
        }
        if let Some(enemy) = round.get("enemy").map(unwrapped) {
            set_known(target, &enemy, "hp", "point");
            set_known(target, &enemy, "maxHp", "target_point");
        }
    }
    if let Some(game) = changes.get("game") {
        for (source, dest) in [
            ("coin", "coin"),
            ("maxEffectVolume", "max_effect_volume"),
            ("tileScoreMap", "tile_score_map"),
            ("fanValueMap", "fan_value_map"),
        ] {
            set_known(target, game, source, dest);
        }
    }
    if let Some(character) = changes.get("character") {
        for (source, dest) in [
            ("characterId", "character_id"),
            ("hp", "hp"),
            ("maxHp", "max_hp"),
        ] {
            set_known(target, character, source, dest);
        }
    }
    if let Some(effect) = changes.get("effect") {
        for (source, dest) in [
            ("effectList", "effect_list"),
            ("packCandidates", "candidate_effect_list"),
            ("shopBuffList", "shop_buff_list"),
        ] {
            set_known(target, effect, source, dest);
        }
    }
    if let Some(shop) = changes.get("shop") {
        set_known(target, shop, "goods", "goods");
        set_known(target, shop, "refreshPrice", "refresh_price");
    }
    if let Some(map) = changes.get("map") {
        for (source, dest) in [
            ("level", "level"),
            ("node", "node"),
            ("mapNodes", "map_nodes"),
        ] {
            set_known(target, map, source, dest);
        }
    }
    if let Some(record) = changes.get("record") {
        merge_record(target, record)
    }
    set_known(target, changes, "ended", "ended");
    normalize_maps(target);
    update_boss_buff(target);
    if let Some(round) = changes.get("round") {
        let event_type = item.get("type").and_then(Value::as_i64).unwrap_or(-1);
        if event_type == 13 {
            rebuild_redeal_wall(target, round);
        } else {
            rebuild_pool_sections(target, round, event_type == 4);
            if event_type == 10 {
                remove_drawn_tile_from_wall(target);
            }
        }
    }
}

pub(super) fn rebuild_pool_sections(
    target: &mut Map<String, Value>,
    round: &Value,
    initial_event: bool,
) {
    let Some(pool_value) = round.get("pool").map(unwrapped) else {
        return;
    };
    let Some(pool) = pool_value.as_array() else {
        return;
    };
    let ids = pool
        .iter()
        .filter_map(|tile| tile.get("id").and_then(Value::as_u64))
        .collect::<Vec<_>>();
    let deck = pool
        .iter()
        .filter_map(|tile| Some((tile.get("id")?.to_string(), tile.get("tile")?.clone())))
        .collect::<Map<_, _>>();
    target.insert("deck_map".into(), Value::Object(deck));

    let hand = ints_from_state(target, "hand_tiles");
    let dora_hint = round
        .get("dora")
        .map(unwrapped)
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_else(|| {
            target
                .get("dora_tiles")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
        });
    let has_current_hand_sections_effect = target
        .get("effect_list")
        .and_then(Value::as_array)
        .is_some_and(|effects| {
            effects.iter().any(|effect| {
                let id = effect.get("id").and_then(Value::as_u64).unwrap_or(0);
                matches!(id, 2250 | 2251)
                    || (id == 2280
                        && effect
                            .get("store")
                            .and_then(Value::as_array)
                            .and_then(|store| store.first())
                            .and_then(value_u64)
                            .is_some_and(|source| matches!(source, 2250 | 2251)))
            })
        });
    let use_current_hand = has_current_hand_sections_effect
        && (initial_event
            || (target.get("stage").and_then(Value::as_i64) == Some(2)
                && target
                    .get("change_tile_count")
                    .and_then(Value::as_u64)
                    .unwrap_or(0)
                    == 0));
    let opening = if use_current_hand {
        hand.clone()
    } else if let Some(first) = dora_hint.first().and_then(Value::as_u64) {
        ids.iter()
            .position(|id| *id == first)
            .map(|index| {
                let mut opening = ids[..index].to_vec();
                if let Some(last) = pool.last() {
                    let last_id = last.get("id").and_then(Value::as_u64).unwrap_or(0);
                    if last.get("tile").and_then(Value::as_str) == Some("bd")
                        && hand.contains(&last_id)
                        && !opening.contains(&last_id)
                    {
                        opening.push(last_id);
                    }
                }
                opening
            })
            .unwrap_or_else(|| hand.clone())
    } else {
        hand.clone()
    };
    target.insert("opening_hand_tiles".into(), json!(opening));
    let excluded = opening
        .into_iter()
        .collect::<std::collections::HashSet<_>>();
    let remaining = ids
        .into_iter()
        .filter(|id| !excluded.contains(id))
        .collect::<Vec<_>>();
    let wall_end = if ints_from_state(target, "boss_buff").contains(&926) {
        28
    } else {
        46
    };
    target.insert(
        "dora_tiles".into(),
        json!(remaining.iter().take(10).copied().collect::<Vec<_>>()),
    );
    let blocked = ints_from_state(target, "locked_tiles")
        .into_iter()
        .chain(ints_from_state(target, "used_desktop_tiles"))
        .collect::<std::collections::HashSet<_>>();
    let mut wall = remaining
        .iter()
        .skip(10)
        .take(wall_end - 10)
        .copied()
        .filter(|id| !blocked.contains(id))
        .collect::<Vec<_>>();
    reorder_wall_for_effects(target, &mut wall);
    trim_wall(target, &mut wall);
    target.insert("wall_tiles".into(), json!(wall));
    target.insert(
        "replacement_tiles".into(),
        json!(remaining.iter().skip(wall_end).copied().collect::<Vec<_>>()),
    );
}

fn rebuild_redeal_wall(target: &mut Map<String, Value>, round: &Value) {
    let Some(pool) = round
        .get("pool")
        .map(unwrapped)
        .and_then(|value| value.as_array().cloned())
    else {
        return;
    };
    target.insert(
        "deck_map".into(),
        Value::Object(
            pool.iter()
                .filter_map(|tile| Some((tile.get("id")?.to_string(), tile.get("tile")?.clone())))
                .collect(),
        ),
    );
    let mut excluded = ints_from_state(target, "hand_tiles")
        .into_iter()
        .chain(ints_from_state(target, "dora_tiles"))
        .collect::<std::collections::HashSet<_>>();
    for meld in target
        .get("ming")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(tiles) = meld.get("tileList").and_then(Value::as_array) {
            excluded.extend(tiles.iter().filter_map(Value::as_u64));
        }
    }
    let limit = target
        .get("desktop_remain")
        .and_then(Value::as_u64)
        .filter(|value| *value > 0)
        .unwrap_or(9)
        .min(9) as usize;
    let blocked = ints_from_state(target, "locked_tiles")
        .into_iter()
        .chain(ints_from_state(target, "used_desktop_tiles"))
        .collect::<std::collections::HashSet<_>>();
    let wall = pool
        .iter()
        .filter_map(|tile| tile.get("id").and_then(Value::as_u64))
        .filter(|id| !excluded.contains(id) && !blocked.contains(id))
        .take(limit)
        .collect::<Vec<_>>();
    target.insert("wall_tiles".into(), json!(wall));
}

fn ints_from_state(target: &Map<String, Value>, key: &str) -> Vec<u64> {
    target
        .get(key)
        .and_then(Value::as_array)
        .map(|values| values.iter().filter_map(Value::as_u64).collect())
        .unwrap_or_default()
}

fn trim_wall(target: &Map<String, Value>, wall: &mut Vec<u64>) {
    let remain = target
        .get("desktop_remain")
        .and_then(Value::as_u64)
        .unwrap_or(wall.len() as u64) as usize;
    if wall.len() > remain {
        *wall = wall.split_off(wall.len() - remain);
    }
}

fn remove_drawn_tile_from_wall(target: &mut Map<String, Value>) {
    let Some(drawn) = target
        .get("hand_tiles")
        .and_then(Value::as_array)
        .and_then(|hand| hand.last())
        .and_then(Value::as_u64)
    else {
        return;
    };
    if let Some(wall) = target.get_mut("wall_tiles").and_then(Value::as_array_mut) {
        if let Some(index) = wall.iter().position(|id| id.as_u64() == Some(drawn)) {
            wall.remove(index);
        }
    }
}

fn reorder_wall_for_effects(target: &Map<String, Value>, wall: &mut [u64]) {
    let has_sort = target
        .get("effect_list")
        .and_then(Value::as_array)
        .is_some_and(|effects| {
            effects.iter().any(|effect| {
                let id = effect.get("id").and_then(Value::as_u64).unwrap_or(0);
                id / 10 == 221
                    || (id == 2280
                        && effect
                            .get("store")
                            .and_then(Value::as_array)
                            .and_then(|store| store.first())
                            .and_then(value_u64)
                            .is_some_and(|source| source / 10 == 221))
            })
        });
    if !has_sort {
        return;
    }
    let deck = target.get("deck_map").and_then(Value::as_object);
    wall.sort_by_key(|id| {
        let face = deck
            .and_then(|deck| deck.get(&id.to_string()))
            .and_then(Value::as_str)
            .unwrap_or("");
        tile_order(face)
    });
}

fn tile_order(face: &str) -> usize {
    let normalized = match face {
        "0m" => "5m",
        "0p" => "5p",
        "0s" => "5s",
        value => value,
    };
    let bytes = normalized.as_bytes();
    if bytes.len() != 2 {
        return usize::MAX;
    }
    let rank = bytes[0].saturating_sub(b'0') as usize;
    match bytes[1] {
        b'm' => rank,
        b'p' => 10 + rank,
        b's' => 20 + rank,
        b'z' => 30 + rank,
        _ => usize::MAX,
    }
}

fn value_u64(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str()?.parse::<u64>().ok())
}

fn merge_record(target: &mut Map<String, Value>, record: &Value) {
    let value = unwrapped(record);
    let Some(source) = value.as_object() else {
        return;
    };
    let is_patch = source
        .values()
        .any(|item| item.get("dirty").is_some() && item.get("value").is_some());
    if !is_patch {
        target.insert("record".into(), value);
        return;
    }
    let record = target.entry("record").or_insert_with(|| json!({}));
    let Some(current) = record.as_object_mut() else {
        return;
    };
    for (key, item) in source {
        if item.get("dirty").and_then(Value::as_bool) == Some(true) {
            current.insert(
                key.clone(),
                item.get("value").cloned().unwrap_or(Value::Null),
            );
        }
    }
}

fn normalize_maps(target: &mut Map<String, Value>) {
    if let Some(items) = target.get("tile_score_map").and_then(Value::as_array) {
        target.insert(
            "tile_score_map".into(),
            Value::Object(
                items
                    .iter()
                    .filter_map(|item| {
                        Some((
                            item.get("tile")?.as_str()?.to_owned(),
                            Value::String(
                                item.get("score")?.to_string().trim_matches('"').to_owned(),
                            ),
                        ))
                    })
                    .collect(),
            ),
        );
    }
    if let Some(items) = target.get("fan_value_map").and_then(Value::as_array) {
        target.insert(
            "fan_value_map".into(),
            Value::Object(
                items
                    .iter()
                    .filter_map(|item| {
                        Some((
                            item.get("id")?.to_string().trim_matches('"').to_owned(),
                            Value::String(
                                item.get("value")?.to_string().trim_matches('"').to_owned(),
                            ),
                        ))
                    })
                    .collect(),
            ),
        );
    }
    if let Some(items) = target.get("shop_buff_list").and_then(Value::as_array) {
        target.insert(
            "shop_buff_list".into(),
            Value::Object(
                items
                    .iter()
                    .filter_map(|item| {
                        Some((
                            item.get("id")?.to_string().trim_matches('"').to_owned(),
                            json!(item
                                .get("store")
                                .and_then(Value::as_array)
                                .and_then(|store| store.first())
                                .and_then(|value| {
                                    value.as_u64().or_else(|| value.as_str()?.parse().ok())
                                })
                                .unwrap_or(0)),
                        ))
                    })
                    .collect(),
            ),
        );
    }
}

fn update_boss_buff(target: &mut Map<String, Value>) {
    let node = target.get("node").and_then(Value::as_u64).unwrap_or(0) as usize;
    let buffs = target
        .get("map_nodes")
        .and_then(Value::as_array)
        .and_then(|nodes| node.checked_sub(1).and_then(|index| nodes.get(index)))
        .and_then(|item| item.get("args"))
        .cloned()
        .unwrap_or_else(|| json!([]));
    target.insert("boss_buff".into(), buffs);
}
