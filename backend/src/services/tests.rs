use super::game_state::{apply_fetch_state, rebuild_pool_sections};
use super::*;
use crate::pipeline::{Direction, Packet};

fn temp_root(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "shanten-lens-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

#[test]
fn fetch_without_game_clears_previous_game_and_offers_start_game() {
    let root = temp_root("fetch-no-game");
    let (events, mut receiver) = broadcast::channel(64);
    let services = Services::load(root.clone(), events).unwrap();
    let mut packet = Packet {
        direction: Direction::Inbound, packet_type: "Res".into(),
        method: ".lq.Lobby.fetchAmuletActivityData".into(), id: Some(1),
        data: json!({"data":{"activityId":261081,"bookEffectId":0,"gameRecords":[],"version":0}}),
    };
    for with_previous_game in [false, true] {
        if with_previous_game {
            packet.data = json!({"data":{"game":{"state":{"current":9,"interrupt":0},
                "round":{"hands":[1,2]},"effect":{"effectList":[{"id":3020,"uid":1}]},
                "shop":{"goods":[{"id":1,"goodsId":101,"price":5,"sold":false}]},"game":{"coin":"20"}}}});
            services.update_game_state_from_flow(&packet, 7);
            assert_eq!(services.game_state()["stage"], 9);
            let live = services.game_state();
            packet.data = json!({"error":{"code":1004},"data":{"activityId":261081}});
            services.update_game_state_from_flow(&packet, 7);
            assert_eq!(services.game_state(), live, "Failed fetches must preserve the current game");
            packet.data = json!({"data":{"activityId":261081,"statistic":{"roundCount":37}}});
        }
        let before = services.game_state();
        services.update_game_state_from_flow(&packet, 7);
        let current = services.game_state();
        assert_eq!(current["source"], "packet");
        assert_eq!(current["flow_id"], 7);
        assert_eq!(current["stage"], 100);
        assert_eq!(current["ended"], true);
        assert!(current["session_id"].as_u64().unwrap() > before["session_id"].as_u64().unwrap());
        assert!(current["revision"].as_u64().unwrap() > before["revision"].as_u64().unwrap());
        for key in ["hand_tiles", "effect_list", "goods", "candidate_effect_list"] {
            assert_eq!(current[key], json!([]), "{key}");
        }
        assert_eq!(current["coin"], "0");
        assert_eq!(current["action_report"]["stage"], 100);
        assert_eq!(current["action_report"]["actions"].as_array().unwrap().len(), 1);
        assert_eq!(current["action_report"]["actions"][0]["kind"], "start_game");
        assert_eq!(current["action_report"]["actions"][0]["availability"], "available");
        assert_eq!(current["action_report"]["global_actions"], json!([]));
        let published = std::iter::from_fn(|| receiver.try_recv().ok())
            .filter(|event| event["type"] == "update_gamestate").last().unwrap();
        assert_eq!(published["data"], current);
    }
    packet.data = json!({"game":{"state":{"current":1,"interrupt":0}}});
    services.update_game_state_from_flow(&packet, 7);
    assert_eq!(services.game_state()["stage"], 1);
    assert_eq!(services.game_state()["ended"], false);
    assert_eq!(services.game_state()["action_report"]["actions"][0]["kind"], "select_character");
    drop(services);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn action_provider_replays_documented_events_and_survives_ipc() {
    let root = temp_root("action-provider");
    let (events, _) = broadcast::channel(64);
    let services = Services::load(root.clone(), events).unwrap();
    let fixtures: Value = serde_json::from_str(include_str!("../../tests/fixtures/provider-events.json")).unwrap();
    for (name, stage, count) in [("start",1,4),("character",2,4),("initial",3,1),("next",5,0),("trade",12,3)] {
        services.update_game_state_from_flow(&Packet {direction: Direction::Inbound, packet_type:"Res".into(),
            method:".lq.Lobby.amuletActivityOperate".into(), id:Some(1), data:fixtures[name].clone()},7);
        let state = services.game_state();
        assert_eq!(state["stage"],stage,"{name}");
        assert_eq!(state["action_report"]["actions"].as_array().unwrap().len(),count,"{name}");
        let typed: crate::ipc::GameStateData = serde_json::from_value(state.clone()).unwrap();
        assert_eq!(serde_json::to_value(typed).unwrap()["action_report"],state["action_report"]);
        assert!(state.get("provider_state").is_none());
        if name == "character" {
            assert_eq!(state["action_report"]["actions"][0]["availability"],"available");
            assert_eq!(state["action_report"]["actions"][3]["reasons"][0]["code"],"initial_must_choose");
        }
    }
    let before = services.game_state()["revision"].as_u64().unwrap();
    services.update_game_state_from_flow(&Packet {direction: Direction::Inbound,packet_type:"Res".into(),
        method:".lq.Lobby.amuletActivityOperate".into(),id:Some(2),data:json!({"events":[{"state":{"current":12,"interrupt":0},
        "valueChanges":{"nodeEvent":{"tradeItems":{"dirty":true,"value":[{"id":2,"store":[999]}]}}}}]})},7);
    let after = services.game_state();
    assert!(after["revision"].as_u64().unwrap() > before);
    assert_eq!(after["action_report"]["actions"][0]["reasons"][0]["code"],"insufficient_coin");
    services.disconnect_game_flow(7);
    assert_eq!(services.game_state()["action_report"]["status"],"waiting_state");
    drop(services);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn playbook_roundtrips_with_runtime_settings() {
    let root = temp_root("playbook");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("autorun.json"), r#"{"op_interval_ms":700}"#).unwrap();
    let (events, _) = broadcast::channel(16);
    let services = Services::load(root.clone(), events.clone()).unwrap();
    assert_eq!(services.table("autorun")["op_interval_ms"], 700);
    assert_eq!(services.table("autorun")["playbook"]["profiles"], json!([]));
    let book: Value = serde_json::from_str(include_str!("../../tests/fixtures/playbook.json")).unwrap();
    services.patch_config(&json!({"autorun":{"playbook":book}})).unwrap();
    let saved = fs::read(root.join("autorun.json")).unwrap();
    let mut invalid = book.clone();
    invalid["profiles"][0]["initial_amulet_id"] = json!(230);
    assert!(services.patch_config(&json!({"autorun":{"playbook":invalid}})).is_err());
    assert_eq!(fs::read(root.join("autorun.json")).unwrap(), saved);
    let mut invalid_opening = book.clone();
    invalid_opening["profiles"][1]["targets"][0]["value"] = json!(0);
    assert!(services.patch_config(&json!({"autorun":{"playbook":invalid_opening}})).is_err());
    assert_eq!(fs::read(root.join("autorun.json")).unwrap(), saved);
    drop(services);
    let reloaded = Services::load(root.clone(), events).unwrap();
    assert_eq!(reloaded.table("autorun")["playbook"], book);
    assert_eq!(reloaded.table("autorun")["op_interval_ms"], 700);
    for field in ["end_count", "targets", "cutoff_level", "need_pionner_badge_count"] {
        assert!(reloaded.table("autorun").get(field).is_none());
        assert!(reloaded.patch_config(&json!({"autorun": {field: 1}})).is_err());
    }
    let wire = serde_json::from_value::<crate::ipc::AutoRunnerConfig>(reloaded.table("autorun")).unwrap();
    assert_eq!(serde_json::to_value(wire).unwrap()["playbook"], book);
    drop(reloaded);
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn playbook_invalid_levels_can_be_repaired_without_blocking_startup_or_overwriting_disk() {
    let root = temp_root("playbook-repair");
    fs::create_dir_all(&root).unwrap();
    let mut valid: Value = serde_json::from_str(include_str!("../../tests/fixtures/playbook.json")).unwrap();
    valid["selected_profile_id"] = json!("opening-profile");
    let mut old = valid.clone();
    old["profiles"][1]["cutoff_level"] = json!(203);
    old["profiles"][1].as_object_mut().unwrap().remove("cutoff_node");
    for stage in old["profiles"][0]["stages"].as_array_mut().unwrap() {
        stage["start_level"] = json!(201);
        stage["deadline_level"] = json!(201);
        stage.as_object_mut().unwrap().remove("start_node");
        stage.as_object_mut().unwrap().remove("deadline_node");
    }
    let path = root.join("autorun.json");
    let original = serde_json::to_vec(&json!({"playbook": old, "op_interval_ms": 700})).unwrap();
    fs::write(&path, &original).unwrap();
    let (events, _) = broadcast::channel(16);
    let services = Arc::new(Services::load(root.clone(), events.clone()).expect("outdated level IDs must not prevent startup"));
    assert_eq!(services.table("autorun")["playbook"], old);
    assert_eq!(fs::read(&path).unwrap(), original);
    services.reload_files();
    assert_eq!(services.table("autorun")["playbook"], old);
    assert_eq!(fs::read(&path).unwrap(), original);
    assert!(services.patch_config(&json!({"autorun": {"playbook": old}})).is_err());
    assert_eq!(fs::read(&path).unwrap(), original);
    let proxy = crate::proxy::ProxyControl::new(services.clone(), Arc::new(tokio::sync::RwLock::new(crate::pipeline::PipelineConfig::default())));
    let automation = crate::automation::Automation::new(services.clone(), proxy);
    assert_eq!(automation.start_autorun(), Err("autoplay.invalid_config"));
    assert_eq!(automation.autorun_status()["running"], false);
    let mut unset = valid.clone();
    unset["profiles"][1]["cutoff_level"] = json!(0);
    unset["profiles"][1]["cutoff_node"] = json!(0);
    let unset_bytes = serde_json::to_vec(&json!({"playbook": unset, "op_interval_ms": 700})).unwrap();
    fs::write(&path, &unset_bytes).unwrap();
    services.reload_files();
    assert_eq!(services.table("autorun")["playbook"], unset);
    assert!(services.patch_config(&json!({"autorun": {"playbook": unset}})).is_err());
    assert_eq!(automation.start_autorun(), Err("autoplay.invalid_config"));
    assert_eq!(fs::read(&path).unwrap(), unset_bytes);
    services.patch_config(&json!({"autorun": {"playbook": valid}})).unwrap();
    assert_eq!(services.table("autorun")["playbook"], valid);
    automation.start_autorun().unwrap();
    automation.stop_autorun();
    drop(automation);
    drop(services);
    let reloaded = Services::load(root.clone(), events).unwrap();
    assert_eq!(reloaded.table("autorun")["playbook"], valid);
    assert_eq!(reloaded.table("autorun")["op_interval_ms"], 700);
    drop(reloaded);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn refresh_price_is_a_string_in_snapshots_and_events_for_both_sources() {
    let root = temp_root("refresh-price");
    let (events, mut received) = broadcast::channel(64);
    let services = Services::load(root.join("configs"), events).unwrap();
    let initial = serde_json::from_value::<crate::ipc::GameStateData>(services.game_state()).unwrap();
    assert_eq!(serde_json::to_value(initial).unwrap()["refresh_price"], "0");
    for price in [json!(2), json!("2"), json!("18446744073709551616")] {
        let expected = price.as_str().map(str::to_owned).unwrap_or_else(|| price.to_string());
        let packet = Packet {
            direction: crate::pipeline::Direction::Inbound,
            packet_type: "Res".into(),
            method: ".lq.Lobby.fetchAmuletActivityData".into(),
            id: Some(1),
            data: json!({"game":{"shop":{"refreshPrice":price}}}),
        };
        services.update_game_state_from_flow(&packet, 1);
        let state = serde_json::from_value::<crate::ipc::GameStateData>(services.game_state()).unwrap();
        assert_eq!(serde_json::to_value(state).unwrap()["refresh_price"], expected);
        services.disconnect_game_flow(1);
        services.publish_qyzz_state(&json!({
            "source":"qyzz", "qyzz_run_id":"test", "qyzz_version":1, "qyzz_deal_id":1,
            "deck_map":{}, "hand_tiles":[], "wall_tiles":[], "locked_tiles":[],
            "replacement_tiles":[], "dora_tiles":[], "effect_list":[], "refresh_price":price,
        })).unwrap();
        let state = serde_json::from_value::<crate::ipc::GameStateData>(services.game_state()).unwrap();
        assert_eq!(serde_json::to_value(state).unwrap()["refresh_price"], expected);
        services.disconnect_qyzz();
    }
    while let Ok(event) = received.try_recv() {
        if event["type"] == "update_gamestate" {
            assert!(event["data"]["refresh_price"].is_string());
        }
    }
    drop(services);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn failed_giveup_preserves_state_and_other_flows_cannot_apply_deltas() {
    let root = temp_root("failed-giveup");
    let (events, _) = broadcast::channel(16);
    let services = Services::load(root.join("configs"), events).unwrap();
    let mut packet = Packet {
        direction: crate::pipeline::Direction::Inbound,
        packet_type: "Res".into(),
        method: ".lq.Lobby.fetchAmuletActivityData".into(),
        id: Some(1),
        data: json!({"game":{"state":{"current":5}}}),
    };
    services.update_game_state_from_flow(&packet, 10);
    let previous = services.game_state();
    packet.method = ".lq.Lobby.amuletActivityGiveup".into();
    packet.data = json!({"error":{"code":1}});
    services.update_game_state_from_flow(&packet, 10);
    assert_eq!(services.game_state(), previous);
    packet.method = ".lq.Lobby.amuletActivityOperate".into();
    packet.data = json!({"events":[{"state":{"current":3}}]});
    services.update_game_state_from_flow(&packet, 20);
    assert_eq!(services.game_state(), previous);
    services.disconnect_game_flow(10);
    assert_ne!(services.game_state()["session_id"], previous["session_id"]);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn fetch_and_delta_use_the_same_pool_partition() {
    for boss in [false, true] {
        let pool = (1..=80)
            .map(|id| json!({"id":id,"tile":if id==80 {"bd"} else {"1m"}}))
            .collect::<Vec<_>>();
        let round = json!({"pool":pool,"hands":[1,2,3,4,5,6,7,8,9,10,11,12,80],"dora":[13],"lockedTile":[25],"usedDesktop":[26],"desktopRemain":16});
        let mut fetched = empty_game_state().as_object().unwrap().clone();
        apply_fetch_state(
            &mut fetched,
            &json!({"game":{"round":round,"map":{"node":1,"mapNodes":[{"args":if boss {vec![926]} else {vec![]}}]}}}),
        );
        assert_eq!(fetched["dora_tiles"], json!((13..23).collect::<Vec<_>>()));
        assert_eq!(fetched["wall_tiles"].as_array().unwrap().len(), 16);
        assert!(!fetched["replacement_tiles"]
            .as_array()
            .unwrap()
            .contains(&json!(80)));
        assert_eq!(fetched["replacement_tiles"][0], if boss { 41 } else { 59 });
        let mut delta = fetched.clone();
        rebuild_pool_sections(
            &mut delta,
            &json!({"pool":{"dirty":true,"value":pool},"dora":{"dirty":true,"value":[13]}}),
            false,
        );
        for field in [
            "deck_map",
            "opening_hand_tiles",
            "dora_tiles",
            "wall_tiles",
            "replacement_tiles",
        ] {
            assert_eq!(fetched[field], delta[field], "{field}");
        }
    }
}

#[test]
fn config_backfills_missing_fields_and_can_overwrite_existing_files() {
    let base = temp_root("config");
    let config_root = base.join("configs");
    fs::create_dir_all(&config_root).unwrap();
    fs::write(config_root.join("game.json"), r#"{"public_all":true}"#).unwrap();
    fs::write(
        config_root.join("backend.json"),
        r#"{"obsolete":true,"mitm_port":10999,"enable_upstream_proxy":false,"upstream_proxy":""}"#,
    )
    .unwrap();
    let (events, _) = broadcast::channel(4);
    let services = Services::load(config_root.clone(), events).unwrap();

    assert_eq!(services.table("game")["public_all"], true);
    assert_eq!(services.table("game")["modify_announcement"], true);
    let saved =
        serde_json::from_slice::<Value>(&fs::read(config_root.join("game.json")).unwrap()).unwrap();
    assert!(saved.get("unlock_illustrated_book").is_some());
    let backend_saved =
        serde_json::from_slice::<Value>(&fs::read(config_root.join("backend.json")).unwrap())
            .unwrap();
    assert!(backend_saved.get("obsolete").is_none());

    assert_eq!(services.startup_announcement()["title"], "欢迎使用向听镜");
    services.set_locale("ja-JP");
    assert_eq!(
        services.startup_announcement()["title"],
        "向聴レンズへようこそ"
    );

    services
        .patch_config(&json!({"game":{"public_all":false}}))
        .unwrap();
    assert_eq!(services.table("game")["public_all"], false);
    fs::write(config_root.join("game.json"), "[]").unwrap();
    let previous = services.table("game");
    services.reload_files();
    assert_eq!(services.table("game"), previous);
    assert_eq!(
        fs::read_to_string(config_root.join("game.json")).unwrap(),
        "[]"
    );
    assert!(services
        .patch_config(&json!({"../outside": {"value": true}}))
        .is_err());
    assert!(!base.join("outside.json").exists());
    assert!(services
        .patch_config(&json!({"game": {"public_all": "wrong type"}}))
        .is_err());
    assert!(services
        .patch_config(&json!({"backend": {"mitm_port": 65536}}))
        .is_err());
    assert_eq!(services.table("game"), previous);

    fs::remove_dir_all(base).unwrap();
}
