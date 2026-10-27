//! Serial autoplay loop: scored legal actions outside battle, target-hand planning inside it.
use super::{switch, State};
use crate::recommendations;
use serde_json::{json, Value};
use std::{cell::Cell, time::{Duration, Instant}};

pub(super) fn start(state: &State) {
    let Some((generation, profile)) = state.automation.round_context() else { return; };
    let worker = state.clone();
    state.automation.start_round_task(generation, run(worker, generation, profile));
}

pub(super) fn in_round(game: &Value) -> bool {
    matches!(game["stage"].as_u64(), Some(5..=8)) && game["ended"] != true
        && (game["source"] == "qyzz" || game.pointer("/action_report/interrupt")
            .or_else(|| game.pointer("/provider_state/state/interrupt")).and_then(Value::as_u64) == Some(0))
}

fn round_key(game: &Value) -> Value {
    json!([game["source"], game["session_id"], game["level"], game["node"]])
}

fn snapshot_key(game: &Value) -> Value {
    json!([round_key(game), game["revision"]])
}

fn plan_report(plan: &Value, status: &str, wins: u64) -> Value {
    json!({"status":status,"strategy":plan["round_strategy"],"stage_name":plan["stage_name"],
        "hand_name":plan["hand_name"],"target_faces":plan["target_physical_faces"],
        "pinzu_count":plan["pinzu_count"],"win_count":plan["win_count"],"score":plan["opening_score"],
        "draws_needed":plan["draws_needed"],"actual_wins":wins})
}

fn choose_plan(game: &Value, profile: &Value, progress: &mut dyn FnMut(Value), stopped: &dyn Fn() -> bool) -> Value {
    let stage = if profile["strategy"] == "opening" { None } else {
        match crate::decision::tactical_stage(game, profile) {
            Ok(stage) => stage,
            Err(_) => return json!({"status":"impossible","reason":"round.no_stage_data"}),
        }
    };
    let Some(stage) = stage else {
        let mut plan = recommendations::opening_round_plan(game, 36, progress, stopped);
        plan["round_strategy"] = json!("opening");
        return plan;
    };
    let wall = game["wall_tiles"].as_array().map_or(0, Vec::len) as u64;
    let mut best: Option<Value> = None;
    for hand in stage["hands"].as_array().into_iter().flatten() {
        if stopped() { return json!({"status":"impossible","reason":"stopped-by-user"}); }
        // Automatic tsumo requires a legal winning shape even if the editor also allows draft shapes.
        let mut plan = recommendations::custom_switch_plan(game, &hand["target_groups"], 36, true, progress, stopped);
        if matches!(plan["reason"].as_str(), Some("custom-search-limit" | "stopped-by-user")) {
            return plan;
        }
        if plan["status"] != "plan" { continue; }
        let wins = wall.saturating_sub(plan["draws_needed"].as_u64().unwrap_or(0)) + 1;
        plan["win_count"] = json!(wins);
        plan["round_strategy"] = json!("full");
        plan["stage_name"] = stage["name"].clone();
        plan["hand_name"] = hand["name"].clone();
        if best.as_ref().is_none_or(|current| wins > current["win_count"].as_u64().unwrap_or(0)) {
            best = Some(plan);
        }
        if stage["hand_selection"] == "priority" { break; }
    }
    best.unwrap_or_else(|| json!({"status":"impossible","reason":"round.no_reachable_hand",
        "round_strategy":"full","stage_name":stage["name"]}))
}

async fn wait_update(updates: &mut tokio::sync::broadcast::Receiver<Value>) {
    // Also wake after a dropped notification, without retrying an unchanged snapshot.
    let _ = tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            match updates.recv().await {
                Ok(event) if matches!(event["type"].as_str(), Some("update_gamestate" | "tsumo_loop_status")) => break,
                Err(_) => break,
                _ => {}
            }
        }
    }).await;
}

async fn matches_target(game: &Value, plan: &Value) -> bool {
    let game = game.clone();
    let target = plan["target_groups"].clone();
    tokio::task::spawn_blocking(move || recommendations::matches_custom_target(&game, &target)).await.unwrap_or(false)
}

fn action_error(result: crate::ipc::CommandResult) -> String {
    if result.reason_key.as_deref() == Some("action_provider.execution.rejected") {
        format!("protocol-error: {}", result.response.as_ref().unwrap_or(&Value::Null)["error"])
    } else { result.reason.or(result.reason_key).unwrap_or_else(|| "action_provider.execution.send_failed".into()) }
}

pub(super) async fn remake_after_search_limit(state: &State, generation: u64, profile: &Value, plan: &Value, game: &Value) -> Result<bool, String> {
    if profile["strategy"] != "opening" || plan["reason"] != "custom-search-limit" { return Ok(false); }
    if !state.automation.round_is_current(generation) || snapshot_key(&state.services.game_state()) != snapshot_key(game) {
        return Ok(true);
    }
    let action = game["action_report"]["global_actions"].as_array()
        .and_then(|actions| actions.iter().find(|action| action["kind"] == "remake" && action["availability"] == "available"))
        .ok_or("action_provider.execution.unavailable")?;
    let mut updates = state.events.subscribe();
    if !state.automation.begin_action(generation, "remake") { return Ok(true); }
    state.automation.set_round_decision(generation, json!({"status":"waiting","strategy":"opening","reason":"round.search_limit_remake"}));
    let result = state.action_executor.execute(&state.services, &state.proxy,
        crate::action_executor::ActionExecutionRequest {
            action_id: action["id"].as_str().ok_or("action_provider.execution.unavailable")?.into(),
            session_id: game["session_id"].as_u64().unwrap_or(0),
            revision: game["revision"].as_u64().unwrap_or(0),
        }).await;
    if !state.automation.round_is_current(generation) { return Ok(true); }
    if !result.ok && result.reason_key.as_deref() != Some("action_provider.execution.awaiting_state") {
        if result.reason_key.as_deref() == Some("action_provider.execution.stale")
            && snapshot_key(&state.services.game_state()) != snapshot_key(game) { return Ok(true); }
        return Err(action_error(result));
    }
    if result.ok { state.automation.record_remake(generation, game, "custom-search-limit"); }
    let sent_at = Instant::now();
    while state.automation.round_is_current(generation) && snapshot_key(&state.services.game_state()) == snapshot_key(game) {
        if sent_at.elapsed() >= Duration::from_secs(15) { return Err("autoplay.state_update_timeout".into()); }
        wait_update(&mut updates).await;
    }
    Ok(true)
}

async fn run(state: State, generation: u64, profile: Value) {
    let mut updates = state.events.subscribe();
    let mut key = Value::Null;
    let mut attempted = Value::Null;
    let mut active: Option<Value> = None;
    let mut last_action = Value::Null;
    let mut action_sent_at = Instant::now();
    let mut wins = 0;
    let mut switch_generation = *state.switch_generation.borrow();
    loop {
        if !state.automation.round_is_current(generation) { return; }
        let game = state.services.game_state();
        state.automation.observe(generation, &game);
        if !state.automation.round_is_current(generation) { return; }
        if round_key(&game) != key || !matches!(game["stage"].as_u64(), Some(5..=8)) || game["ended"] == true {
            key = round_key(&game);
            active = None;
            attempted = Value::Null;
            wins = 0;
        }
        let action = crate::decision::first_action(&game);
        let priority_remake = action.as_ref().is_some_and(|action| action.kind == "remake"
            && game["decision_report"]["actions"].as_array().is_some_and(|scores| scores.iter()
                .any(|score| score["action_id"] == action.id && score["total"].as_f64().is_some_and(|total| total > 0.0))));
        if !in_round(&game) || priority_remake {
            state.automation.set_round_decision(generation, json!({"status":"waiting","reason":"round.no_round"}));
            if last_action == snapshot_key(&game) {
                if action_sent_at.elapsed() >= Duration::from_secs(15) {
                    state.automation.fail(generation, "autoplay.state_update_timeout");
                    return;
                }
            } else if game["action_report"]["status"] == "ready" || priority_remake {
                if let Some(action) = action {
                    if !state.automation.begin_action(generation, &action.kind) { return; }
                    last_action = snapshot_key(&game);
                    let result = state.action_executor.execute(&state.services, &state.proxy,
                        crate::action_executor::ActionExecutionRequest {
                            action_id: action.id.clone(),
                            session_id: game["session_id"].as_u64().unwrap_or(0),
                            revision: game["revision"].as_u64().unwrap_or(0),
                        }).await;
                    action_sent_at = Instant::now();
                    if !state.automation.round_is_current(generation) { return; }
                    if result.ok && action.kind == "remake" {
                        let cutoff = game["decision_report"]["actions"].as_array()
                            .and_then(|scores| scores.iter().find(|score| score["action_id"] == action.id))
                            .and_then(|score| score["rules"].as_array()).is_some_and(|rules| rules.iter().any(|rule|
                                rule["rule_id"] == "remake_at_cutoff" && rule["score"].as_f64().is_some_and(|score| score > 0.0)));
                        state.automation.record_remake(generation, &game, if cutoff { "remake_at_cutoff" } else { "decision" });
                    }
                    if !result.ok && result.reason_key.as_deref() != Some("action_provider.execution.awaiting_state") {
                        if result.reason_key.as_deref() == Some("action_provider.execution.stale")
                            && snapshot_key(&state.services.game_state()) != last_action { continue; }
                        state.automation.fail(generation, &action_error(result));
                        return;
                    }
                    let delay = state.services.table("autorun")["op_interval_ms"].as_u64().unwrap_or(1000).clamp(100, 30_000);
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                    continue;
                }
            }
            wait_update(&mut updates).await;
            continue;
        }
        if *state.switch_generation.borrow() != switch_generation {
            state.automation.set_round_decision(generation, json!({"status":"blocked","reason":"round.manual_execution"}));
            return;
        }
        if state.automation.tsumo_status().running {
            state.automation.set_round_decision(generation, json!({"status":"blocked","reason":"round.manual_tsumo"}));
            wait_update(&mut updates).await;
            continue;
        }
        if attempted == snapshot_key(&game) {
            wait_update(&mut updates).await;
            continue;
        }
        attempted = snapshot_key(&game);
        if ![1, 4, 8, 100, 101].iter().any(|kind| switch::operation_available(&game, *kind)) {
            state.automation.set_round_decision(generation, json!({"status":"waiting","reason":"round.no_operations"}));
            continue;
        }
        let Ok(_execution) = state.switch_execution.try_lock() else {
            state.automation.set_round_decision(generation, json!({"status":"blocked","reason":"round.manual_execution"}));
            return;
        };
        let ready = match &active { Some(plan) => matches_target(&game, plan).await, None => false };
        if !ready {
            let mut report = json!({"status":"searching","strategy":if profile["strategy"] == "opening" {"opening"} else {"full"},"actual_wins":wins});
            state.automation.set_round_decision(generation, report.clone());
            let worker = state.clone();
            let snapshot = game.clone();
            let config = profile.clone();
            let mut plan = match tokio::task::spawn_blocking(move || {
                let last_check = Cell::new(Instant::now());
                let changed = Cell::new(false);
                let stopped = || {
                    if last_check.get().elapsed() >= Duration::from_millis(50) {
                        changed.set(snapshot_key(&worker.services.game_state()) != snapshot_key(&snapshot));
                        last_check.set(Instant::now());
                    }
                    changed.get() || !worker.automation.round_is_current(generation)
                        || *worker.switch_generation.borrow() != switch_generation
                };
                let mut progress = |value| {
                    if stopped() { return; }
                    report["progress"] = value;
                    worker.automation.set_round_decision(generation, report.clone());
                };
                choose_plan(&snapshot, &config, &mut progress, &stopped)
            }).await {
                Ok(plan) => plan,
                Err(error) => json!({"status":"impossible","reason":format!("search-task-failed: {error}")}),
            };
            if !state.automation.round_is_current(generation) { return; }
            if snapshot_key(&state.services.game_state()) != snapshot_key(&game) { continue; }
            if *state.switch_generation.borrow() != switch_generation { continue; }
            if plan["status"] != "plan" {
                match remake_after_search_limit(&state, generation, &profile, &plan, &game).await {
                    Ok(true) => continue,
                    Ok(false) => {},
                    Err(reason) => { state.automation.fail(generation, &reason); return; },
                }
                let mut report = plan_report(&plan, "blocked", wins);
                report["reason"] = plan["reason"].clone();
                state.automation.set_round_decision(generation, report);
                continue;
            }
            // Install through the same plan identity used by manual execution and cancellation.
            let mut current = state.switch_plan.write().await;
            if *state.switch_generation.borrow() != switch_generation
                || snapshot_key(&state.services.game_state()) != snapshot_key(&game) { continue; }
            state.switch_generation.send_modify(|value| *value += 1);
            switch_generation = *state.switch_generation.borrow();
            plan["generation"] = json!(switch_generation);
            plan["plan_id"] = json!(format!("round-{generation}-{switch_generation}"));
            plan["session_id"] = game["session_id"].clone();
            plan["state_revision"] = game["revision"].clone();
            plan["round_level"] = game["level"].clone();
            plan["round_node"] = game["node"].clone();
            plan["round_generation"] = json!(generation);
            *current = Some(plan.clone());
            drop(current);
            state.automation.set_round_decision(generation, plan_report(&plan, "executing", wins));
            let mut cancellation = state.switch_generation.subscribe();
            let result = tokio::select! {
                biased;
                _ = cancellation.changed() => Err("stopped-by-user".to_owned()),
                result = switch::execute_custom_switch_plan(&state, &plan) => result,
            };
            if let Err(reason) = result {
                let current = state.services.game_state();
                if !in_round(&current) || round_key(&current) != key {
                    active = None;
                    continue;
                }
                // shortcut: replan acknowledged hand-changing effects; add simulation for exact optimization across them.
                if reason == "blackhole.execute_reason_hand_modified" && snapshot_key(&current) != snapshot_key(&game) {
                    active = None;
                    attempted = Value::Null;
                    state.automation.set_round_decision(generation, json!({"status":"waiting","reason":"round.state_changed"}));
                    continue;
                }
                let mut report = plan_report(&plan, "blocked", wins);
                report["reason"] = json!(reason);
                state.automation.set_round_decision(generation, report);
                return;
            }
            active = Some(plan);
        }
        let plan = active.as_ref().unwrap();
        let game = state.services.game_state();
        if !state.automation.round_is_current(generation) { return; }
        if round_key(&game) != key || !in_round(&game) { continue; }
        if !matches_target(&game, plan).await {
            let mut report = plan_report(plan, "blocked", wins);
            report["reason"] = json!("custom-target-mismatch");
            state.automation.set_round_decision(generation, report);
            return;
        }
        if !switch::operation_available(&game, 8) {
            let mut report = plan_report(plan, "waiting", wins);
            report["reason"] = json!("round.no_operations");
            state.automation.set_round_decision(generation, report);
            attempted = snapshot_key(&game);
            continue;
        }
        state.automation.set_round_decision(generation, plan_report(plan, "winning", wins));
        if let Err(reason) = switch::send_game_operation(&state, plan, &game, 8, &[]).await {
            let mut report = plan_report(plan, "blocked", wins);
            report["reason"] = json!(reason);
            state.automation.set_round_decision(generation, report);
            return; // A timeout may have applied the win; never retry a mutating request.
        }
        wins += 1;
        let mut report = plan_report(plan, "winning", wins);
        report["reason"] = json!("round.waiting_update");
        state.automation.set_round_decision(generation, report);
        attempted = snapshot_key(&game);
        let delay = state.services.table("autorun")["op_interval_ms"].as_u64().unwrap_or(1000).clamp(100, 30_000);
        tokio::time::sleep(Duration::from_millis(delay)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game() -> Value {
        let faces = ["1p","1p","2p","2p","3p","3p","4p","4p","5p","5p","6p","6p","7p","7p","9s"];
        json!({"stage":6,"source":"packet","session_id":1,"revision":1,"level":13001,"node":3,
            "provider_state":{"map":{"level":13001,"node":3},"state":{"interrupt":0}},
            "hand_tiles":(1..=14).collect::<Vec<_>>(),"wall_tiles":[15],"replacement_tiles":[],"ming":[],
            "next_operation":[{"type":1},{"type":8}],"total_change_tile_count":0,"change_tile_count":0,
            "deck_map":faces.iter().enumerate().map(|(i,f)|((i+1).to_string(),json!(f))).collect::<serde_json::Map<_,_>>()})
    }

    fn hand(name: &str, faces: &[&str]) -> Value {
        json!({"name":name,"target_groups":faces.iter().map(|face|json!({"kind":"single","rule":{"faces":[face]}})).collect::<Vec<_>>()})
    }

    #[test]
    fn configured_hands_use_priority_or_most_wins() {
        let mut book: Value = serde_json::from_str(include_str!("../../tests/fixtures/playbook.json")).unwrap();
        let profile = &mut book["profiles"][0];
        let early = ["1p","1p","2p","2p","3p","3p","4p","4p","5p","5p","6p","6p","7p","7p"];
        let mut later = early;
        later[12] = "9s";
        later[13] = "9s";
        let mut game = game();
        // A fixed joker permits both alternative waits while preserving valid winning shapes.
        game["deck_map"]["13"] = json!("bd");
        profile["stages"][0]["hands"] = json!([hand("later", &later), hand("early", &early)]);
        profile["stages"][0]["hand_selection"] = json!("most_wins");
        let best = choose_plan(&game, profile, &mut |_| {}, &|| false);
        assert_eq!(best["status"], "plan", "{best}");
        assert_eq!(best["hand_name"], "early");
        assert_eq!(best["win_count"], 2);
        profile["stages"][0]["hand_selection"] = json!("priority");
        let first = choose_plan(&game, profile, &mut |_| {}, &|| false);
        assert_eq!(first["hand_name"], "later", "{first}");
        assert_eq!(first["win_count"], 1);
    }

    #[test]
    fn requires_battle_without_interrupt_and_resolves_full_stage_from_map() {
        let mut game = game();
        assert!(in_round(&game));
        game["provider_state"]["state"]["interrupt"] = json!(1000);
        assert!(!in_round(&game));
        game["stage"] = json!(9);
        assert!(!in_round(&game));
        let book: Value = serde_json::from_str(include_str!("../../tests/fixtures/playbook.json")).unwrap();
        game["provider_state"]["map"] = Value::Null;
        game["level"] = Value::Null;
        assert_eq!(choose_plan(&game, &book["profiles"][0], &mut |_| {}, &|| false)["reason"], "round.no_stage_data");
    }
}
