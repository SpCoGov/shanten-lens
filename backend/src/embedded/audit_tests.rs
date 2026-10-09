use super::switch::{plan_is_current, switch_command};
use super::*;

#[tokio::test]
async fn debug_cannot_replace_live_plan_and_execution_checks_identity_and_session() {
    const CHILD_ROOT: &str = "SHANTEN_SWITCH_TEST_ROOT";
    let root = match std::env::var_os(CHILD_ROOT) {
        Some(root) => PathBuf::from(root),
        None => {
            let root = std::env::temp_dir().join(format!(
                "shanten-switch-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            // Runtime logging is process-global; finish that process before removing its files.
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "embedded::audit_tests::debug_cannot_replace_live_plan_and_execution_checks_identity_and_session",
                    "--nocapture",
                ])
                .env(CHILD_ROOT, &root)
                .output()
                .unwrap();
            assert!(output.status.success(), "{output:?}");
            std::fs::remove_dir_all(root).unwrap();
            return;
        }
    };
    let runtime_root = root.join("runtime");
    let runtime = tokio::task::spawn_blocking(move || BackendRuntime::load(runtime_root))
        .await
        .unwrap()
        .unwrap();
    let state = &runtime.state;
    let mut events = runtime.subscribe();
    runtime
        .switch(
            serde_json::from_value(json!({
                "action":"start", "options":{"search_algorithm":"target_enumeration_search"}
            }))
            .unwrap(),
        )
        .await;
    let searching = events.try_recv().unwrap();
    assert_eq!(searching["data"][0]["data"]["status"], "searching");
    assert_eq!(searching["data"][0]["data"]["request_source"], "live");
    assert_eq!(
        searching["data"][0]["data"]["search_algorithm"],
        "target_enumeration_search"
    );
    let finished = events.try_recv().unwrap();
    assert_eq!(finished["data"][0]["data"]["status"], "impossible");
    assert_eq!(
        finished["data"][0]["data"]["search_algorithm"],
        "target_enumeration_search"
    );
    let live = state.switch_plan.read().await.clone().unwrap();
    switch_command(state, &json!({"action":"start_debug","snapshot":{}})).await;
    switch_command(
        state,
        &json!({"action":"validate_manual_debug","snapshot":{}}),
    )
    .await;
    assert_eq!(state.switch_plan.read().await.as_ref(), Some(&live));
    let tiles = ["4m", "4m", "4m", "4m", "7m", "7m", "7m", "7m", "1p", "1p", "1p", "1p", "6m", "5m", "1z"];
    let deck = tiles.iter().enumerate().map(|(i, face)| ((i + 1).to_string(), json!(face))).collect::<serde_json::Map<_, _>>();
    let preferred = switch_command(state, &json!({
        "action":"start_debug", "options":{"search_algorithm":"target_enumeration_search",
            "preferences":{"prefer_soul":true,"any_waits":true}},
        "snapshot":{"deck_map":deck,"hand_tiles":(1..=13).collect::<Vec<_>>(),"wall_tiles":[14,15],"tian_dora_tiles":["7m"]}
    })).await;
    let preferred_plan = &preferred[0]["data"][0]["data"];
    assert_eq!(preferred_plan["quad_faces"], json!(["7m", "1p"]));
    assert_eq!(preferred_plan["search_preferences"]["prefer_soul"], true);
    let custom = switch_command(state, &json!({
        "action":"start_debug", "options":{"search_algorithm":"custom_target",
            "target_groups":vec![json!({"rule":{}});14],"wall_limit":1},
        "snapshot":{"deck_map":deck,"hand_tiles":(1..=13).collect::<Vec<_>>(),"wall_tiles":[14]}
    })).await;
    assert_eq!(custom[0]["data"][0]["data"]["status"], "plan");
    assert_eq!(custom[0]["data"][0]["data"]["search_algorithm"], "custom_target");
    assert_eq!(custom[0]["data"][0]["data"]["request_source"], "debug");
    assert_eq!(custom[0]["data"][0]["data"]["target_physical_ids"].as_array().unwrap().len(), 14);
    let invalid_custom = switch_command(state, &json!({"action":"start_debug",
        "options":{"search_algorithm":"custom_target","target_groups":[]}})).await;
    assert_eq!(invalid_custom[0]["data"][0]["data"]["reason"], "custom-invalid-target");
    for preferences in [json!({"any_waits":"invalid"}), json!({"preferred_suit":"x"}), json!({"preferred_meld_type":"quad"})] {
        let invalid = switch_command(state, &json!({"action":"start_debug","options":{"preferences":preferences}})).await;
        assert_eq!(invalid[0]["data"][0]["data"]["reason"], "invalid-search-preferences");
    }
    assert_eq!(state.switch_plan.read().await.as_ref(), Some(&live));
    assert!(plan_is_current(state, &live).await);
    let result = switch_command(
        state,
        &json!({"action":"execute_plan","options":{"plan_id":"debug-1"}}),
    )
    .await;
    assert_eq!(result[0]["data"]["reason"], "stale-plan");
    let lock = state.switch_execution.lock().await;
    let result = switch_command(
        state,
        &json!({"action":"execute_plan","options":{"plan_id":live["plan_id"]}}),
    )
    .await;
    assert_eq!(result[0]["data"]["reason"], "execution-already-running");
    drop(lock);
    state.services.update_game_state(&pipeline::Packet {
        direction: pipeline::Direction::Inbound,
        packet_type: "Res".into(),
        method: ".lq.Lobby.fetchAmuletActivityData".into(),
        id: Some(1),
        data: json!({"game":{"state":{"current":5}}}),
    });
    assert!(!plan_is_current(state, &live).await);
    let result = switch_command(
        state,
        &json!({"action":"execute_plan","options":{"plan_id":live["plan_id"]}}),
    )
    .await;
    assert_eq!(result[0]["data"]["reason"], "stale-plan");
    let mut cancel = state.switch_generation.subscribe();
    let stopped = switch_command(state, &json!({"action":"stop"})).await;
    assert_eq!(stopped[0]["data"][0]["data"]["status"], "impossible");
    assert_eq!(
        stopped[0]["data"][0]["data"]["search_algorithm"],
        "target_enumeration_search"
    );
    assert_eq!(stopped[0]["data"][1]["data"]["request_source"], "debug");
    assert!(cancel.has_changed().unwrap());
    cancel.changed().await.unwrap();
    assert!(state.switch_plan.read().await.is_none());
    drop(runtime);
}
