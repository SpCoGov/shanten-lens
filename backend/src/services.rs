use crate::storage::write_json;
use crate::{
    ipc::{PacketLogItem, PacketLogSnapshot},
    logging::JsonLineLog,
    pipeline::Packet,
    protocol::{decode_game_record_base64, encode_game_record_base64},
};
use anyhow::Context;
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
use tokio::sync::broadcast;

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

#[derive(Clone)]
pub struct Services {
    root: PathBuf,
    config: Arc<Mutex<Value>>,
    locale: Arc<Mutex<String>>,
    game_state: Arc<Mutex<Value>>,
    packet_log: Arc<Mutex<VecDeque<PacketLogItem>>>,
    packet_log_file: Arc<JsonLineLog>,
    registry: Arc<Mutex<Value>>,
    game_record_override: Arc<Mutex<Option<Value>>>,
    confirmations: Arc<Mutex<std::collections::HashMap<String, mpsc::Sender<bool>>>>,
    next_confirmation: Arc<AtomicU64>,
    pub events: broadcast::Sender<Value>,
    proxy_status: Arc<Mutex<crate::ipc::ProxyStatus>>,
}

impl Services {
    pub fn load(root: PathBuf, events: broadcast::Sender<Value>) -> anyhow::Result<Self> {
        fs::create_dir_all(&root)?;
        let mut config = defaults();
        for name in ["game", "general", "backend", "fuse", "autorun"] {
            let path = root.join(format!("{name}.json"));
            let registered = config[name].clone();
            let (loaded, need_write) = load_config_table(&path, &registered, &registered)?;
            validate_config(name, &loaded).map_err(anyhow::Error::msg)?;
            config[name] = loaded;
            if need_write {
                write_json(&path, &config[name])?;
            }
        }
        let data_dir = root.parent().unwrap_or(&root).join("data");
        let log_dir = root.parent().unwrap_or(&root).join("logs");
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let packet_log_file =
            JsonLineLog::create(log_dir.join(format!("shanten-lens-packets-{timestamp}.log")))?;
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
            packet_log_file: Arc::new(packet_log_file),
            registry: Arc::new(Mutex::new(registry)),
            game_record_override: Arc::new(Mutex::new(None)),
            confirmations: Arc::new(Mutex::new(std::collections::HashMap::new())),
            next_confirmation: Arc::new(AtomicU64::new(1)),
            events,
            proxy_status: Arc::new(Mutex::new(crate::ipc::ProxyStatus::default())),
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

    pub fn proxy_status(&self) -> crate::ipc::ProxyStatus {
        self.proxy_status.lock().unwrap().clone()
    }

    pub fn set_proxy_status(&self, running: bool, error: Option<String>) {
        let status = crate::ipc::ProxyStatus { running, error };
        *self.proxy_status.lock().unwrap() = status.clone();
        let _ = self
            .events
            .send(event("proxy_status", serde_json::to_value(status).unwrap()));
    }

    pub fn patch_config(&self, patch: &Value) -> Result<(), String> {
        let Some(tables) = patch.as_object() else {
            return Err("config patch must be an object".into());
        };
        let mut config = self.config.lock().unwrap();
        let defaults = defaults();
        let mut candidate = config.clone();
        for (name, partial) in tables {
            if defaults.get(name).is_none() {
                return Err(format!("unknown config table: {name}"));
            }
            let Some(values) = partial.as_object() else {
                return Err(format!("config table {name} must be an object"));
            };
            let table = candidate
                .as_object_mut()
                .unwrap()
                .entry(name)
                .or_insert_with(|| json!({}));
            let Some(table) = table.as_object_mut() else {
                return Err(format!("config table {name} is invalid"));
            };
            table.extend(values.clone());
            validate_config(name, &candidate[name])?;
        }
        let mut applied = Vec::new();
        for name in tables.keys() {
            write_json(&self.root.join(format!("{name}.json")), &candidate[name])
                .map_err(|e| format!("failed to save {name}: {e}; saved tables: {applied:?}"))?;
            config[name] = candidate[name].clone();
            applied.push(name);
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
                    match load_config_table(&path, &default_config[name], &current).and_then(
                        |loaded| {
                            validate_config(name, &loaded.0).map_err(anyhow::Error::msg)?;
                            Ok(loaded)
                        },
                    ) {
                        Ok(loaded) => loaded,
                        Err(error) => {
                            tracing::warn!(%name, %error, "keeping last valid config");
                            continue;
                        }
                    };
                if need_write {
                    if let Err(error) = write_json(&path, &loaded) {
                        tracing::warn!(%name, %error, "config backfill failed");
                        continue;
                    }
                }
                if current != loaded || need_write {
                    config[name] = loaded;
                    match name {
                        "fuse" => changed_fuse = true,
                        "autorun" => changed_autorun = true,
                        _ => changed_normal = true,
                    }
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

    pub fn publish_game_record(&self, packet: &Packet) {
        if packet.packet_type == "Res"
            && packet.direction == crate::pipeline::Direction::Inbound
            && packet.method == ".lq.Lobby.fetchGameRecord"
        {
            if let Some(encoded) = packet.data.get("data").and_then(Value::as_str) {
                if let Ok(record) = decode_game_record_base64(encoded) {
                    let mut payload = packet.data.clone();
                    payload["data"] = record;
                    let _ = self.events.send(event("update_game_record", payload));
                }
            }
        }
    }

    pub fn arm_game_record_override(&self, response: &Value) -> anyhow::Result<()> {
        let mut encoded = response.clone();
        let record = response
            .get("data")
            .context("fetch response data missing")?;
        encoded["data"] = Value::String(encode_game_record_base64(record)?);
        *self.game_record_override.lock().unwrap() = Some(encoded);
        let _ = self
            .events
            .send(event("game_record_override_status", json!(true)));
        Ok(())
    }

    pub fn apply_game_record_override(&self, packet: &mut Packet) {
        if packet.direction == crate::pipeline::Direction::Inbound
            && packet.packet_type == "Res"
            && packet.method == ".lq.Lobby.fetchGameRecord"
        {
            if let Some(response) = self.game_record_override.lock().unwrap().take() {
                packet.data = response;
                let _ = self
                    .events
                    .send(event("game_record_override_status", json!(false)));
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
        self.packet_log_file.write(&value);
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
        self.update_game_state_from_flow(packet, 0);
    }

    pub fn active_flow_id(&self) -> u64 {
        self.game_state.lock().unwrap()["flow_id"]
            .as_u64()
            .unwrap_or(0)
    }

    pub fn disconnect_game_flow(&self, flow_id: u64) {
        let mut state = self.game_state.lock().unwrap();
        if state["flow_id"].as_u64() != Some(flow_id) {
            return;
        }
        state["session_id"] = json!(state["session_id"].as_u64().unwrap_or(0) + 1);
        state["revision"] = json!(state["revision"].as_u64().unwrap_or(0) + 1);
        let payload = state.clone();
        drop(state);
        let _ = self.events.send(event("update_gamestate", payload));
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
        let mut state = self.game_state.lock().unwrap();
        let Some(target) = state.as_object_mut() else {
            return;
        };
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
            let payload = Value::Object(target.clone());
            drop(state);
            let _ = self.events.send(event("update_gamestate", payload));
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
    write_json(path, value)
}

fn load_config_table(
    path: &Path,
    defaults: &Value,
    registered: &Value,
) -> anyhow::Result<(Value, bool)> {
    let disk =
        match fs::read(path) {
            Ok(bytes) => {
                let value: Value = serde_json::from_slice(&bytes)
                    .with_context(|| format!("invalid config file: {}", path.display()))?;
                Some(value.as_object().cloned().ok_or_else(|| {
                    anyhow::anyhow!("config must be an object: {}", path.display())
                })?)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
    let Some(items) = registered.as_object() else {
        anyhow::bail!("config schema must be an object");
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
    Ok((Value::Object(loaded), need_write))
}

fn validate_config(name: &str, value: &Value) -> Result<(), String> {
    fn validate(schema: &Value, value: &Value, path: &str) -> Result<(), String> {
        let valid = match (schema, value) {
            (Value::Object(schema), Value::Object(values)) => {
                for (key, value) in values {
                    let expected = schema
                        .get(key)
                        .ok_or_else(|| format!("unknown config field: {path}.{key}"))?;
                    validate(expected, value, &format!("{path}.{key}"))?;
                }
                schema.keys().all(|key| values.contains_key(key))
            }
            (Value::Bool(_), Value::Bool(_))
            | (Value::String(_), Value::String(_))
            | (Value::Array(_), Value::Array(_)) => true,
            (Value::Number(_), Value::Number(number)) => number.as_u64().is_some(),
            _ => false,
        };
        if valid {
            Ok(())
        } else {
            Err(format!("invalid config value: {path}"))
        }
    }
    validate(&defaults()[name], value, name)?;
    match name {
        "fuse" => {
            serde_json::from_value::<crate::ipc::FuseConfig>(value.clone())
                .map_err(|error| error.to_string())?;
        }
        "autorun" => {
            serde_json::from_value::<crate::ipc::AutoRunnerConfig>(value.clone())
                .map_err(|error| error.to_string())?;
        }
        "backend"
            if !value["mitm_port"]
                .as_u64()
                .is_some_and(|port| (1..=65535).contains(&port)) =>
        {
            return Err("invalid mitm_port".into())
        }
        _ => {}
    }
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
    fn failed_giveup_preserves_state_and_other_flows_cannot_apply_deltas() {
        let root = temp_root("failed-giveup");
        let (events, _) = broadcast::channel(16);
        let services = Services::load(root.join("configs"), events).unwrap();
        let mut packet = Packet {
            direction: crate::pipeline::Direction::Inbound,
            packet_type: "Res".into(),
            method: ".lq.Lobby.fetchAmuletActivityData".into(),
            id: Some(1),
            data: json!({"game":{"state":{"current":5}}}),
        };
        services.update_game_state_from_flow(&packet, 10);
        let previous = services.game_state();
        packet.method = ".lq.Lobby.amuletActivityGiveup".into();
        packet.data = json!({"error":{"code":1}});
        services.update_game_state_from_flow(&packet, 10);
        assert_eq!(services.game_state(), previous);
        packet.method = ".lq.Lobby.amuletActivityOperate".into();
        packet.data = json!({"events":[{"state":{"current":3}}]});
        services.update_game_state_from_flow(&packet, 20);
        assert_eq!(services.game_state(), previous);
        services.disconnect_game_flow(10);
        assert_ne!(services.game_state()["session_id"], previous["session_id"]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn fetch_and_delta_use_the_same_pool_partition() {
        for boss in [false, true] {
            let pool = (1..=80)
                .map(|id| json!({"id":id,"tile":if id==80 {"bd"} else {"1m"}}))
                .collect::<Vec<_>>();
            let round = json!({"pool":pool,"hands":[1,2,3,4,5,6,7,8,9,10,11,12,80],"dora":[13],"lockedTile":[25],"usedDesktop":[26],"desktopRemain":16});
            let mut fetched = empty_game_state().as_object().unwrap().clone();
            apply_fetch_state(
                &mut fetched,
                &json!({"game":{"round":round,"map":{"node":1,"mapNodes":[{"args":if boss {vec![926]} else {vec![]}}]}}}),
            );
            assert_eq!(fetched["dora_tiles"], json!((13..23).collect::<Vec<_>>()));
            assert_eq!(fetched["wall_tiles"].as_array().unwrap().len(), 16);
            assert!(!fetched["replacement_tiles"]
                .as_array()
                .unwrap()
                .contains(&json!(80)));
            assert_eq!(fetched["replacement_tiles"][0], if boss { 41 } else { 59 });
            let mut delta = fetched.clone();
            rebuild_pool_sections(
                &mut delta,
                &json!({"pool":{"dirty":true,"value":pool},"dora":{"dirty":true,"value":[13]}}),
                false,
            );
            for field in [
                "deck_map",
                "opening_hand_tiles",
                "dora_tiles",
                "wall_tiles",
                "replacement_tiles",
            ] {
                assert_eq!(fetched[field], delta[field], "{field}");
            }
        }
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
        let previous = services.table("game");
        services.reload_files();
        assert_eq!(services.table("game"), previous);
        assert_eq!(
            fs::read_to_string(config_root.join("game.json")).unwrap(),
            "[]"
        );
        assert!(services
            .patch_config(&json!({"../outside": {"value": true}}))
            .is_err());
        assert!(!base.join("outside.json").exists());
        assert!(services
            .patch_config(&json!({"game": {"public_all": "wrong type"}}))
            .is_err());
        assert!(services
            .patch_config(&json!({"backend": {"mitm_port": 65536}}))
            .is_err());
        assert_eq!(services.table("game"), previous);

        fs::remove_dir_all(base).unwrap();
    }
}
