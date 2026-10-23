use super::{event, Services};
use crate::{
    ipc::{PacketLogItem, PacketLogSnapshot, PacketRecordingStatus},
    pipeline::Packet,
};
use serde_json::{json, Value};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

pub(super) struct PacketRecording {
    file: File,
    status: PacketRecordingStatus,
}

impl Services {
    pub fn packet_recording_status(&self) -> PacketRecordingStatus {
        self.packet_recording
            .lock()
            .unwrap()
            .as_ref()
            .map(|recording| recording.status.clone())
            .unwrap_or_default()
    }

    pub fn record_dir(&self) -> PathBuf {
        self.root.parent().unwrap_or(&self.root).join("record")
    }

    pub fn set_packet_recording(&self, active: bool) -> Result<PacketRecordingStatus, String> {
        let mut recording = self.packet_recording.lock().unwrap();
        let status = if active {
            if let Some(recording) = recording.as_ref() {
                return Ok(recording.status.clone());
            }
            let directory = self.record_dir();
            fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|error| error.to_string())?
                .as_nanos();
            let path = directory.join(format!("shanten-lens-recording-{timestamp}.jsonl"));
            let file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(|error| error.to_string())?;
            let status = PacketRecordingStatus {
                active: true,
                path: Some(path.to_string_lossy().into_owned()),
                ..Default::default()
            };
            *recording = Some(PacketRecording {
                file,
                status: status.clone(),
            });
            status
        } else if let Some(mut finished) = recording.take() {
            if let Err(error) = finished.file.sync_all() {
                finished.status.error = Some(error.to_string());
            }
            finished.status.active = false;
            finished.status
        } else {
            PacketRecordingStatus::default()
        };
        let _ = self
            .events
            .send(event("packet_recording_status", json!(status)));
        Ok(status)
    }

    pub fn record_received_packet(&self, packet: &Packet) {
        let mut recording = self.packet_recording.lock().unwrap();
        let Some(recording) = recording.as_mut() else {
            return;
        };
        if recording.status.error.is_some() {
            return;
        }
        let mut packet = packet.clone();
        redact_auth_data(&packet.method, &mut packet.data);
        let result = serde_json::to_vec(&packet)
            .map_err(|error| error.to_string())
            .and_then(|mut line| {
                line.push(b'\n');
                recording
                    .file
                    .write_all(&line)
                    .map_err(|error| error.to_string())
            });
        match result {
            Ok(()) => recording.status.count += 1,
            Err(error) => {
                recording.status.error = Some(error.clone());
                let _ = self.events.send(event(
                    "ui_toast",
                    json!({
                        "msg_key": "diagnostics.packet_recording_failed",
                        "msg_values": {"reason": error}, "kind": "error", "duration": 10000
                    }),
                ));
            }
        }
    }

    pub fn record_packet(&self, packet: &Packet) {
        let mut data = packet.data.clone();
        redact_auth_data(&packet.method, &mut data);
        let value = PacketLogItem {
            direction: packet.direction,
            packet_type: packet.packet_type.clone(),
            method: packet.method.clone(),
            id: packet.id,
            data,
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
        if method.starts_with("qyzz.") {
            return false;
        }
        self.packet_log.lock().unwrap().iter().any(|packet| {
            packet.direction == crate::pipeline::Direction::Outbound
                && packet.packet_type == "Req"
                && packet.method == method
        })
    }
}

fn redact_auth_data(method: &str, data: &mut Value) {
    if !matches!(
        method,
        ".lq.Lobby.login"
            | ".lq.Lobby.oauth2Auth"
            | ".lq.Lobby.oauth2Check"
            | ".lq.Lobby.oauth2Login"
            | ".lq.Lobby.oauth2Signup"
    ) {
        return;
    }
    fn redact(value: &mut Value) {
        match value {
            Value::Object(fields) => {
                for (key, value) in fields {
                    if matches!(
                        key.as_str(),
                        "password" | "accessToken" | "access_token" | "token" | "code"
                    ) && value.is_string()
                    {
                        *value = Value::String("[redacted]".into());
                    } else {
                        redact(value);
                    }
                }
            }
            Value::Array(values) => values.iter_mut().for_each(redact),
            _ => {}
        }
    }
    redact(data);
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authentication_secrets_are_redacted_in_logs_and_recordings_without_changing_wire_packets() {
        let root = std::env::temp_dir().join(format!(
            "lens-auth-redaction-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let (events, mut received) = tokio::sync::broadcast::channel(32);
        let services = Services::load(root.join("configs"), events).unwrap();
        let path = services.set_packet_recording(true).unwrap().path.unwrap();
        let packet = Packet {
            direction: crate::pipeline::Direction::Outbound,
            packet_type: "Req".into(),
            method: ".lq.Lobby.oauth2Auth".into(),
            id: Some(1),
            data: json!({"uid":"123", "code":"sensitive-code", "password":"sensitive-pass", "accessToken":"sensitive-token", "error":{"code":151}}),
        };
        services.record_received_packet(&packet);
        services.record_packet(&packet);
        services.set_packet_recording(false).unwrap();
        assert_eq!(packet.data["code"], "sensitive-code");
        assert_eq!(
            services.packet_log_snapshot().packets[0].data["error"]["code"],
            151
        );
        let recording = std::fs::read_to_string(path).unwrap();
        assert!(!recording.contains("sensitive-"));
        assert!(recording.contains("[redacted]"));
        while let Ok(event) = received.try_recv() {
            assert!(!event.to_string().contains("sensitive-"));
        }
        drop(services);
        for entry in std::fs::read_dir(root.join("logs")).unwrap() {
            assert!(!std::fs::read_to_string(entry.unwrap().path())
                .unwrap()
                .contains("sensitive-"));
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
