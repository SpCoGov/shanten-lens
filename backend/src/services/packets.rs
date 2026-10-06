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
        let result = serde_json::to_vec(packet)
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
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
