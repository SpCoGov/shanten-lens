use super::Services;
use crate::storage::write_json;
use serde_json::{json, Value};
use std::{fs, path::Path};

pub(super) const AMULETS: &str = include_str!("../../assets/amulets.json");
pub(super) const BADGES: &str = include_str!("../../assets/badges.json");

impl Services {
    pub fn registry(&self) -> Value {
        self.registry.lock().unwrap().clone()
    }
}

pub(super) fn registry_items(external: &Path, builtin: &str, kind: &str) -> Value {
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
