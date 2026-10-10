use super::Services;
use crate::ipc::DataSource;
use crate::storage::write_json;
use serde_json::{json, Value};
use std::{fs, path::Path, sync::LazyLock};

pub(super) const AMULETS: &str = include_str!("../../assets/amulets.json");
pub(super) const BADGES: &str = include_str!("../../assets/badges.json");

// Bundled from D:/qyzz/assets/amulets/catalog.json; registry IDs omit the variant digit.
static QYZZ_AMULETS: LazyLock<Value> = LazyLock::new(|| {
    let catalog: Value = serde_json::from_str(include_str!("../../assets/qyzz_amulets.json"))
        .expect("invalid bundled QYZZ amulet catalog");
    catalog["amulets"].as_array().unwrap().iter().map(|amulet| {
        let icon = amulet["icon"].as_str().unwrap();
        let icon_id: u64 = icon.strip_prefix("fu_").and_then(|text| text.strip_suffix(".png"))
            .and_then(|text| text.parse().ok()).expect("invalid QYZZ amulet icon");
        let rarity = match amulet["rarity"].as_u64().unwrap() {
            1 => "PURPLE", 2 => "ORANGE", 3 => "BLUE", 4 => "GREEN", 5 => "GRAY",
            _ => panic!("invalid QYZZ amulet rarity"),
        };
        json!({
            "id": amulet["data_id"].as_u64().unwrap() / 10,
            "icon_id": icon_id, "name": amulet["name"], "rarity": rarity,
            "plus_name": amulet["plus_name"],
            "sell_price": amulet["sell_price"], "plus_sell_price": amulet["plus_sell_price"],
        })
    }).collect()
});

impl Services {
    pub fn registry(&self) -> Value {
        let sources = self.data_sources.lock().unwrap();
        self.registry_for_source(sources.status.active)
    }

    pub(super) fn registry_for_source(&self, source: Option<DataSource>) -> Value {
        let mut registry = self.registry.lock().unwrap().clone();
        if source == Some(DataSource::Qyzz) {
            registry["source"] = json!("qyzz");
            registry["amulets"] = QYZZ_AMULETS.clone();
        }
        registry
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qyzz_catalog_preserves_ids_icons_names_rarities_and_prices() {
        let catalog: Value = serde_json::from_str(include_str!("../../assets/qyzz_amulets.json")).unwrap();
        let rows = QYZZ_AMULETS.as_array().unwrap();
        assert_eq!(rows.len(), 181);
        assert!(valid_registry(&json!({"schema_version": 1, "items": *QYZZ_AMULETS}), "amulets"));
        for (row, original) in rows.iter().zip(catalog["amulets"].as_array().unwrap()) {
            let id = row["id"].as_u64().unwrap();
            assert_eq!(id * 10, original["data_id"]);
            assert_eq!(id * 10 + 1, original["upgrade_id"]);
            assert_eq!(row["name"], original["name"]);
            assert_eq!(row["plus_name"], original["plus_name"]);
            assert_eq!(row["sell_price"], original["sell_price"]);
            assert_eq!(row["plus_sell_price"], original["plus_sell_price"]);
            let rarity = ["", "PURPLE", "ORANGE", "BLUE", "GREEN", "GRAY"][original["rarity"].as_u64().unwrap() as usize];
            assert_eq!(row["rarity"], rarity);
            assert_eq!(original["plus_rarity"], original["rarity"]);
            let icon = format!("fu_{:04}.png", row["icon_id"].as_u64().unwrap());
            assert_eq!(original["icon"], icon);
            assert!(Path::new(env!("CARGO_MANIFEST_DIR")).join("../app/public/assets/amulet").join(icon).is_file());
        }
        let payload = json!({"source": "qyzz", "amulets": *QYZZ_AMULETS, "badges": []});
        let typed: crate::ipc::RegistryPayload = serde_json::from_value(payload.clone()).unwrap();
        assert_eq!(serde_json::to_value(typed).unwrap(), payload);
        let packet = json!({"amulets": [{"id": 1, "name": "legacy", "icon_id": 1, "rarity": "GREEN"}], "badges": []});
        let typed: crate::ipc::RegistryPayload = serde_json::from_value(packet.clone()).unwrap();
        assert_eq!(serde_json::to_value(typed).unwrap(), packet);
    }
}
