use super::registry::{registry_items, AMULETS, BADGES};
use super::{event, Services};
use crate::storage::write_json;
use anyhow::Context;
use serde_json::{json, Map, Value};
use std::{fs, path::Path};

pub(super) fn defaults() -> Value {
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

impl Services {
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
        let sources = self.data_sources.lock().unwrap();
        let mut current = self.registry.lock().unwrap();
        if *current != registry {
            *current = registry;
            drop(current);
            let _ = self.events.send(event("update_registry", self.registry_for_source(sources.status.active)));
        }
    }
}

pub(super) fn load_config_table(
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

pub(super) fn validate_config(name: &str, value: &Value) -> Result<(), String> {
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
