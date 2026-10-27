use crate::{
    ipc::TsumoLoopStatus,
    proxy::ProxyControl,
    services::{event, Services},
};
use serde_json::{json, Value};
use hudsucker::futures::FutureExt;
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
    round_task: Option<tokio::task::JoinHandle<()>>,
    generation: u64,
    round_decision: Value,
    running: bool,
    runs: u64,
    started: Option<Instant>,
    started_at: u128,
    best: u64,
    step: String,
    error: String,
    email_status: String,
    email_error: String,
    had_live_game: bool,
    remaking: bool,
    remake_records: Vec<Value>,
    best_remake_record: Option<Value>,
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
                round_task: None,
                generation: 0,
                round_decision: json!({"status":"stopped"}),
                running: false,
                runs: 0,
                started: None,
                started_at: 0,
                best: 0,
                step: "-".into(),
                error: String::new(),
                email_status: String::new(),
                email_error: String::new(),
                had_live_game: false,
                remaking: false,
                remake_records: Vec::new(),
                best_remake_record: None,
            })),
            tsumo: Arc::new(Mutex::new(TsumoStatus::default())),
        }
    }

    pub fn autorun_status(&self) -> Value {
        let status = self.autorun.lock().unwrap();
        let flow_ready = self.proxy.flows().iter().any(|flow| flow.is_preferred);
        let achieved = status.profile.as_ref().map(|profile| achieved_count(&self.services.game_state(), profile));
        json!({
            "active_profile":status.profile,"running":status.running,"runs":status.runs,"round_decision":status.round_decision,
            "elapsed_ms":status.started.map_or(0, |started| started.elapsed().as_millis()),
            "best_achieved_count":status.best,"current_achieved_count":achieved,
            "current_step":status.step,"last_error":status.error,"started_at":status.started_at,
            "email_status":status.email_status,"email_error":status.email_error,
            "game_ready":flow_ready,
            "has_live_game":self.services.game_state().get("stage").and_then(Value::as_i64).unwrap_or(-1) >= 0,
            "game_ready_reason":"","game_ready_code":"","probe_fail_count":0,"probe_ok":true,"probe_reason":"",
            "probe_at":0,"preferred_flow_ready":flow_ready,
            "preferred_flow_peer":Value::Null,"remake_records":status.remake_records,"best_remake_record":status.best_remake_record,
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
        let profile = selected_profile(&config)?.clone();
        let mut status = self.autorun.lock().unwrap();
        if status.running {
            return Ok(());
        }
        status.profile = Some(profile);
        status.generation += 1;
        status.round_decision = json!({"status":"waiting","reason":"round.no_round"});
        status.best = 0;
        status.running = true;
        self.services.set_autorun_active(true);
        let game = self.services.game_state();
        status.had_live_game = matches!(game["stage"].as_u64(), Some(1..=99)) && game["ended"] != true;
        status.remaking = false;
        status.email_status.clear();
        status.email_error.clear();
        status.started = Some(Instant::now());
        status.started_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        status.error.clear();
        status.step = "monitoring".into();
        let this = self.clone();
        let generation = status.generation;
        let mut updates = self.services.events.subscribe();
        status.task = Some(tokio::spawn(async move {
            this.observe(generation, &this.services.game_state());
            while this.round_is_current(generation) {
                match updates.recv().await {
                    Ok(event) if event["type"] == "update_gamestate" => this.observe(generation, &this.services.game_state()),
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => this.observe(generation, &this.services.game_state()),
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    _ => {}
                }
            }
        }));
        drop(status);
        self.emit_autorun();
        Ok(())
    }

    pub fn stop_autorun(&self) {
        let mut status = self.autorun.lock().unwrap();
        if !status.running { return; }
        status.running = false;
        self.services.set_autorun_active(false);
        status.generation += 1;
        status.round_decision["status"] = json!("stopped");
        if let Some(task) = status.round_task.take() {
            task.abort();
        }
        if let Some(task) = status.task.take() {
            task.abort();
        }
        drop(status);
        self.emit_autorun();
    }

    pub(crate) fn observe(&self, generation: u64, state: &Value) {
        let mut status = self.autorun.lock().unwrap();
        if !status.running || status.generation != generation { return; }
        let profile = status.profile.clone().unwrap();
        let achieved = achieved_count(state, &profile);
        status.best = status.best.max(achieved);
        let live = matches!(state["stage"].as_u64(), Some(1..=99)) && state["ended"] != true;
        let complete = if profile["strategy"] == "opening" && achieved >= profile["end_count"].as_u64().unwrap() {
            Some("goal_met")
        } else if profile["strategy"] != "opening" && status.had_live_game && !status.remaking
            && (state["stage"] == 100 || state["ended"] == true) {
            Some("completed")
        } else { None };
        if live {
            status.had_live_game = true;
            // A giveup response ends the old game; only a new game's live state clears this flag.
            if state["stage"] == 1 { status.remaking = false; }
        }
        drop(status);
        if let Some(step) = complete { self.finish(generation, step, ""); return; }
        self.emit_autorun();
    }

    pub(crate) fn begin_action(&self, generation: u64, kind: &str) -> bool {
        let mut status = self.autorun.lock().unwrap();
        if !status.running || status.generation != generation { return false; }
        status.step = format!("action:{kind}");
        if kind == "remake" { status.remaking = true; }
        if kind == "start_game" { status.runs += 1; }
        drop(status);
        self.emit_autorun();
        true
    }

    pub(crate) fn record_remake(&self, generation: u64, game: &Value, reason: &str) {
        let mut status = self.autorun.lock().unwrap();
        if !status.running || status.generation != generation { return; }
        if status.remake_records.last().is_some_and(|record| record["session_id"] == game["session_id"] && record["source"] == game["source"]) {
            return;
        }
        let effects = game["effect_list"].as_array().cloned().unwrap_or_default();
        let profile = status.profile.as_ref().unwrap();
        let record = json!({
            "seq":status.remake_records.last().map_or(1, |record| record["seq"].as_u64().unwrap() + 1),
            "run_index":status.runs.max(1),
            "ts":SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis(),
            "reason":reason,"stage":game["stage"],"level":game["level"],"node":game["node"],
            "source":game["source"],"session_id":game["session_id"],
            "profile_id":profile["id"],"profile_name":profile["name"],
            "target_value":achieved_count(game, profile),"amulet_count":effects.len(),"effect_list":effects,
        });
        if status.best_remake_record.as_ref().is_none_or(|best| best["target_value"].as_u64() < record["target_value"].as_u64()) {
            status.best_remake_record = Some(record.clone());
        }
        status.remake_records.push(record);
        if status.remake_records.len() > 200 { status.remake_records.remove(0); }
        drop(status);
        self.emit_autorun();
    }

    pub(crate) fn fail(&self, generation: u64, reason: &str) {
        self.finish(generation, "failed", reason);
    }

    fn finish(&self, generation: u64, step: &str, reason: &str) {
        let mut status = self.autorun.lock().unwrap();
        if !status.running || status.generation != generation { return; }
        status.running = false;
        status.generation += 1;
        let notification_generation = status.generation;
        status.step = step.into();
        status.error = reason.into();
        self.services.set_autorun_active(false);
        if step != "failed" { status.round_decision["status"] = json!("stopped"); }
        if let Some(task) = status.round_task.take() { task.abort(); }
        if let Some(task) = status.task.take() { task.abort(); }
        let mail = self.services.table("autorun")["email_notify"].clone();
        let enabled = mail["enabled"] == true;
        status.email_status = if enabled { "sending" } else { "disabled" }.into();
        let outcome = match step { "failed" => "运行出错，已停止", "goal_met" => "开局目标已达成", _ => "本局已结束" };
        let subject = format!("Shanten Lens 全自动：{outcome}");
        let body = format!("玩法：{}\n结果：{outcome}\n已运行：{} 秒\n{}",
            status.profile.as_ref().and_then(|p| p["name"].as_str()).unwrap_or(""),
            status.started.map_or(0, |start| start.elapsed().as_secs()), reason);
        drop(status);
        self.emit_autorun();
        if enabled {
            let this = self.clone();
            std::thread::spawn(move || {
                let result = crate::mail::send(&mail, &subject, &body);
                let mut status = this.autorun.lock().unwrap();
                if status.generation != notification_generation { return; }
                match result {
                    Ok(()) => status.email_status = "sent".into(),
                    Err(error) => { status.email_status = "failed".into(); status.email_error = error; }
                }
                drop(status);
                this.emit_autorun();
            });
        }
    }

    pub(crate) fn round_context(&self) -> Option<(u64, Value)> {
        let status = self.autorun.lock().unwrap();
        status.running.then(|| (status.generation, status.profile.clone().unwrap()))
    }

    pub(crate) fn round_is_current(&self, generation: u64) -> bool {
        let status = self.autorun.lock().unwrap();
        status.running && status.generation == generation
    }

    pub(crate) fn start_round_task(&self, generation: u64, task: impl std::future::Future<Output = ()> + Send + 'static) {
        let mut status = self.autorun.lock().unwrap();
        if status.running && status.generation == generation && status.round_task.is_none() {
            let this = self.clone();
            status.round_task = Some(tokio::spawn(async move {
                if std::panic::AssertUnwindSafe(task).catch_unwind().await.is_err() {
                    this.fail(generation, "autoplay.worker_failed");
                }
            }));
        }
    }

    pub(crate) fn set_round_decision(&self, generation: u64, report: Value) {
        let mut status = self.autorun.lock().unwrap();
        if !status.running || status.generation != generation || status.round_decision == report { return; }
        let error = (report["status"] == "blocked").then(|| report["reason"].as_str().unwrap_or("round.no_reachable_hand").to_owned());
        status.round_decision = report;
        drop(status);
        if let Some(error) = error { self.fail(generation, &error); return; }
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

fn selected_profile(config: &Value) -> Result<&Value, &'static str> {
    let book = &config["playbook"];
    let profile = book["profiles"].as_array()
        .and_then(|profiles| profiles.iter().find(|profile| profile["id"] == book["selected_profile_id"]))
        .ok_or("autoplay.select_profile")?;
    Ok(profile)
}

fn signature(item: &Value) -> (u64, bool, Option<u64>) {
    let raw = item.get("id").and_then(Value::as_u64).unwrap_or(0);
    (
        raw / 10,
        raw % 10 == 1,
        item.get("badgeId")
            .or_else(|| item.pointer("/badge/id"))
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

    #[tokio::test]
    async fn remake_history_keeps_snapshots_best_value_and_a_bounded_recent_list() {
        let root = std::env::temp_dir().join(format!("lens-remake-history-{}-{}", std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        let (events, _) = tokio::sync::broadcast::channel(128);
        let services = Arc::new(Services::load(root.clone(), events).unwrap());
        let mut book: Value = serde_json::from_str(include_str!("../tests/fixtures/playbook.json")).unwrap();
        book["selected_profile_id"] = json!("opening-profile");
        services.patch_config(&json!({"autorun":{"playbook":book}})).unwrap();
        let proxy = ProxyControl::new(services.clone(), Arc::new(tokio::sync::RwLock::new(crate::pipeline::PipelineConfig::default())));
        let automation = Automation::new(services.clone(), proxy);
        automation.start_autorun().unwrap();
        let (generation, _) = automation.round_context().unwrap();
        let mut game = json!({"source":"packet","session_id":1,"stage":9,"level":13002,"node":4,
            "effect_list":[{"id":11,"uid":1,"badge":{"id":600010}}]});
        automation.record_remake(generation, &game, "remake_at_cutoff");
        automation.record_remake(generation, &game, "duplicate");
        assert_eq!(automation.autorun_status()["remake_records"].as_array().unwrap().len(), 1);
        game["effect_list"] = json!([]);
        for session in 2..=205 {
            game["session_id"] = json!(session);
            automation.record_remake(generation, &game, "custom-search-limit");
        }
        let status = automation.autorun_status();
        let records = status["remake_records"].as_array().unwrap();
        assert_eq!(records.len(), 200);
        assert_eq!(records[0]["seq"], 6);
        assert_eq!(records.last().unwrap()["seq"], 205);
        let best = &status["best_remake_record"];
        assert_eq!(best["seq"], 1, "the best record must survive recent-history eviction");
        assert_eq!(best["target_value"], 5);
        assert_eq!(best["amulet_count"], 1);
        assert_eq!(best["effect_list"][0]["id"], 11, "later state changes must not alter the snapshot");
        assert_eq!(best["node"], 4);
        assert_eq!(best["profile_id"], "opening-profile");
        assert!(best["ts"].as_u64().unwrap() > 0);
        serde_json::from_value::<crate::ipc::AutoRunnerStatus>(status).unwrap();
        automation.stop_autorun();
        automation.start_autorun().unwrap();
        automation.record_remake(generation, &game, "stale-worker");
        assert_eq!(automation.autorun_status()["remake_records"].as_array().unwrap().len(), 200);
        assert_eq!(automation.autorun_status()["remake_records"][199]["seq"], 205);
        automation.stop_autorun();
        drop(automation);
        drop(services);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn terminal_states_notify_once_restore_fuses_and_do_not_complete_remakes() {
        let root = std::env::temp_dir().join(format!("lens-autorun-lifecycle-{}-{}", std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        let (events, _) = tokio::sync::broadcast::channel(128);
        let services = Arc::new(Services::load(root.clone(), events).unwrap());
        let book: Value = serde_json::from_str(include_str!("../tests/fixtures/playbook.json")).unwrap();
        let mut mail = services.table("autorun")["email_notify"].clone();
        mail["enabled"] = json!(true);
        services.patch_config(&json!({"autorun":{"playbook":book,"email_notify":mail}})).unwrap();
        let proxy = ProxyControl::new(services.clone(), Arc::new(tokio::sync::RwLock::new(crate::pipeline::PipelineConfig::default())));
        let automation = Automation::new(services.clone(), proxy);
        for (step, error) in [("completed", ""), ("failed", "test-error")] {
            automation.start_autorun().unwrap();
            let (generation, _) = automation.round_context().unwrap();
            assert!(services.autorun_active());
            automation.observe(generation, &json!({"stage":100,"ended":true}));
            assert!(automation.round_is_current(generation), "initial lobby is not completion");
            automation.observe(generation, &json!({"stage":1}));
            automation.begin_action(generation, "remake");
            automation.observe(generation, &json!({"stage":100,"ended":true}));
            assert!(automation.round_is_current(generation), "remake must continue with a new game");
            automation.observe(generation, &json!({"stage":1}));
            if error.is_empty() { automation.observe(generation, &json!({"stage":100,"ended":true})); }
            else { automation.fail(generation, error); }
            assert!(!services.autorun_active());
            automation.fail(generation, "duplicate-error");
            automation.stop_autorun(); // Cleanup must not invalidate an in-flight notification.
            tokio::time::timeout(Duration::from_secs(2), async {
                while automation.autorun_status()["email_status"] == "sending" {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }).await.unwrap();
            let status = automation.autorun_status();
            assert_eq!(status["current_step"], step);
            assert_eq!(status["last_error"], error);
            assert_eq!(status["email_status"], "failed");
            assert_eq!(status["email_error"], "email-not-configured"); // No real SMTP traffic.
        }
        automation.start_autorun().unwrap();
        automation.stop_autorun();
        assert!(!services.autorun_active());
        assert_eq!(automation.autorun_status()["email_status"], "", "manual pause must not send mail");
        drop(automation);
        drop(services);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn opening_targets_come_from_the_selected_playbook_profile() {
        let mut book: Value = serde_json::from_str(include_str!("../tests/fixtures/playbook.json")).unwrap();
        book["selected_profile_id"] = json!("opening-profile");
        let config = json!({"playbook": book});
        let profile = selected_profile(&config).unwrap();
        assert_eq!(profile["end_count"], 5);
        assert_eq!(profile["cutoff_level"], 13002);
        assert_eq!(profile["cutoff_node"], 4);
        assert_eq!(achieved_count(&json!({"effect_list": [{"id": 11, "badge": {"id": 600010}}]}), profile), 5);
        assert_eq!(achieved_count(&json!({"effect_list": [{"id": 11, "badgeId": 600010}]}), profile), 5);
        assert_eq!(achieved_count(&json!({"effect_list": [{"id": 10, "badge": {"id": 600010}}]}), profile), 2);
        assert_eq!(selected_profile(&json!({})), Err("autoplay.select_profile"));
        let mut full = config.clone();
        full["playbook"]["selected_profile_id"] = json!("profile-a");
        assert_eq!(selected_profile(&full).unwrap()["id"], "profile-a");
    }

    #[tokio::test]
    async fn opening_pauses_on_operation_updates_without_waiting_for_the_interval() {
        use crate::pipeline::{Direction, Packet};
        let root = std::env::temp_dir().join(format!("shanten-opening-pause-{}-{}", std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        let (events, mut received) = tokio::sync::broadcast::channel(128);
        let services = Arc::new(Services::load(root.clone(), events).unwrap());
        let mut book: Value = serde_json::from_str(include_str!("../tests/fixtures/playbook.json")).unwrap();
        book["selected_profile_id"] = json!("opening-profile");
        services.patch_config(&json!({"autorun": {"playbook": book, "op_interval_ms": 30_000}})).unwrap();
        services.update_game_state(&Packet {
            direction: Direction::Inbound, packet_type: "Res".into(), id: Some(1),
            method: ".lq.Lobby.fetchAmuletActivityData".into(),
            data: json!({"game":{"state":{"current":3,"interrupt":0},"effect":{"effectList":[]}}}),
        });
        let proxy = ProxyControl::new(services.clone(), Arc::new(tokio::sync::RwLock::new(crate::pipeline::PipelineConfig::default())));
        let automation = Automation::new(services.clone(), proxy);
        automation.start_autorun().unwrap();
        // Wait for the initial check so later updates cannot pass via startup instead.
        tokio::time::timeout(Duration::from_secs(2), async {
            let mut checks = 0;
            while checks < 2 {
                let event = received.recv().await.unwrap();
                if event["type"] == "autorun_status" && event["data"]["current_step"] == "monitoring" { checks += 1; }
            }
        }).await.unwrap();
        for (id, achieved, running) in [(10, 2, true), (11, 5, false)] {
            services.update_game_state(&Packet {
                direction: Direction::Inbound, packet_type: "Res".into(), id: Some(2),
                method: ".lq.Lobby.amuletActivityOperate".into(),
                data: json!({"events":[{"state":{"current":3,"interrupt":0},"valueChanges":{
                    "effect":{"effectList":{"dirty":true,"value":[{"id":id,"uid":1,"volume":1,"badgeId":600010}]}}
                }}]}),
            });
            let status = tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    let event = received.recv().await.unwrap();
                    if event["type"] == "autorun_status" && event["data"]["current_achieved_count"] == achieved {
                        break event["data"].clone();
                    }
                }
            }).await.expect("each operation must be checked before the next operation interval");
            assert_eq!(status["running"], running);
            assert_eq!(status["best_achieved_count"], achieved);
            assert_eq!(status["current_step"], if running { "monitoring" } else { "goal_met" });
        }
        assert!(automation.autorun.lock().unwrap().task.is_none());
        drop(automation);
        drop(services);
        std::fs::remove_dir_all(root).unwrap();
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
        automation.start_autorun().unwrap();
        let (generation, _) = automation.round_context().unwrap();
        let (sender, receiver) = tokio::sync::oneshot::channel::<()>();
        automation.start_round_task(generation, async move { let _sender = sender; std::future::pending::<()>().await; });
        automation.observe(generation, &services.game_state());
        assert_eq!(automation.autorun_status()["running"], true, "full runs must not use the opening goal");
        automation.stop_autorun();
        assert!(!automation.round_is_current(generation));
        assert!(tokio::time::timeout(Duration::from_secs(1), receiver).await.unwrap().is_err());
        book["selected_profile_id"] = json!("opening-profile");
        services.patch_config(&json!({"autorun": {"playbook": book}})).unwrap();
        automation.start_autorun().unwrap();
        let old = automation.autorun_status()["round_decision"].clone();
        automation.set_round_decision(generation, json!({"status":"winning"}));
        assert_eq!(automation.autorun_status()["round_decision"], old, "stale workers cannot overwrite a restarted run");
        automation.stop_autorun();
        assert!(automation.autorun.lock().unwrap().task.is_none());
        drop(automation);
        drop(services);
        std::fs::remove_dir_all(root).unwrap();
    }
}
