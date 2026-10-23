//! Executes one current provider candidate, without scoring or automatic retries.
use crate::{action_provider::{ActionCandidate, Availability}, ipc::CommandResult, proxy::ProxyControl, services::Services};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use specta::Type;
use std::time::Duration;

#[derive(Clone, Debug, Deserialize, Serialize, Type)]
pub struct ActionExecutionRequest {
    pub action_id: String,
    pub session_id: u64,
    pub revision: u64,
}

#[derive(Default)]
pub(crate) struct ActionExecutor {
    last_sent: tokio::sync::Mutex<Option<(u64, u64)>>,
}

impl ActionExecutor {
    pub(crate) async fn execute(&self, services: &Services, proxy: &ProxyControl, request: ActionExecutionRequest) -> CommandResult {
        let Ok(mut last_sent) = self.last_sent.try_lock() else { return failed("busy"); };
        let stamp = (request.session_id, request.revision);
        if *last_sent == Some(stamp) { return failed("awaiting_state"); }
        let current = services.game_state();
        let action = match candidate(&current, &request) { Ok(action) => action, Err(reason) => return failed(reason) };
        let (method, payload) = match command(&action, &current) { Ok(command) => command, Err(reason) => return failed(reason) };
        let result = proxy.request_action(method, &payload, Duration::from_secs(12), &request, &mut last_sent).await;
        let submitted = *last_sent == Some(stamp);
        let mut result = match result {
            Ok((id, response)) => CommandResult {
                ok: response.get("error").is_none(),
                reason_key: response.get("error").map(|_| "action_provider.execution.rejected".into()),
                reason_values: Some([("code".into(), response["error"]["code"].clone())].into()),
                msg_id: Some(id), response: Some(response), ..Default::default()
            },
            Err(reason) => {
                let key = match reason.as_str() {
                    "stale" | "unavailable" | "unsupported" => reason.as_str(),
                    "packet-dropped-by-pipeline" => "blocked",
                    _ if submitted => "uncertain",
                    _ => "send_failed",
                };
                let mut result = failed(key);
                result.reason = Some(reason);
                result
            }
        };
        result.action = Some(action.kind);
        result
    }
}

fn failed(reason: &str) -> CommandResult {
    CommandResult { reason_key: Some(format!("action_provider.execution.{reason}")), ..Default::default() }
}

pub(crate) fn candidate(state: &Value, request: &ActionExecutionRequest) -> Result<ActionCandidate, &'static str> {
    if state["source"] != "packet" { return Err("unsupported"); }
    if state["session_id"].as_u64() != Some(request.session_id) || state["revision"].as_u64() != Some(request.revision) {
        return Err("stale");
    }
    let report = &state["action_report"];
    let action = ["actions", "global_actions"].iter()
        .filter_map(|key| report[key].as_array()).flatten()
        .find(|action| action["id"].as_str() == Some(&request.action_id)).ok_or("stale")?;
    let action: ActionCandidate = serde_json::from_value(action.clone()).map_err(|_| "unavailable")?;
    if action.availability != Availability::Available { return Err("unavailable"); }
    Ok(action)
}

fn command(action: &ActionCandidate, state: &Value) -> Result<(&'static str, Value), &'static str> {
    let activity = crate::action_provider::activity_id(state).ok_or("unsupported")?;
    let param = |key: &str| action.params[key].as_u64().and_then(|id| u32::try_from(id).ok()).ok_or("invalid_action");
    let (kind, args) = match action.kind.as_str() {
        "start_game" => return Ok((".lq.Lobby.amuletActivityStartGame", json!({"activityId":activity}))),
        "remake" => return Ok((".lq.Lobby.amuletActivityGiveup", json!({"activityId":activity}))),
        "select_character" => (1, vec![param("id")?]),
        "next_node" if state["stage"] == 3 => (2, vec![]),
        "next_node" if state["stage"] == 9 => (3, vec![]),
        "skip" => (3, vec![]),
        "buy_pack" => (4, vec![param("id")?]),
        "sell_effect" => (5, vec![param("uid")?]),
        "shop_upgrade" => (6, vec![param("id")?]),
        "transport" => (7, vec![]),
        "refresh_shop" => (9, vec![]),
        "buy_effect" | "buy_rune" => (10, vec![param("id")?]),
        "advanced_upgrade" => (11, vec![param("id")?]),
        "trade" => (12, action.params["uids"].as_array().ok_or("invalid_action")?.iter()
            .map(|uid| uid.as_u64().and_then(|uid| u32::try_from(uid).ok()).ok_or("invalid_action"))
            .collect::<Result<Vec<_>, _>>()?),
        "bonfire" => (13, vec![param("id")?]),
        "forge_effect" => (14, vec![param("uid")?]),
        "gamble" => (15, vec![]),
        "select_candidate" => (16, vec![param("index")?]),
        _ => return Err("invalid_action"),
    };
    Ok((".lq.Lobby.amuletActivityOperate", json!({"activityId":activity,"type":kind,"args":args})))
}

#[cfg(test)]
mod tests;
