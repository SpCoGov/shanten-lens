use super::{event, Services};
use crate::{
    pipeline::Packet,
    protocol::{decode_game_record_base64, encode_game_record_base64},
};
use anyhow::Context;
use serde_json::{json, Value};

impl Services {
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
}
