use crate::{
    ipc::{PacketLogItem, PacketLogSnapshot},
    pipeline::Packet,
};
use serde_json::{json, Map, Value};
use std::{
    collections::VecDeque,
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{broadcast, oneshot};

const AMULETS: &str = include_str!("../assets/amulets.json");
const BADGES: &str = include_str!("../assets/badges.json");

fn defaults() -> Value {
    json!({
        "game": {"modify_announcement": true, "public_all": false, "unlock_illustrated_book": false},
        "general": {"debug": false, "error_code_test": 0},
        "backend": {"mitm_port": 10999, "enable_upstream_proxy": false, "upstream_proxy": ""},
        "fuse": {
            "guard_skip_contains": {"amulets": [], "badges": []}, "enable_skip_guard": true,
            "enable_shop_force_pick": false, "enable_ting_ready_skip_guard": true,
            "enable_prestart_kavi_guard": true, "conduction_min_count": 3,
            "enable_anti_steal_eat": true, "enable_missing_hand_tile_guard": true,
            "enable_kavi_plus_buffer_guard": true, "enable_hanabi_win_guard": true,
            "enable_exit_coin_guard": true, "enable_exit_life_guard": false
        },
        "autorun": {
            "end_count": 1, "targets": [], "cutoff_level": 0, "op_interval_ms": 1000,
            "need_pionner_badge_count": 4, "record_detailed_operations": false,
            "email_notify": {"enabled": false, "host": "", "port": 587, "ssl": false, "from": "", "pass": "", "to": ""}
        }
    })
}

fn empty_game_state() -> Value {
    json!({
        "stage": -1, "deck_map": {}, "hand_tiles": [], "dora_tiles": [], "tian_dora_tiles": [],
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

#[derive(Clone)]
pub struct Services {
    root: PathBuf,
    config: Arc<Mutex<Value>>,
    locale: Arc<Mutex<String>>,
    game_state: Arc<Mutex<Value>>,
    packet_log: Arc<Mutex<VecDeque<PacketLogItem>>>,
    registry: Arc<Mutex<Value>>,
    waiters: Arc<Mutex<std::collections::HashMap<u32, oneshot::Sender<Value>>>>,
    reserved_response_ids: Arc<Mutex<std::collections::HashSet<u32>>>,
    confirmations: Arc<Mutex<std::collections::HashMap<String, mpsc::Sender<bool>>>>,
    next_confirmation: Arc<AtomicU64>,
    pub events: broadcast::Sender<Value>,
}

impl Services {
    pub fn load(root: PathBuf, events: broadcast::Sender<Value>) -> anyhow::Result<Self> {
        fs::create_dir_all(&root)?;
        let mut config = defaults();
        for name in ["game", "general", "backend", "fuse", "autorun"] {
            let path = root.join(format!("{name}.json"));
            let registered = config[name].clone();
            let (loaded, need_write) = load_config_table(&path, &registered, &registered);
            config[name] = loaded;
            if need_write {
                write_json(&path, &config[name])?;
            }
        }
        let data_dir = root.parent().unwrap_or(&root).join("data");
        let registry = json!({
            "amulets": registry_items(&data_dir.join("amulets.json"), AMULETS, "amulets"),
            "badges": registry_items(&data_dir.join("badges.json"), BADGES, "badges"),
        });
        Ok(Self {
            root,
            config: Arc::new(Mutex::new(config)),
            locale: Arc::new(Mutex::new("zh-CN".into())),
            game_state: Arc::new(Mutex::new(empty_game_state())),
            packet_log: Arc::new(Mutex::new(VecDeque::new())),
            registry: Arc::new(Mutex::new(registry)),
            waiters: Arc::new(Mutex::new(std::collections::HashMap::new())),
            reserved_response_ids: Arc::new(Mutex::new(std::collections::HashSet::new())),
            confirmations: Arc::new(Mutex::new(std::collections::HashMap::new())),
            next_confirmation: Arc::new(AtomicU64::new(1)),
            events,
        })
    }

    pub fn config_payload(&self) -> Value {
        let config = self.config.lock().unwrap();
        json!({"game": config["game"], "general": config["general"], "backend": config["backend"]})
    }
    pub fn table(&self, name: &str) -> Value {
        self.config
            .lock()
            .unwrap()
            .get(name)
            .cloned()
            .unwrap_or_else(|| json!({}))
    }
    pub fn registry(&self) -> Value {
        self.registry.lock().unwrap().clone()
    }
    pub fn config_root(&self) -> &Path {
        &self.root
    }
    pub fn set_locale(&self, locale: &str) {
        *self.locale.lock().unwrap() = if locale.starts_with("ja") {
            "ja-JP".into()
        } else {
            "zh-CN".into()
        };
    }
    pub fn startup_announcement(&self) -> Value {
        if self.locale.lock().unwrap().starts_with("ja") {
            json!({
                "id": 9999,
                "title": "向聴レンズへようこそ",
                "content": "向聴レンズが起動しました！みなさんにガチャ運がモリモリ湧いてきますように！",
                "headerImage": "internal://2.jpg"
            })
        } else {
            json!({
                "id": 9999,
                "title": "欢迎使用向听镜",
                "content": "向听镜已启动，祝各位大大欧气满满！",
                "headerImage": "internal://2.jpg"
            })
        }
    }
    pub fn game_state(&self) -> Value {
        self.game_state.lock().unwrap().clone()
    }

    pub fn patch_config(&self, patch: &Value) -> Result<(), String> {
        let Some(tables) = patch.as_object() else {
            return Err("config patch must be an object".into());
        };
        let mut config = self.config.lock().unwrap();
        for (name, partial) in tables {
            let Some(values) = partial.as_object() else {
                return Err(format!("config table {name} must be an object"));
            };
            let table = config
                .as_object_mut()
                .unwrap()
                .entry(name)
                .or_insert_with(|| json!({}));
            let Some(table) = table.as_object_mut() else {
                return Err(format!("config table {name} is invalid"));
            };
            table.extend(values.clone());
            write_json(
                &self.root.join(format!("{name}.json")),
                &Value::Object(table.clone()),
            )
            .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub fn reload_files(&self) {
        let mut changed_normal = false;
        let mut changed_fuse = false;
        let mut changed_autorun = false;
        {
            let mut config = self.config.lock().unwrap();
            let default_config = defaults();
            for name in ["game", "general", "backend", "fuse", "autorun"] {
                let path = self.root.join(format!("{name}.json"));
                let current = config[name].clone();
                let (loaded, need_write) =
                    load_config_table(&path, &default_config[name], &current);
                if current != loaded || need_write {
                    config[name] = loaded;
                    match name {
                        "fuse" => changed_fuse = true,
                        "autorun" => changed_autorun = true,
                        _ => changed_normal = true,
                    }
                }
                if need_write {
                    let _ = write_json(&path, &config[name]);
                }
            }
        }
        if changed_fuse {
            let _ = self
                .events
                .send(event("update_fuse_config", self.table("fuse")));
        }
        if changed_autorun {
            let _ = self
                .events
                .send(event("update_autorun_config", self.table("autorun")));
        }
        if changed_normal {
            let _ = self
                .events
                .send(event("update_config", self.config_payload()));
        }
        let data_dir = self.root.parent().unwrap_or(&self.root).join("data");
        let registry = json!({
            "amulets": registry_items(&data_dir.join("amulets.json"), AMULETS, "amulets"),
            "badges": registry_items(&data_dir.join("badges.json"), BADGES, "badges"),
        });
        let mut current = self.registry.lock().unwrap();
        if *current != registry {
            *current = registry.clone();
            drop(current);
            let _ = self.events.send(event("update_registry", registry));
        }
    }

    pub fn resolve_response(&self, packet: &Packet) {
        if packet.packet_type == "Res" {
            if let Some(id) = packet.id {
                self.reserved_response_ids.lock().unwrap().remove(&id);
            }
            if let Some(sender) = packet
                .id
                .and_then(|id| self.waiters.lock().unwrap().remove(&id))
            {
                let _ = sender.send(packet.data.clone());
            }
        }
    }

    pub fn record_packet(&self, packet: &Packet) {
        let value = PacketLogItem {
            direction: packet.direction,
            packet_type: packet.packet_type.clone(),
            method: packet.method.clone(),
            id: packet.id,
            data: packet.data.clone(),
            ts_ms: now_ms(),
        };
        let mut packets = self.packet_log.lock().unwrap();
        if packets.len() == 1_000 {
            packets.pop_front();
        }
        packets.push_back(value.clone());
        drop(packets);
        let _ = self.events.send(event(
            "packet_log_event",
            serde_json::to_value(value).unwrap_or(Value::Null),
        ));
    }

    pub fn packet_log_snapshot(&self) -> PacketLogSnapshot {
        PacketLogSnapshot {
            packets: self.packet_log.lock().unwrap().iter().cloned().collect(),
        }
    }

    pub fn can_replay(&self, method: &str) -> bool {
        self.packet_log.lock().unwrap().iter().any(|packet| {
            packet.direction == crate::pipeline::Direction::Outbound
                && packet.packet_type == "Req"
                && packet.method == method
        })
    }

    pub fn try_response_waiter(&self, id: u32) -> Option<oneshot::Receiver<Value>> {
        let mut reserved = self.reserved_response_ids.lock().unwrap();
        if !reserved.insert(id) {
            return None;
        }
        let (sender, receiver) = oneshot::channel();
        self.waiters.lock().unwrap().insert(id, sender);
        Some(receiver)
    }

    pub fn reserve_response_id(&self, id: u32) -> bool {
        self.reserved_response_ids.lock().unwrap().insert(id)
    }

    pub fn cancel_waiter(&self, id: u32) {
        self.waiters.lock().unwrap().remove(&id);
        self.reserved_response_ids.lock().unwrap().remove(&id);
    }

    pub fn confirm(&self, title: &str, message: &str, values: Value) -> bool {
        let id = format!(
            "rust-{:016x}",
            self.next_confirmation.fetch_add(1, Ordering::Relaxed)
        );
        let (sender, receiver) = mpsc::channel();
        self.confirmations
            .lock()
            .unwrap()
            .insert(id.clone(), sender);
        let _ = self.events.send(event("msgbox", json!({
            "id":id,"title":title,"message":message,"okText":"common.continue","cancelText":"common.cancel","values":values
        })));
        let answer = receiver
            .recv_timeout(Duration::from_secs(45))
            .unwrap_or(false);
        self.confirmations.lock().unwrap().remove(&id);
        answer
    }

    pub fn resolve_confirmation(&self, id: &str, answer: bool) -> bool {
        self.confirmations
            .lock()
            .unwrap()
            .remove(id)
            .is_some_and(|sender| sender.send(answer).is_ok())
    }

    pub fn update_game_state(&self, packet: &Packet) {
        if packet.direction != crate::pipeline::Direction::Inbound {
            return;
        }
        let mut state = self.game_state.lock().unwrap();
        let Some(target) = state.as_object_mut() else {
            return;
        };
        if packet.method == ".lq.Lobby.amuletActivityGiveup" {
            *target = empty_game_state().as_object().unwrap().clone();
            target.insert("ended".into(), Value::Bool(true));
            target.insert("update_reason".into(), json!([packet.method]));
            let payload = Value::Object(target.clone());
            drop(state);
            let _ = self.events.send(event("update_gamestate", payload));
            return;
        }
        if packet.method == ".lq.Lobby.fetchAmuletActivityData" {
            apply_fetch_state(target, &packet.data);
        }
        if let Some(events) = packet.data.get("events").and_then(Value::as_array) {
            for item in events {
                apply_event_state(target, item);
            }
        }
        target.insert("update_reason".into(), json!([packet.method]));
        let payload = Value::Object(target.clone());
        drop(state);
        let _ = self.events.send(event("update_gamestate", payload.clone()));
        if payload
            .get("hand_tiles")
            .and_then(Value::as_array)
            .is_some_and(|hand| hand.len() == 14)
        {
            let _ = self.events.send(event(
                "discard_recommendation",
                crate::recommendations::discard_recommendations(&payload),
            ));
        }
    }
}

pub fn event(kind: &str, data: Value) -> Value {
    json!({"type": kind, "data": data})
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn registry_items(external: &Path, builtin: &str, kind: &str) -> Value {
    let builtin = serde_json::from_str::<Value>(builtin)
        .ok()
        .filter(|value| valid_registry(value, kind))
        .unwrap_or_else(|| json!({"items": []}));
    let external_value = fs::read(external)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .filter(|value| valid_registry(value, kind));
    let value = match external_value {
        Some(value) if registry_version(&value) >= registry_version(&builtin) => value,
        Some(_) => {
            let _ = write_registry(external, &builtin);
            builtin
        }
        None if !external.exists() => {
            let _ = write_registry(external, &builtin);
            builtin
        }
        None => builtin,
    };
    value.get("items").cloned().unwrap_or_else(|| json!([]))
}

fn registry_version(value: &Value) -> i64 {
    value
        .get("version")
        .and_then(|version| {
            version
                .as_i64()
                .or_else(|| version.as_str()?.parse::<i64>().ok())
        })
        .unwrap_or(0)
}

fn valid_registry(value: &Value, kind: &str) -> bool {
    if value.get("schema_version").and_then(Value::as_i64) != Some(1) {
        return false;
    }
    let Some(items) = value.get("items").and_then(Value::as_array) else {
        return false;
    };
    let allowed_rarities: &[&str] = match kind {
        "amulets" => &["GREEN", "BLUE", "ORANGE", "PURPLE", "GRAY"],
        "badges" => &["BROWN", "BLUE", "RED"],
        _ => return false,
    };
    let mut ids = std::collections::HashSet::new();
    let mut names = std::collections::HashSet::new();
    items.iter().all(|item| {
        let Some(row) = item.as_object() else {
            return false;
        };
        let integer = |key: &str| {
            row.get(key).is_some_and(|value| {
                value
                    .as_i64()
                    .or_else(|| value.as_str()?.parse::<i64>().ok())
                    .is_some()
            })
        };
        let Some(id) = row.get("id").and_then(|value| {
            value
                .as_i64()
                .or_else(|| value.as_str()?.parse::<i64>().ok())
        }) else {
            return false;
        };
        let Some(name) = row.get("name").and_then(Value::as_str).map(str::trim) else {
            return false;
        };
        let Some(rarity) = row.get("rarity").and_then(Value::as_str) else {
            return false;
        };
        integer("icon_id")
            && !name.is_empty()
            && allowed_rarities.contains(&rarity.to_ascii_uppercase().as_str())
            && ids.insert(id)
            && names.insert(name.to_ascii_lowercase())
    })
}

fn write_registry(path: &Path, value: &Value) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

fn load_config_table(path: &Path, defaults: &Value, registered: &Value) -> (Value, bool) {
    let disk = fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|value| value.as_object().cloned());
    let Some(items) = registered.as_object() else {
        return (json!({}), true);
    };
    let need_write = disk.as_ref().is_none_or(|values| {
        values.len() != items.len() || items.keys().any(|key| !values.contains_key(key))
    });
    let mut loaded = Map::new();
    for (key, current) in items {
        loaded.insert(
            key.clone(),
            disk.as_ref()
                .and_then(|values| values.get(key))
                .cloned()
                .or_else(|| defaults.get(key).cloned())
                .unwrap_or_else(|| current.clone()),
        );
    }
    (Value::Object(loaded), need_write)
}

fn write_json(path: &Path, value: &Value) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

fn unwrapped(value: &Value) -> Value {
    value.get("value").cloned().unwrap_or_else(|| value.clone())
}

fn set_known(target: &mut Map<String, Value>, source: &Value, source_key: &str, target_key: &str) {
    if let Some(value) = source.get(source_key) {
        target.insert(target_key.into(), unwrapped(value));
    }
}

fn apply_fetch_state(target: &mut Map<String, Value>, response: &Value) {
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
    if let Some(pool) = round.get("pool").and_then(Value::as_array) {
        let deck = pool
            .iter()
            .filter_map(|tile| Some((tile.get("id")?.to_string(), tile.get("tile")?.clone())))
            .collect::<Map<_, _>>();
        target.insert("deck_map".into(), Value::Object(deck));
        let hands = round
            .get("hands")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let ids = pool
            .iter()
            .filter_map(|tile| tile.get("id").and_then(Value::as_u64))
            .collect::<Vec<_>>();
        let hand_ids = hands
            .iter()
            .filter_map(Value::as_u64)
            .collect::<std::collections::HashSet<_>>();
        let dora_hint = round
            .get("dora")
            .and_then(Value::as_array)
            .and_then(|items| items.first())
            .and_then(Value::as_u64);
        let mut opening = if let Some(first_dora) = dora_hint {
            ids.iter()
                .position(|id| *id == first_dora)
                .map(|index| ids[..index].to_vec())
                .unwrap_or_else(|| hand_ids.iter().copied().collect())
        } else {
            hand_ids.iter().copied().collect()
        };
        if let Some(last) = pool.last() {
            if last.get("tile").and_then(Value::as_str) == Some("bd") {
                if let Some(id) = last.get("id").and_then(Value::as_u64) {
                    if hand_ids.contains(&id) && !opening.contains(&id) {
                        opening.push(id)
                    }
                }
            }
        }
        let opening = opening
            .into_iter()
            .collect::<std::collections::HashSet<_>>();
        let remaining = ids
            .into_iter()
            .filter(|id| !opening.contains(id))
            .collect::<Vec<_>>();
        target.insert(
            "dora_tiles".into(),
            json!(remaining.iter().take(10).collect::<Vec<_>>()),
        );
        let locked = round
            .get("lockedTile")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_u64)
            .collect::<std::collections::HashSet<_>>();
        let used = round
            .get("usedDesktop")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_u64)
            .collect::<std::collections::HashSet<_>>();
        let mut wall = remaining
            .iter()
            .skip(10)
            .take(36)
            .copied()
            .filter(|id| !locked.contains(id) && !used.contains(id))
            .collect::<Vec<_>>();
        let remain = round
            .get("desktopRemain")
            .and_then(Value::as_u64)
            .unwrap_or(wall.len() as u64) as usize;
        if wall.len() > remain {
            wall = wall.split_off(wall.len() - remain)
        }
        target.insert("wall_tiles".into(), json!(wall));
        target.insert(
            "replacement_tiles".into(),
            json!(remaining.iter().skip(46).collect::<Vec<_>>()),
        );
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

fn rebuild_pool_sections(target: &mut Map<String, Value>, round: &Value, initial_event: bool) {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "shanten-lens-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn config_backfills_missing_fields_and_can_overwrite_existing_files() {
        let base = temp_root("config");
        let config_root = base.join("configs");
        fs::create_dir_all(&config_root).unwrap();
        fs::write(config_root.join("game.json"), r#"{"public_all":true}"#).unwrap();
        fs::write(
            config_root.join("backend.json"),
            r#"{"obsolete":true,"mitm_port":10999,"enable_upstream_proxy":false,"upstream_proxy":""}"#,
        )
        .unwrap();
        let (events, _) = broadcast::channel(4);
        let services = Services::load(config_root.clone(), events).unwrap();

        assert_eq!(services.table("game")["public_all"], true);
        assert_eq!(services.table("game")["modify_announcement"], true);
        let saved =
            serde_json::from_slice::<Value>(&fs::read(config_root.join("game.json")).unwrap())
                .unwrap();
        assert!(saved.get("unlock_illustrated_book").is_some());
        let backend_saved =
            serde_json::from_slice::<Value>(&fs::read(config_root.join("backend.json")).unwrap())
                .unwrap();
        assert!(backend_saved.get("obsolete").is_none());

        assert_eq!(services.startup_announcement()["title"], "欢迎使用向听镜");
        services.set_locale("ja-JP");
        assert_eq!(
            services.startup_announcement()["title"],
            "向聴レンズへようこそ"
        );

        services
            .patch_config(&json!({"game":{"public_all":false}}))
            .unwrap();
        assert_eq!(services.table("game")["public_all"], false);
        fs::write(config_root.join("game.json"), "[]").unwrap();
        services.reload_files();
        assert_eq!(services.table("game"), defaults()["game"]);

        fs::remove_dir_all(base).unwrap();
    }
}
