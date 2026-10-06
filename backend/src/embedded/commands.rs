use super::{envelope, BackendRuntime, State};
use crate::{
    ipc::{
        AmuletAction, AmuletActionRequest, AutorunAction, CommandResult, ConfigTables,
        FlowDumpResult, JsonMap, PacketLogSnapshot, PacketRecordingStatus, TsumoLoopStatus,
    },
    pipeline::{self, PacketModuleInfo, PipelineConfig, GAME_RECORD},
    runtime::{normalize_config, register_builtin_modules},
    services::event,
};
use serde_json::{json, Value};
use std::time::Duration;
use tracing::error;

impl BackendRuntime {
    pub fn data_sources(&self) -> crate::ipc::DataSourceStatus {
        self.state.services.data_source_status()
    }

    pub async fn select_data_source(&self, source: crate::ipc::DataSource, revision: u64) -> Result<crate::ipc::DataSourceStatus, String> {
        let status = self.state.services.select_data_source(source, revision)?;
        self.state.automation.stop_autorun();
        self.state.automation.stop_tsumo();
        self.state.switch_generation.send_modify(|generation| *generation += 1);
        *self.state.switch_plan.write().await = None;
        Ok(status)
    }

    pub async fn packet_pipeline(&self) -> PipelineConfig {
        self.state.pipeline.read().await.clone()
    }

    pub fn packet_modules(&self) -> Vec<PacketModuleInfo> {
        self.state.module_registry.snapshot()
    }

    pub async fn set_packet_pipeline(&self, config: PipelineConfig) -> CommandResult {
        let config = normalize_config(config);
        let mut current = self.state.pipeline.write().await;
        match pipeline::validate(&config)
            .and_then(|_| pipeline::save(&self.state.pipeline_path, &config))
        {
            Ok(()) => {
                *current = config.clone();
                drop(current);
                register_builtin_modules(&self.state.module_registry, &config);
                let _ = self.state.events.send(envelope(
                    "packet_pipeline",
                    serde_json::to_value(&config).unwrap_or(Value::Null),
                ));
                let _ = self.state.events.send(envelope(
                    "packet_modules",
                    serde_json::to_value(self.packet_modules()).unwrap_or(Value::Null),
                ));
                CommandResult {
                    ok: true,
                    ..Default::default()
                }
            }
            Err(error) => CommandResult {
                error: Some(error),
                ..Default::default()
            },
        }
    }

    pub fn packet_log(&self) -> PacketLogSnapshot {
        self.state.services.packet_log_snapshot()
    }

    pub fn packet_recording_status(&self) -> PacketRecordingStatus {
        self.state.services.packet_recording_status()
    }

    pub fn set_packet_recording(&self, active: bool) -> Result<PacketRecordingStatus, String> {
        self.state.services.set_packet_recording(active)
    }

    pub async fn replay_packet(&self, method: String, payload: JsonMap) -> CommandResult {
        if method.is_empty() {
            return failed("invalid-payload");
        }
        if !self.state.services.can_replay(&method) {
            return failed("packet-not-observed");
        }
        let config = self.state.pipeline.read().await.clone();
        match self
            .state
            .proxy
            .replay(
                &method,
                &Value::Object(payload.into_iter().collect()),
                Duration::from_secs(12),
                config,
            )
            .await
        {
            Ok((id, response)) => CommandResult {
                ok: response.get("error").is_none(),
                reason: response
                    .get("error")
                    .is_some()
                    .then(|| "protocol-error".into()),
                msg_id: Some(id),
                response: Some(response),
                ..Default::default()
            },
            Err(error) => failed(error),
        }
    }

    pub async fn fetch_game_record(&self, game_uuid: String) -> CommandResult {
        let game_uuid = game_uuid.trim();
        if game_uuid.is_empty() || game_uuid.len() > 200 {
            return failed("invalid-payload");
        }
        let config = self.state.pipeline.read().await.clone();
        match self
            .state
            .proxy
            .inject(
                ".lq.Lobby.fetchGameRecord",
                &json!({
                    "clientVersionString": "StandaloneWindows_2022-0.16.273",
                    "gameUuid": game_uuid,
                }),
                Duration::from_secs(12),
                config,
                GAME_RECORD,
            )
            .await
        {
            Ok((id, response)) => CommandResult {
                ok: response.get("error").is_none(),
                reason: response
                    .get("error")
                    .is_some()
                    .then(|| "protocol-error".into()),
                msg_id: Some(id),
                response: Some(response),
                ..Default::default()
            },
            Err(error) => failed(error),
        }
    }

    pub fn override_game_record(&self, record: Value) -> CommandResult {
        match self.state.services.arm_game_record_override(&record) {
            Ok(()) => CommandResult {
                ok: true,
                ..Default::default()
            },
            Err(error) => {
                let reason = format!("{error:#}");
                error!(target: "shanten_backend::game_record", error = %reason, "failed to arm game record override");
                failed(reason)
            }
        }
    }

    pub fn update_config(&self, config: ConfigTables) -> Result<(), String> {
        let config = serde_json::to_value(config).map_err(|error| error.to_string())?;
        let result = self.state.services.patch_config(&config);
        for update in [
            event("update_fuse_config", self.state.services.table("fuse")),
            event(
                "update_autorun_config",
                self.state.services.table("autorun"),
            ),
            event("update_config", self.state.services.config_payload()),
        ] {
            let _ = self.state.events.send(update);
        }
        result
    }

    pub fn set_locale(&self, locale: String) {
        self.state.services.set_locale(&locale);
    }

    pub fn dump_flows(&self) -> FlowDumpResult {
        let flows = self.state.proxy.flows();
        FlowDumpResult {
            ok: true,
            count: flows.len(),
            flows,
        }
    }

    pub async fn fetch_activity(&self, activity_id: u64) -> CommandResult {
        request_reply(
            &self.state,
            ".lq.Lobby.fetchAmuletActivityData",
            json!({"activityId": activity_id}),
        )
        .await
    }

    pub async fn discard_tile(&self, tile_id: u64) -> CommandResult {
        if tile_id == 0 {
            return failed("invalid-tile-id");
        }
        request_reply(
            &self.state,
            ".lq.Lobby.amuletActivityGameOperate",
            json!({"activityId":260511,"type":1,"tileList":[tile_id]}),
        )
        .await
    }

    pub async fn upgrade_shop_buff(&self, activity_id: u64, id: u64) -> CommandResult {
        request_reply(
            &self.state,
            ".lq.Lobby.amuletActivityUpgradeShopBuff",
            json!({"activityId":activity_id,"id":id}),
        )
        .await
    }

    pub async fn amulet_action(&self, request: AmuletActionRequest) -> CommandResult {
        hotkey_reply(&self.state, request).await
    }

    pub fn start_tsumo_loop(&self, interval_ms: u64, reset_count: bool) -> TsumoLoopStatus {
        self.state.automation.start_tsumo(interval_ms, reset_count);
        self.state.automation.tsumo_status()
    }

    pub fn stop_tsumo_loop(&self) -> TsumoLoopStatus {
        self.state.automation.stop_tsumo();
        self.state.automation.tsumo_status()
    }

    pub async fn autorun(
        &self,
        action: AutorunAction,
        force: bool,
        mode: Option<String>,
    ) -> CommandResult {
        let result = match action {
            AutorunAction::Start => {
                let current = self.state.services.game_state();
                let has_live = current.get("stage").and_then(Value::as_i64).unwrap_or(-1) >= 0
                    && !current
                        .get("ended")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                if has_live && !force {
                    CommandResult {
                        requires_confirmation: Some(true),
                        reason: Some("existing-live-game".into()),
                        ..Default::default()
                    }
                } else {
                    self.state.automation.start_autorun(has_live && force);
                    CommandResult {
                        ok: true,
                        ..Default::default()
                    }
                }
            }
            AutorunAction::Stop => {
                self.state.automation.stop_autorun();
                CommandResult {
                    ok: true,
                    ..Default::default()
                }
            }
            AutorunAction::SetMode => {
                self.state
                    .automation
                    .set_mode(mode.as_deref().unwrap_or("continuous"));
                CommandResult {
                    ok: true,
                    ..Default::default()
                }
            }
            AutorunAction::Step => {
                self.state.automation.tick().await;
                CommandResult {
                    ok: true,
                    ..Default::default()
                }
            }
            AutorunAction::Probe => match self
                .state
                .proxy
                .request_with_retry(
                    ".lq.Lobby.fetchAmuletActivityData",
                    &json!({"activityId":260511}),
                    Duration::from_secs(12),
                )
                .await
            {
                Ok(_) => CommandResult {
                    ok: true,
                    ..Default::default()
                },
                Err(error) => failed(error),
            },
            AutorunAction::NotifyTestEmail => {
                let config = self
                    .state
                    .services
                    .table("autorun")
                    .get("email_notify")
                    .cloned()
                    .unwrap_or(Value::Null);
                match tokio::task::spawn_blocking(move || {
                    shanten_backend::mail::send(
                        &config,
                        "Shanten Lens 测试通知",
                        "Rust 3.0 后端邮件通知工作正常。",
                    )
                })
                .await
                {
                    Ok(Ok(())) => CommandResult {
                        ok: true,
                        ..Default::default()
                    },
                    Ok(Err(error)) => failed(error),
                    Err(error) => failed(error.to_string()),
                }
            }
        };
        let _ = self.state.events.send(event(
            "autorun_status",
            self.state.automation.autorun_status(),
        ));
        result
    }

    pub fn resolve_confirmation(&self, id: String, ok: bool) {
        self.state.services.resolve_confirmation(&id, ok);
    }
}

async fn request_reply(state: &State, method: &str, payload: Value) -> CommandResult {
    match state
        .proxy
        .request_with_retry(method, &payload, Duration::from_secs(12))
        .await
    {
        Ok((id, response)) => CommandResult {
            ok: response.get("error").is_none(),
            reason: response
                .get("error")
                .is_some()
                .then(|| "protocol-error".into()),
            msg_id: Some(id),
            response: Some(response),
            ..Default::default()
        },
        Err(error) => failed(error),
    }
}

async fn hotkey_reply(state: &State, request: AmuletActionRequest) -> CommandResult {
    let action = request.action.as_str();
    let (method, payload) = match request.action {
        AmuletAction::BuyPack => (
            ".lq.Lobby.amuletActivityOperate",
            json!({"activityId":260511,"type":4,"args":[request.good_id.unwrap_or(0)]}),
        ),
        AmuletAction::RefreshShop => (
            ".lq.Lobby.amuletActivityOperate",
            json!({"activityId":260511,"type":9,"args":[]}),
        ),
        AmuletAction::Skip => (
            ".lq.Lobby.amuletActivityOperate",
            json!({"activityId":260511,"type":3,"args":[]}),
        ),
        AmuletAction::SelectCandidate => (
            ".lq.Lobby.amuletActivityOperate",
            json!({"activityId":260511,"type":16,"args":[request.selected_index.unwrap_or(0)]}),
        ),
        AmuletAction::SellEffect => (
            ".lq.Lobby.amuletActivityOperate",
            json!({"activityId":260511,"type":5,"args":[request.uid.unwrap_or(0)]}),
        ),
        AmuletAction::SortEffect => (
            ".lq.Lobby.amuletActivityOperate",
            json!({"activityId":260511,"type":8,"args":request.sorted_uid.unwrap_or_default()}),
        ),
        AmuletAction::SellRecent => {
            let state_value = state.services.game_state();
            let raw_id = request.raw_id;
            let uid = state_value
                .get("effect_list")
                .and_then(Value::as_array)
                .and_then(|items| {
                    items.iter().rev().find(|item| {
                        raw_id.is_none() || item.get("id").and_then(Value::as_u64) == raw_id
                    })
                })
                .and_then(|item| item.get("uid"))
                .and_then(Value::as_u64)
                .unwrap_or(0);
            (
                ".lq.Lobby.amuletActivityOperate",
                json!({"activityId":260511,"type":5,"args":[uid]}),
            )
        }
    };
    let mut result = request_reply(state, method, payload).await;
    result.action = Some(action.into());
    result
}

fn failed(reason: impl Into<String>) -> CommandResult {
    CommandResult {
        reason: Some(reason.into()),
        ..Default::default()
    }
}
