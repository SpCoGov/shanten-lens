use crate::{
    ipc::TsumoLoopStatus,
    proxy::ProxyControl,
    services::{event, Services},
};
use serde_json::{json, Value};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone)]
pub struct Automation {
    services: Arc<Services>,
    proxy: ProxyControl,
    autorun: Arc<Mutex<AutoStatus>>,
    tsumo: Arc<Mutex<TsumoStatus>>,
}

struct AutoStatus {
    profile: Option<Value>,
    task: Option<tokio::task::JoinHandle<()>>,
    running: bool,
    runs: u64,
    started: Option<Instant>,
    started_at: u128,
    best: u64,
    step: String,
    error: String,
}

#[derive(Default)]
struct TsumoStatus {
    running: bool,
    last_reason: String,
    win_count: u64,
    generation: u64,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl Automation {
    pub fn new(services: Arc<Services>, proxy: ProxyControl) -> Self {
        Self {
            services,
            proxy,
            autorun: Arc::new(Mutex::new(AutoStatus {
                profile: None,
                task: None,
                running: false,
                runs: 0,
                started: None,
                started_at: 0,
                best: 0,
                step: "-".into(),
                error: String::new(),
            })),
            tsumo: Arc::new(Mutex::new(TsumoStatus::default())),
        }
    }

    pub fn autorun_status(&self) -> Value {
        let status = self.autorun.lock().unwrap();
        let flow_ready = self.proxy.flows().iter().any(|flow| flow.is_preferred);
        let achieved = status.profile.as_ref().map(|profile| achieved_count(&self.services.game_state(), profile));
        json!({
            "active_profile":status.profile,"running":status.running,"runs":status.runs,
            "elapsed_ms":status.started.map_or(0, |started| started.elapsed().as_millis()),
            "best_achieved_count":status.best,"current_achieved_count":achieved,
            "current_step":status.step,"last_error":status.error,"started_at":status.started_at,
            "game_ready":flow_ready,
            "has_live_game":self.services.game_state().get("stage").and_then(Value::as_i64).unwrap_or(-1) >= 0,
            "game_ready_reason":"","game_ready_code":"","probe_fail_count":0,"probe_ok":true,"probe_reason":"",
            "probe_at":0,"preferred_flow_ready":flow_ready,
            "preferred_flow_peer":Value::Null,"remake_records":[],"best_remake_record":Value::Null,
            "record_detailed_operations":false,"operation_records":[],"operation_count_by_run":{},"decision_trace":[]
        })
    }

    pub fn tsumo_status(&self) -> TsumoLoopStatus {
        let status = self.tsumo.lock().unwrap();
        TsumoLoopStatus {
            running: status.running,
            last_reason: status.last_reason.clone(),
            win_count: status.win_count,
        }
    }

    pub fn start_autorun(&self) -> Result<(), &'static str> {
        let config = self.services.table("autorun");
        crate::services::playbook::validate(&config["playbook"]).map_err(|_| "autoplay.invalid_config")?;
        let profile = selected_opening_profile(&config)?.clone();
        let mut status = self.autorun.lock().unwrap();
        if status.running {
            return Ok(());
        }
        status.profile = Some(profile);
        status.best = 0;
        status.running = true;
        status.started = Some(Instant::now());
        status.started_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        status.error.clear();
        status.step = "monitoring".into();
        let this = self.clone();
        status.task = Some(tokio::spawn(async move {
            while this.autorun.lock().unwrap().running {
                this.tick().await;
                let delay = this
                    .services
                    .table("autorun")
                    .get("op_interval_ms")
                    .and_then(Value::as_u64)
                    .unwrap_or(1000)
                    .clamp(100, 30_000);
                tokio::time::sleep(Duration::from_millis(delay)).await;
            }
        }));
        drop(status);
        self.emit_autorun();
        Ok(())
    }

    pub fn stop_autorun(&self) {
        let mut status = self.autorun.lock().unwrap();
        status.running = false;
        if let Some(task) = status.task.take() {
            task.abort();
        }
        drop(status);
        self.emit_autorun();
    }

    async fn tick(&self) {
        let profile = {
            let status = self.autorun.lock().unwrap();
            if !status.running { return; }
            status.profile.clone().unwrap()
        };
        let state = self.services.game_state();
        let config = self.services.table("autorun");
        let achieved = achieved_count(&state, &profile);
        {
            let mut status = self.autorun.lock().unwrap();
            status.best = status.best.max(achieved);
            if achieved >= profile["end_count"].as_u64().unwrap() {
                status.step = "goal_met".into();
                drop(status);
                self.stop_autorun();
                let mail = config.get("email_notify").cloned().unwrap_or(Value::Null);
                if mail
                    .get("enabled")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    std::thread::spawn(move || {
                        let _ = crate::mail::send(
                            &mail,
                            "Shanten Lens 自动运行完成",
                            "目标护符已经达成。",
                        );
                    });
                }
                return;
            }
        }
        self.autorun.lock().unwrap().step = "monitoring".into();
        self.emit_autorun();
    }

    pub fn start_tsumo(&self, interval_ms: u64, reset: bool) {
        let mut status = self.tsumo.lock().unwrap();
        if status.running {
            return;
        }
        status.running = true;
        status.generation += 1;
        let generation = status.generation;
        if reset {
            status.win_count = 0;
        }
        let this = self.clone();
        status.task = Some(tokio::spawn(async move {
            loop {
                if !this.tsumo_is_current(generation) {
                    break;
                }
                let response = this
                    .proxy
                    .request_with_retry(
                        ".lq.Lobby.amuletActivityGameOperate",
                        &json!({"activityId":261081,"type":8,"tileList":[]}),
                        Duration::from_secs(12),
                    )
                    .await;
                let succeeded = {
                    let mut status = this.tsumo.lock().unwrap();
                    if !status.running || status.generation != generation {
                        break;
                    }
                    match response {
                        Ok((_, response)) if response.get("error").is_none() => {
                            status.win_count += 1;
                            status.last_reason.clear();
                            true
                        }
                        Ok((_, _)) => {
                            status.last_reason = "protocol-error".into();
                            false
                        }
                        Err(error) => {
                            status.last_reason = error;
                            false
                        }
                    }
                };
                this.emit_tsumo();
                let delay = if succeeded {
                    interval_ms.clamp(50, 10_000)
                } else {
                    5_000
                };
                tokio::time::sleep(Duration::from_millis(delay)).await;
            }
        }));
        drop(status);
        self.emit_tsumo();
    }
    pub fn stop_tsumo(&self) {
        let mut status = self.tsumo.lock().unwrap();
        status.running = false;
        status.generation += 1;
        if let Some(task) = status.task.take() {
            task.abort();
        }
        drop(status);
        self.emit_tsumo();
    }

    fn tsumo_is_current(&self, generation: u64) -> bool {
        let status = self.tsumo.lock().unwrap();
        status.running && status.generation == generation
    }
    fn emit_autorun(&self) {
        let _ = self
            .services
            .events
            .send(event("autorun_status", self.autorun_status()));
    }
    fn emit_tsumo(&self) {
        let _ = self.services.events.send(event(
            "tsumo_loop_status",
            serde_json::to_value(self.tsumo_status()).unwrap_or(Value::Null),
        ));
    }
}

fn selected_opening_profile(config: &Value) -> Result<&Value, &'static str> {
    let book = &config["playbook"];
    let profile = book["profiles"].as_array()
        .and_then(|profiles| profiles.iter().find(|profile| profile["id"] == book["selected_profile_id"]))
        .ok_or("autoplay.select_profile")?;
    if profile["strategy"] != "opening" {
        return Err("autoplay.full_not_ready");
    }
    Ok(profile)
}

fn signature(item: &Value) -> (u64, bool, Option<u64>) {
    let raw = item.get("id").and_then(Value::as_u64).unwrap_or(0);
    (
        raw / 10,
        raw % 10 == 1,
        item.get("badge")
            .and_then(|b| b.get("id"))
            .and_then(Value::as_u64),
    )
}
fn matches(item: &Value, target: &Value) -> bool {
    let (reg, plus, badge) = signature(item);
    match target.get("kind").and_then(Value::as_str) {
        Some("badge") => badge == target.get("id").and_then(Value::as_u64),
        Some("amulet") => {
            reg == target.get("id").and_then(Value::as_u64).unwrap_or(0)
                && plus == target.get("plus").and_then(Value::as_bool).unwrap_or(false)
                && target
                    .get("badge")
                    .and_then(Value::as_u64)
                    .is_none_or(|id| badge == Some(id))
        }
        _ => false,
    }
}
fn achieved_count(state: &Value, profile: &Value) -> u64 {
    let targets = profile
        .get("targets")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let effects = state
        .get("effect_list")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    targets
        .iter()
        .filter(|target| effects.iter().any(|item| matches(item, target)))
        .map(|target| target.get("value").and_then(Value::as_u64).unwrap_or(1))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_targets_come_from_the_selected_playbook_profile() {
        let mut book: Value = serde_json::from_str(include_str!("../tests/fixtures/playbook.json")).unwrap();
        book["selected_profile_id"] = json!("opening-profile");
        let config = json!({"playbook": book});
        let profile = selected_opening_profile(&config).unwrap();
        assert_eq!(profile["end_count"], 5);
        assert_eq!(profile["cutoff_level"], 13002);
        assert_eq!(profile["cutoff_node"], 4);
        assert_eq!(achieved_count(&json!({"effect_list": [{"id": 11, "badge": {"id": 600010}}]}), profile), 5);
        assert_eq!(achieved_count(&json!({"effect_list": [{"id": 10, "badge": {"id": 600010}}]}), profile), 2);
        assert_eq!(selected_opening_profile(&json!({})), Err("autoplay.select_profile"));
        let mut full = config.clone();
        full["playbook"]["selected_profile_id"] = json!("profile-a");
        assert_eq!(selected_opening_profile(&full), Err("autoplay.full_not_ready"));
    }

    #[tokio::test]
    async fn autorun_continuously_monitors_a_snapshot_of_the_started_profile() {
        let root = std::env::temp_dir().join(format!("shanten-autorun-{}-{}", std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        let (events, mut received) = tokio::sync::broadcast::channel(128);
        let services = Arc::new(Services::load(root.clone(), events).unwrap());
        let mut book: Value = serde_json::from_str(include_str!("../tests/fixtures/playbook.json")).unwrap();
        book["selected_profile_id"] = json!("opening-profile");
        services.patch_config(&json!({"autorun": {"playbook": book, "op_interval_ms": 100}})).unwrap();
        let proxy = ProxyControl::new(services.clone(), Arc::new(tokio::sync::RwLock::new(crate::pipeline::PipelineConfig::default())));
        let automation = Automation::new(services.clone(), proxy);
        automation.start_autorun().unwrap();
        let profile = automation.autorun_status()["active_profile"].clone();
        book["selected_profile_id"] = json!("profile-a");
        services.patch_config(&json!({"autorun": {"playbook": book}})).unwrap();
        assert_eq!(automation.autorun_status()["active_profile"], profile);
        services.publish_qyzz_state(&json!({
            "source": "qyzz", "qyzz_run_id": "autorun-test", "qyzz_version": 1, "qyzz_deal_id": 1,
            "deck_map": {}, "hand_tiles": [], "wall_tiles": [], "locked_tiles": [],
            "replacement_tiles": [], "dora_tiles": [],
            "effect_list": [{"id": 11, "uid": 1, "volume": 1, "store": [], "tags": [], "badge": {"id": 600010, "uid": 1, "random": 0, "store": []}}]
        })).unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let event = received.recv().await.unwrap();
                if event["type"] == "autorun_status" && event["data"]["current_step"] == "goal_met" { break; }
            }
        }).await.expect("continuous execution must reach the target without a manual step");
        let status = automation.autorun_status();
        assert_eq!(status["running"], false);
        assert_eq!(status["current_achieved_count"], 5);
        assert_eq!(status["active_profile"], profile);
        assert!(status.get("mode").is_none());
        serde_json::from_value::<crate::ipc::AutoRunnerStatus>(status).unwrap();
        assert_eq!(automation.start_autorun(), Err("autoplay.full_not_ready"));
        book["selected_profile_id"] = json!("opening-profile");
        services.patch_config(&json!({"autorun": {"playbook": book}})).unwrap();
        automation.start_autorun().unwrap();
        automation.stop_autorun();
        assert!(automation.autorun.lock().unwrap().task.is_none());
        drop(automation);
        drop(services);
        std::fs::remove_dir_all(root).unwrap();
    }
}
