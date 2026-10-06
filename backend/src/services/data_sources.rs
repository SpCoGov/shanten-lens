use super::{event, game_state::empty_game_state, Services};
use crate::ipc::{DataSource, DataSourceStatus};
use serde_json::{json, Value};

#[derive(Default)]
pub(super) struct DataSources {
    pub packet: Option<Value>,
    pub qyzz: Option<Value>,
    pub choice: Option<DataSource>,
    pub status: DataSourceStatus,
}

impl Services {
    pub fn data_source_status(&self) -> DataSourceStatus {
        self.data_sources.lock().unwrap().status.clone()
    }

    pub fn packet_processing_allowed(&self) -> bool {
        let sources = self.data_sources.lock().unwrap();
        !sources.status.pending && sources.status.active != Some(DataSource::Qyzz)
    }

    pub fn qyzz_operations_allowed(&self) -> bool {
        self.data_source_status().active == Some(DataSource::Qyzz)
    }

    pub fn select_data_source(&self, source: DataSource, revision: u64) -> Result<DataSourceStatus, String> {
        let mut sources = self.data_sources.lock().unwrap();
        if !sources.status.pending || sources.status.revision != revision {
            return Err("data-source-conflict-expired".into());
        }
        sources.choice = Some(source);
        self.publish_selected_source(&mut sources, None);
        Ok(sources.status.clone())
    }

    // Call with the source lock held so source selection and state publication stay ordered.
    pub(super) fn publish_selected_source(&self, sources: &mut DataSources, updated: Option<DataSource>) {
        let both = sources.packet.is_some() && sources.qyzz.is_some();
        if !both { sources.choice = None; }
        let active = if both { sources.choice } else if sources.qyzz.is_some() {
            Some(DataSource::Qyzz)
        } else if sources.packet.is_some() { Some(DataSource::Packet) } else { None };
        let mut status = DataSourceStatus {
            active, pending: both && sources.choice.is_none(),
            packet: sources.packet.is_some(), qyzz: sources.qyzz.is_some(),
            revision: sources.status.revision,
        };
        let changed = status != sources.status;
        let switched = status.active != sources.status.active || status.pending != sources.status.pending;
        if changed { status.revision += 1; }
        sources.status = status.clone();
        if switched || (active.is_some() && updated == active) {
            let mut next = match active {
                Some(DataSource::Qyzz) => {
                    let mut next = empty_game_state();
                    next.as_object_mut().unwrap().extend(sources.qyzz.as_ref().unwrap().as_object().unwrap().clone());
                    next["source"] = json!("qyzz");
                    next["qyzz_connected"] = json!(true);
                    next
                }
                Some(DataSource::Packet) => {
                    let mut next = sources.packet.as_ref().unwrap().clone();
                    next["source"] = json!("packet");
                    next["origin_session_id"] = next["session_id"].clone();
                    next
                }
                None => {
                    let mut next = empty_game_state();
                    next["qyzz_connected"] = json!(false);
                    next
                }
            };
            let mut current = self.game_state.lock().unwrap();
            let new_session = switched || current["source"] != next["source"]
                || current["origin_session_id"] != next["origin_session_id"]
                || current["qyzz_run_id"] != next["qyzz_run_id"]
                || current["qyzz_deal_id"] != next["qyzz_deal_id"];
            next["session_id"] = json!(current["session_id"].as_u64().unwrap_or(0) + u64::from(new_session));
            next["revision"] = json!(current["revision"].as_u64().unwrap_or(0) + 1);
            *current = next.clone();
            drop(current);
            let _ = self.events.send(event("update_gamestate", next.clone()));
            if next["hand_tiles"].as_array().is_some_and(|hand| hand.len() == 14) {
                let _ = self.events.send(event("discard_recommendation", crate::recommendations::discard_recommendations(&next)));
            }
        }
        if changed { let _ = self.events.send(event("data_source_status", json!(status))); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::{Direction, Packet};

    #[test]
    fn conflict_requires_choice_and_keeps_both_sources_separate() {
        let root = std::env::temp_dir().join(format!("lens-sources-{}", std::process::id()));
        let (events, mut received) = tokio::sync::broadcast::channel(64);
        let services = Services::load(root.join("configs"), events).unwrap();
        let mut packet = Packet { direction: Direction::Inbound, packet_type: "Res".into(),
            method: ".lq.Lobby.fetchAmuletActivityData".into(), id: Some(1),
            data: json!({"game":{"state":{"current":5},"round":{"hands":[1]}}}) };
        let mut qyzz = json!({"source":"qyzz","qyzz_run_id":"run","qyzz_deal_id":1,"qyzz_version":1,
            "stage":4,"deck_map":{"2":"8s"},"hand_tiles":[2],"wall_tiles":[],"replacement_tiles":[],
            "locked_tiles":[],"dora_tiles":[],"effect_list":[]});
        services.update_game_state_from_flow(&packet, 7);
        let original_session = services.game_state()["session_id"].as_u64().unwrap();
        services.publish_qyzz_state(&qyzz).unwrap();
        let conflict = services.data_source_status();
        assert!(conflict.pending && conflict.qyzz && conflict.packet);
        assert_eq!(services.game_state()["stage"], -1);
        assert!(!services.packet_processing_allowed() && !services.qyzz_operations_allowed());
        assert!(services.select_data_source(DataSource::Qyzz, conflict.revision - 1).is_err());
        services.select_data_source(DataSource::Qyzz, conflict.revision).unwrap();
        assert_eq!(services.game_state()["hand_tiles"], json!([2]));
        assert!(!services.packet_processing_allowed());
        packet.data["game"]["round"]["hands"] = json!([3]);
        services.update_game_state_from_flow(&packet, 7);
        assert_eq!(services.game_state()["hand_tiles"], json!([2]));
        assert!(!services.data_source_status().pending);
        services.disconnect_qyzz();
        assert_eq!(services.game_state()["hand_tiles"], json!([3]));
        assert!(services.packet_processing_allowed());
        services.publish_qyzz_state(&qyzz).unwrap();
        let second = services.data_source_status();
        assert!(second.pending && second.revision > conflict.revision);
        services.select_data_source(DataSource::Packet, second.revision).unwrap();
        qyzz["qyzz_version"] = json!(2);
        services.publish_qyzz_state(&qyzz).unwrap();
        assert_eq!(services.game_state()["hand_tiles"], json!([3]));
        assert!(!services.qyzz_operations_allowed());
        services.disconnect_game_flow(99);
        assert!(services.data_source_status().packet);
        services.disconnect_game_flow(7);
        assert_eq!(services.game_state()["source"], "qyzz");
        assert!(services.game_state()["session_id"].as_u64().unwrap() > original_session);
        services.disconnect_qyzz();
        assert_eq!(services.game_state()["stage"], -1);
        // Also detect the second source when the local connection arrives first.
        services.publish_qyzz_state(&qyzz).unwrap();
        services.update_game_state_from_flow(&packet, 9);
        assert!(services.data_source_status().pending);
        let conflicts = std::iter::from_fn(|| received.try_recv().ok())
            .filter(|event| event["type"] == "data_source_status" && event["data"]["pending"] == true).count();
        assert_eq!(conflicts, 3);
        drop(services);
        std::fs::remove_dir_all(root).unwrap();
    }
}
