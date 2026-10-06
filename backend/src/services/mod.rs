//! Shared configuration, game state, packet records and UI events.
mod config;
mod game_record;
mod game_state;
mod packets;
mod registry;
mod qyzz;
mod data_sources;
#[cfg(test)]
mod tests;

use crate::{ipc::PacketLogItem, logging::JsonLineLog, storage::write_json};
use config::{defaults, load_config_table, validate_config};
use game_state::empty_game_state;
use packets::PacketRecording;
use registry::{registry_items, AMULETS, BADGES};
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::broadcast;

#[derive(Clone)]
pub struct Services {
    root: PathBuf,
    config: Arc<Mutex<Value>>,
    locale: Arc<Mutex<String>>,
    game_state: Arc<Mutex<Value>>,
    data_sources: Arc<Mutex<data_sources::DataSources>>,
    packet_log: Arc<Mutex<VecDeque<PacketLogItem>>>,
    packet_log_file: Arc<JsonLineLog>,
    // ponytail: Serialize file writes for ordering; use a writer queue if disk I/O delays packets.
    packet_recording: Arc<Mutex<Option<PacketRecording>>>,
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
            data_sources: Arc::new(Mutex::new(data_sources::DataSources::default())),
            packet_log: Arc::new(Mutex::new(VecDeque::new())),
            packet_log_file: Arc::new(packet_log_file),
            packet_recording: Arc::new(Mutex::new(None)),
            registry: Arc::new(Mutex::new(registry)),
            game_record_override: Arc::new(Mutex::new(None)),
            confirmations: Arc::new(Mutex::new(std::collections::HashMap::new())),
            next_confirmation: Arc::new(AtomicU64::new(1)),
            events,
            proxy_status: Arc::new(Mutex::new(crate::ipc::ProxyStatus::default())),
        })
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
}

pub fn event(kind: &str, data: Value) -> Value {
    json!({"type": kind, "data": data})
}
