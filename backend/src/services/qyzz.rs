use super::Services;
use serde_json::Value;
#[cfg(test)]
use serde_json::json;

fn validate(snapshot: &Value) -> Result<(), String> {
    let object = snapshot.as_object().ok_or("qyzz-state-not-object")?;
    if object.is_empty() { return Ok(()); }
    if snapshot["source"] != "qyzz"
        || snapshot["qyzz_run_id"].as_str().is_none_or(|id| id.is_empty() || id.len() > 128)
        || snapshot["qyzz_version"].as_u64().is_none()
        || snapshot["qyzz_deal_id"].as_u64().is_none() {
        return Err("qyzz-invalid-revision".into());
    }
    let deck = snapshot["deck_map"].as_object().ok_or("qyzz-missing-deck")?;
    if deck.len() > 256 || deck.iter().any(|(id, face)| id.parse::<u64>().is_err() || face.as_str().is_none_or(|face| !valid_tile(face))) {
        return Err("qyzz-invalid-deck".into());
    }
    for field in ["hand_tiles", "wall_tiles", "locked_tiles", "replacement_tiles", "dora_tiles"] {
        let ids = snapshot[field].as_array().ok_or("qyzz-missing-tiles")?;
        if ids.len() > 256 || ids.iter().any(|id| id.as_u64().is_none_or(|id| !deck.contains_key(&id.to_string()))) {
            return Err("qyzz-invalid-tile-id".into());
        }
    }
    let effects = snapshot["effect_list"].as_array().ok_or("qyzz-missing-effects")?;
    if effects.len() > 10 || effects.iter().any(|effect| {
        effect["id"].as_u64().is_none() || effect["uid"].as_u64().is_none()
            || effect["store"].as_array().is_none_or(|store| store.iter().any(|value| value.as_str().is_none_or(|text| text.is_empty() || text.len() > 10000 || !text.bytes().all(|c| c.is_ascii_digit()))))
    }) { return Err("qyzz-invalid-effects-or-growth-out-of-range".into()); }
    Ok(())
}

fn valid_tile(face: &str) -> bool {
    if face == "bd" { return true; }
    let bytes = face.as_bytes();
    bytes.len() == 2 && match bytes[1] {
        b'm' | b'p' | b's' => bytes[0].is_ascii_digit(),
        b'z' => (b'1'..=b'7').contains(&bytes[0]),
        _ => false,
    }
}

impl Services {
    pub fn publish_qyzz_state(&self, snapshot: &Value) -> Result<(), String> {
        validate(snapshot)?;
        let mut sources = self.data_sources.lock().unwrap();
        if let Some(previous) = &sources.qyzz {
            if previous["qyzz_run_id"] == snapshot["qyzz_run_id"] && previous["qyzz_deal_id"] == snapshot["qyzz_deal_id"]
                && snapshot["qyzz_version"].as_u64() < previous["qyzz_version"].as_u64() { return Ok(()); }
        }
        sources.qyzz = Some(snapshot.clone());
        self.publish_selected_source(&mut sources, Some(crate::ipc::DataSource::Qyzz));
        Ok(())
    }

    pub fn disconnect_qyzz(&self) {
        let mut sources = self.data_sources.lock().unwrap();
        if sources.qyzz.take().is_some() { self.publish_selected_source(&mut sources, None); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_bridge_tile_ids_and_growth() {
        assert!(validate(&json!({})).is_ok());
        let mut state = json!({"source":"qyzz", "qyzz_run_id":"test", "qyzz_version":1, "qyzz_deal_id":1,
            "deck_map":{"1":"8s","1000":"bd"}, "hand_tiles":[1,1000], "wall_tiles":[],
            "locked_tiles":[], "replacement_tiles":[], "dora_tiles":[],
            "effect_list":[{"id":1911,"uid":9,"store":["230","0"]}]});
        assert!(validate(&state).is_ok());
        state["hand_tiles"] = json!([2]);
        assert!(validate(&state).is_err());
        state["hand_tiles"] = json!([1]);
        state["effect_list"][0]["store"] = json!(["1.2"]);
        assert!(validate(&state).is_err());
    }
}
