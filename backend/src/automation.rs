use crate::{
    ipc::TsumoLoopStatus,
    proxy::ProxyControl,
    services::{event, Services},
};
use serde_json::{json, Value};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone)]
pub struct Automation {
    services: Arc<Services>,
    proxy: ProxyControl,
    autorun: Arc<Mutex<AutoStatus>>,
    autorun_running: Arc<AtomicBool>,
    tsumo: Arc<Mutex<TsumoStatus>>,
}

struct AutoStatus {
    mode: String,
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
                mode: "continuous".into(),
                running: false,
                runs: 0,
                started: None,
                started_at: 0,
                best: 0,
                step: "-".into(),
                error: String::new(),
            })),
            autorun_running: Arc::new(AtomicBool::new(false)),
            tsumo: Arc::new(Mutex::new(TsumoStatus::default())),
        }
    }

    pub fn autorun_status(&self) -> Value {
        let status = self.autorun.lock().unwrap();
        let achieved = achieved_count(&self.services.game_state(), &self.services.table("autorun"));
        json!({
            "mode":status.mode,"running":status.running,"runs":status.runs,
            "elapsed_ms":status.started.map_or(0, |started| started.elapsed().as_millis()),
            "best_achieved_count":status.best,"current_achieved_count":achieved,
            "current_step":status.step,"last_error":status.error,"started_at":status.started_at,
            "game_ready":!self.proxy.flows().is_empty(),
            "has_live_game":self.services.game_state().get("stage").and_then(Value::as_i64).unwrap_or(-1) >= 0,
            "game_ready_reason":"","game_ready_code":"","probe_fail_count":0,"probe_ok":true,"probe_reason":"",
            "probe_at":0,"preferred_flow_ready":!self.proxy.flows().is_empty(),
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

    pub fn set_mode(&self, mode: &str) {
        self.autorun.lock().unwrap().mode =
            if mode == "step" { "step" } else { "continuous" }.into();
        self.emit_autorun();
    }

    pub fn start_autorun(&self, _continue_existing: bool) {
        if self.autorun_running.swap(true, Ordering::SeqCst) {
            return;
        }
        {
            let mut status = self.autorun.lock().unwrap();
            status.running = true;
            status.started = Some(Instant::now());
            status.started_at = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            status.error.clear();
            status.step = "monitoring".into();
        }
        self.emit_autorun();
        if self.autorun.lock().unwrap().mode == "continuous" {
            let this = self.clone();
            tokio::spawn(async move {
                while this.autorun_running.load(Ordering::SeqCst) {
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
            });
        }
    }

    pub fn stop_autorun(&self) {
        self.autorun_running.store(false, Ordering::SeqCst);
        let mut status = self.autorun.lock().unwrap();
        status.running = false;
        drop(status);
        self.emit_autorun();
    }

    pub async fn tick(&self) {
        if !self.autorun_running.load(Ordering::SeqCst) {
            return;
        }
        let state = self.services.game_state();
        let config = self.services.table("autorun");
        let achieved = achieved_count(&state, &config);
        {
            let mut status = self.autorun.lock().unwrap();
            status.best = status.best.max(achieved);
            if achieved >= config.get("end_count").and_then(Value::as_u64).unwrap_or(1) {
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
                        &json!({"activityId":260511,"type":8,"tileList":[]}),
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
fn achieved_count(state: &Value, config: &Value) -> u64 {
    let targets = config
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
