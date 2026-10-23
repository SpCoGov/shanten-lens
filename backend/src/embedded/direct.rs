use super::{BackendRuntime, State};
use crate::{
    direct_connection::{self, DirectLoginOptions, LoginCredentials, Region, Route},
    services::{event, DirectAccountInput, DirectAccountSummary},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    sync::Mutex,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Serialize, Deserialize, specta::Type)]
pub struct DirectStatus {
    pub state: String,
    pub account_id: Option<String>,
    pub route: Option<String>,
    pub last_heartbeat_ms: Option<u64>,
    pub error: Option<String>,
    pub capture_account_id: Option<String>,
}

impl Default for DirectStatus {
    fn default() -> Self {
        Self {
            state: "disconnected".into(),
            account_id: None,
            route: None,
            last_heartbeat_ms: None,
            error: None,
            capture_account_id: None,
        }
    }
}

#[derive(Default)]
pub(super) struct DirectLink {
    task: tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
    status: Mutex<DirectStatus>,
    email_challenge: Mutex<Option<EmailChallenge>>,
}

#[derive(Clone)]
struct EmailChallenge {
    region: Region,
    email: String,
    device_id: String,
    expires: Instant,
}

impl State {
    pub(super) fn emit_direct_status(&self) {
        let mut status = self.direct.status.lock().unwrap().clone();
        status.capture_account_id = self.services.capture_account_id();
        let _ = self
            .events
            .send(event("direct_connection_status", json!(status)));
    }

    async fn stop_direct_operations(&self) {
        self.automation.stop_autorun();
        self.automation.stop_tsumo();
        self.switch_generation
            .send_modify(|generation| *generation += 1);
        *self.switch_plan.write().await = None;
    }

    async fn fail_direct_session(&self, error: String) {
        // Stop account-specific work before restoring the MITM transport.
        self.stop_direct_operations().await;
        self.proxy.unregister_direct();
        self.services.select_packet_transport(None);
        {
            let mut status = self.direct.status.lock().unwrap();
            status.state = "error".into();
            status.error = Some(error);
        }
        self.emit_direct_status();
    }

    async fn direct_session(&self, options: DirectLoginOptions) -> Result<(), String> {
        let connection = direct_connection::connect(options)
            .await
            .map_err(|error| error.to_string())?;
        let (processor, outgoing, local) = self
            .proxy
            .register_direct(&connection.route.endpoint, self.module_registry.clone())
            .await;
        self.direct.status.lock().unwrap().route = Some(connection.route.id);
        self.emit_direct_status();
        let socket = self.proxy.run_direct_socket(
            connection.socket,
            processor,
            outgoing,
            local,
            connection.notices,
        );
        let heartbeat = async {
            let data = self
                .proxy
                .direct_request(
                    ".lq.Lobby.fetchAmuletActivityData",
                    &json!({"activityId":261081}),
                )
                .await?;
            direct_connection::validate_response(".lq.Lobby.fetchAmuletActivityData", &data)
                .map_err(|error| error.to_string())?;
            loop {
                let data = self
                    .proxy
                    .direct_request(
                        direct_connection::HEARTBEAT_METHOD,
                        &direct_connection::heartbeat_payload(),
                    )
                    .await?;
                direct_connection::validate_response(direct_connection::HEARTBEAT_METHOD, &data)
                    .map_err(|error| error.to_string())?;
                {
                    let mut status = self.direct.status.lock().unwrap();
                    status.state = "connected".into();
                    status.last_heartbeat_ms = Some(
                        SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as u64,
                    );
                }
                self.emit_direct_status();
                tokio::time::sleep(direct_connection::HEARTBEAT_INTERVAL).await;
            }
        };
        tokio::select! { result = socket => result, result = heartbeat => result }
    }
}

impl BackendRuntime {
    pub async fn send_direct_email_code(
        &self,
        region: Region,
        email: String,
    ) -> Result<(), String> {
        let _task = self.state.direct.task.lock().await;
        let email = email.trim().to_owned();
        let device_id = crate::services::new_direct_device_id()?;
        direct_connection::yostar::Yostar::new()
            .map_err(|error| error.to_string())?
            .send_code(region, &device_id, &email)
            .await
            .map_err(|error| error.to_string())?;
        *self.state.direct.email_challenge.lock().unwrap() = Some(EmailChallenge {
            region,
            email,
            device_id,
            expires: Instant::now() + Duration::from_secs(900),
        });
        Ok(())
    }

    pub fn direct_accounts(&self) -> Vec<DirectAccountSummary> {
        self.state.services.direct_accounts()
    }

    pub async fn save_direct_account(
        &self,
        account: DirectAccountInput,
    ) -> Result<Vec<DirectAccountSummary>, String> {
        let _task = self.state.direct.task.lock().await;
        let status = self.direct_status();
        if matches!(status.state.as_str(), "connecting" | "connected")
            && account.id == status.account_id
        {
            return Err("direct.disconnect_before_edit".into());
        }
        let verification = account.verification_code.as_deref().unwrap_or("").trim();
        if account.login_method == "email" && !verification.is_empty() {
            let challenge = self
                .state
                .direct
                .email_challenge
                .lock()
                .unwrap()
                .clone()
                .filter(|challenge| {
                    challenge.email == account.username.trim()
                        && challenge.region.tag() == account.region
                        && Instant::now() < challenge.expires
                })
                .ok_or("direct.email_code_required")?;
            let session = direct_connection::yostar::Yostar::new()
                .map_err(|error| error.to_string())?
                .email_login(
                    challenge.region,
                    &challenge.device_id,
                    &challenge.email,
                    verification,
                )
                .await
                .map_err(|error| error.to_string())?;
            let result = self
                .state
                .services
                .save_direct_account_with_session(account, Some(session))?;
            *self.state.direct.email_challenge.lock().unwrap() = None;
            Ok(result)
        } else {
            self.state.services.save_direct_account(account)
        }
    }

    pub async fn delete_direct_account(
        &self,
        id: String,
    ) -> Result<Vec<DirectAccountSummary>, String> {
        let _task = self.state.direct.task.lock().await;
        let status = self.direct_status();
        if matches!(status.state.as_str(), "connecting" | "connected")
            && status.account_id.as_deref() == Some(&id)
        {
            return Err("direct.disconnect_before_edit".into());
        }
        self.state.services.delete_direct_account(&id)
    }

    pub async fn direct_routes(&self, region: Region) -> Result<Vec<Route>, String> {
        direct_connection::discover_routes(region)
            .await
            .map_err(|error| error.to_string())
    }

    pub fn direct_status(&self) -> DirectStatus {
        let mut status = self.state.direct.status.lock().unwrap().clone();
        status.capture_account_id = self.state.services.capture_account_id();
        status
    }

    pub async fn capture_direct_account(
        &self,
        account_id: Option<String>,
    ) -> Result<DirectStatus, String> {
        let _task = self.state.direct.task.lock().await;
        if account_id.is_some() && self.state.services.direct_flow_id().is_some() {
            return Err("direct.disconnect_before_capture".into());
        }
        self.state
            .services
            .capture_direct_account(account_id.as_deref())?;
        self.state.emit_direct_status();
        Ok(self.direct_status())
    }

    pub async fn connect_direct(&self, account_id: String) -> Result<DirectStatus, String> {
        let mut task = self.state.direct.task.lock().await;
        let account = self.state.services.direct_account(&account_id)?;
        let region =
            serde_json::from_value(json!(account.region)).map_err(|_| "direct.invalid_account")?;
        let email_session =
            (account.login_method == "email").then(|| direct_connection::yostar::Session {
                uid: account.uid.clone(),
                token: account.yostar_token.clone(),
                device_id: account.device_id.clone(),
            });
        let credentials = if account.login_method == "password" {
            LoginCredentials::Password {
                account: account.username,
                password: account.password,
            }
        } else if account.login_method == "email" {
            if account.yostar_token.is_empty() || account.uid.is_empty() {
                return Err("direct.email_code_required".into());
            }
            LoginCredentials::OAuth {
                uid: account.uid.clone(),
                code: account.yostar_token,
                oauth_type: if region == Region::Jp { 21 } else { 22 },
                client_version_string: String::new(),
            }
        } else {
            if account.uid.is_empty() || account.code.is_empty() {
                return Err("direct.credentials_required".into());
            }
            LoginCredentials::OAuth {
                uid: account.uid,
                code: account.code,
                oauth_type: account.oauth_type,
                client_version_string: account.client_version_string,
            }
        };
        if let Some(previous) = task.take() {
            previous.abort();
            let _ = previous.await;
        }
        self.state.stop_direct_operations().await;
        self.state.proxy.unregister_direct();
        self.state.services.capture_direct_account(None)?;
        self.state.services.select_packet_transport(Some(0));
        *self.state.direct.status.lock().unwrap() = DirectStatus {
            state: "connecting".into(),
            account_id: Some(account_id.clone()),
            ..Default::default()
        };
        self.state.emit_direct_status();
        let state = self.state.clone();
        *task = Some(tokio::spawn(async move {
            let result = async {
                let mut options = DirectLoginOptions {
                    region,
                    route_id: account.route,
                    credentials,
                    device_id: account.device_id,
                };
                if let Some(session) = email_session {
                    let next = direct_connection::yostar::Yostar::new()
                        .map_err(|error| error.to_string())?
                        .quick_login(region, &session)
                        .await
                        .map_err(|error| error.to_string())?;
                    state
                        .services
                        .update_direct_email_session(&account_id, &next)?;
                    options.credentials = LoginCredentials::OAuth {
                        uid: next.uid,
                        code: next.token,
                        oauth_type: if region == Region::Jp { 21 } else { 22 },
                        client_version_string: String::new(),
                    };
                }
                state.direct_session(options).await
            }
            .await;
            state.fail_direct_session(
                result.err().unwrap_or_else(|| "direct-connection-closed".into()),
            ).await;
        }));
        Ok(self.direct_status())
    }

    pub async fn disconnect_direct(&self) -> DirectStatus {
        let mut task = self.state.direct.task.lock().await;
        if let Some(previous) = task.take() {
            previous.abort();
            let _ = previous.await;
        }
        self.state.stop_direct_operations().await;
        self.state.proxy.unregister_direct();
        self.state.services.select_packet_transport(None);
        *self.state.direct.status.lock().unwrap() = DirectStatus::default();
        self.state.emit_direct_status();
        self.direct_status()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{pipeline::Direction, protocol::{LiqiCodec, MessageType}, runtime::{FlowProcessor, FrameOutcome}};

    #[tokio::test]
    async fn direct_failure_restores_mitm_pipeline_without_resuming_automation() {
        const CHILD_ROOT: &str = "SHANTEN_DIRECT_FAILURE_TEST_ROOT";
        let root = match std::env::var_os(CHILD_ROOT) {
            Some(root) => std::path::PathBuf::from(root),
            None => {
                let root = std::env::temp_dir().join(format!("lens-direct-failure-{}-{}",
                    std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
                // Runtime logging owns files until the child process exits.
                let output = std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", "embedded::direct::tests::direct_failure_restores_mitm_pipeline_without_resuming_automation", "--nocapture"])
                    .env(CHILD_ROOT, &root).output().unwrap();
                assert!(output.status.success(), "{output:?}");
                std::fs::remove_dir_all(root).unwrap();
                return;
            }
        };
        let runtime = tokio::task::spawn_blocking(move || BackendRuntime::load(root.join("configs")))
            .await.unwrap().unwrap();
        let state = &runtime.state;
        let config = state.pipeline.read().await.clone();
        let mut mitm = FlowProcessor::with_services(config.clone(), state.services.clone());
        let mut book: serde_json::Value = serde_json::from_str(include_str!("../../tests/fixtures/playbook.json")).unwrap();
        book["selected_profile_id"] = json!("opening-profile");
        state.services.patch_config(&json!({"autorun":{"playbook":book}})).unwrap();
        for connected in [false, true] {
            mitm.set_config(config.clone()).unwrap();
            state.services.select_packet_transport(Some(0));
            let _direct = if connected {
                Some(state.proxy.register_direct("wss://jpgs.mahjongsoul.com/gateway", state.module_registry.clone()).await)
            } else { None };
            *state.direct.status.lock().unwrap() = DirectStatus {
                state: if connected { "connected" } else { "connecting" }.into(),
                account_id: Some("test-account".into()),
                ..Default::default()
            };
            let previous_transport = state.services.packet_transport();
            let generation = *state.switch_generation.borrow();
            *state.switch_plan.write().await = Some(json!({"plan_id":"old-account"}));
            state.automation.start_autorun().unwrap();
            state.automation.start_tsumo(30_000, false);
            let logged = state.services.packet_log_snapshot().packets.len();
            let request = mitm.build_request(1, ".lq.Route.heartbeat", &json!({})).unwrap();
            let response = LiqiCodec::new().build(MessageType::Response, Some(1), ".lq.Route.heartbeat", &json!({})).unwrap();
            mitm.process(&request, Direction::Outbound).unwrap();
            assert_eq!(mitm.process(&response, Direction::Inbound).unwrap(), FrameOutcome::Forward(response.clone()));
            assert_eq!(state.services.packet_log_snapshot().packets.len(), logged);

            state.fail_direct_session("direct-connection-closed".into()).await;
            assert_eq!(state.services.direct_flow_id(), None);
            assert_eq!(runtime.direct_status().state, "error");
            assert_eq!(runtime.direct_status().error.as_deref(), Some("direct-connection-closed"));
            assert_eq!(state.automation.autorun_status()["running"], false);
            assert!(!state.automation.tsumo_status().running);
            assert!(state.switch_plan.read().await.is_none());
            assert!(*state.switch_generation.borrow() > generation);
            assert_eq!(state.services.send_on_packet_transport(previous_transport, false, || panic!("stale operation sent")), Err("data-source-changed".into()));

            let request = mitm.build_request(1, ".lq.Route.heartbeat", &json!({})).unwrap();
            assert_eq!(mitm.process(&request, Direction::Outbound).unwrap(), FrameOutcome::Forward(request));
            assert_eq!(mitm.process(&response, Direction::Inbound).unwrap(), FrameOutcome::Forward(response));
            assert_eq!(state.services.packet_log_snapshot().packets.len(), logged);
            let method = ".lq.Lobby.fetchAmuletActivityData";
            let request = mitm.build_request(2, method, &json!({"activityId":261081})).unwrap();
            mitm.process(&request, Direction::Outbound).unwrap();
            let response = LiqiCodec::new().build(MessageType::Response, Some(2), method,
                &json!({"data":{"game":{"state":{"current":5}}}})).unwrap();
            mitm.process(&response, Direction::Inbound).unwrap();
            assert_eq!(state.services.game_state()["stage"], 5);
            assert_eq!(state.services.packet_log_snapshot().packets.len(), logged + 2);

            let mut disabled_logger = config.clone();
            disabled_logger.modules.iter_mut().find(|module| module.id == "packet_logger").unwrap().enabled = false;
            mitm.set_config(disabled_logger).unwrap();
            let request = mitm.build_request(3, ".lq.Lobby.loginBeat", &json!({})).unwrap();
            mitm.process(&request, Direction::Outbound).unwrap();
            assert_eq!(state.services.packet_log_snapshot().packets.len(), logged + 2);
        }
    }
}
