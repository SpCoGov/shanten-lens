use super::*;
use crate::{ipc::AutorunAction, pipeline::{Direction, Packet}, protocol::{LiqiCodec, MessageType}};
use hudsucker::tokio_tungstenite::tungstenite::Message;

#[tokio::test]
async fn hand_changing_effect_replans_and_wins_without_another_exchange() {
    const CHILD_ROOT: &str = "SHANTEN_HAND_CHANGE_TEST_ROOT";
    let root = match std::env::var_os(CHILD_ROOT) {
        Some(root) => PathBuf::from(root),
        None => {
            let root = std::env::temp_dir().join(format!("lens-hand-change-{}", std::process::id()));
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "embedded::autorun_tests::hand_changing_effect_replans_and_wins_without_another_exchange", "--nocapture"])
                .env(CHILD_ROOT, &root).output().unwrap();
            assert!(output.status.success(), "{output:?}");
            std::fs::remove_dir_all(root).unwrap();
            return;
        }
    };
    let runtime = tokio::task::spawn_blocking(move || BackendRuntime::load(root.join("configs"))).await.unwrap().unwrap();
    let state = &runtime.state;
    let mut book: Value = serde_json::from_str(include_str!("../../tests/fixtures/playbook.json")).unwrap();
    book["selected_profile_id"] = json!("opening-profile");
    state.services.patch_config(&json!({"autorun":{"playbook":book,"op_interval_ms":100}})).unwrap();
    let (processor, mut outgoing, _) = state.proxy.register_direct("test", state.module_registry.clone()).await;
    let flow = processor.lock().unwrap().flow_id;
    let faces = ["1p","1p","1p","2p","2p","2p","3p","3p","3p","4p","4p","4p","5m"];
    let mut pool = faces.iter().enumerate().map(|(i, face)|json!({"id":i+1,"tile":face})).collect::<Vec<_>>();
    pool.extend((101..=110).map(|id|json!({"id":id,"tile":"1z"})));
    pool.extend((201..=236).map(|id|json!({"id":id,"tile":if id == 234 {"5p"} else {"9s"}})));
    pool.extend([json!({"id":301,"tile":"9s"}),json!({"id":302,"tile":"5p"})]);
    state.services.update_game_state_from_flow(&Packet {
        direction:Direction::Inbound,packet_type:"Res".into(),id:Some(1),method:".lq.Lobby.fetchAmuletActivityData".into(),
        data:json!({"game":{"state":{"current":5,"interrupt":0},"map":{"level":13001,"node":3},
            "effect":{"effectList":[{"id":2180,"uid":1,"volume":1,"store":["1"]}]},
            "round":{"pool":pool,"hands":(1..=13).collect::<Vec<_>>(),"dora":[101],"desktopRemain":3,
                "totalChangeTileCount":2,"changeTileCount":0,"used":[],"ming":[],"nextOperation":[{"type":101,"value":0}]}}}),
    }, flow);
    let mut updates = state.events.subscribe();
    assert!(runtime.autorun(AutorunAction::Start, true).await.ok);
    let mut codec = LiqiCodec::new();
    for step in 0..3 {
        let Message::Binary(frame) = tokio::time::timeout(Duration::from_secs(15), outgoing.recv()).await.unwrap().unwrap()
            else { panic!("expected automatic game operation") };
        let packet = codec.parse(&frame).unwrap();
        let response = match step {
            0 => {
                assert_eq!(packet.method.as_ref(), ".lq.Lobby.amuletActivityGameOperate");
                assert_eq!(packet.data["type"], 101);
                assert_eq!(packet.data["tileList"], json!((1..=12).collect::<Vec<_>>()));
                let hand = (1..=12).chain([302]).collect::<Vec<_>>();
                json!({"events":[{"type":7,"state":{"current":5,"interrupt":0},
                    "effectedHooks":[{"id":2180,"type":1,"uid":1,"result":{"modifyChangeHandsList":hand}}],
                    "valueChanges":{"round":{"hands":{"dirty":true,"value":hand},"used":{"dirty":true,"value":[13]},
                        "changeTileCount":{"dirty":true,"value":1}}}}]})
            },
            1 => {
                assert_eq!(packet.method.as_ref(), ".lq.Lobby.amuletActivityOperate", "replanning must finish instead of repeating the old second exchange: {}", packet.data);
                assert_eq!(packet.data["type"], 3);
                json!({"events":[{"type":10,"state":{"current":6,"interrupt":0},"result":{"dealResult":{"tile":234}},
                    "valueChanges":{"round":{"hands":{"dirty":true,"value":(1..=12).chain([302,234]).collect::<Vec<_>>()},
                        "nextOperation":{"dirty":true,"value":[{"type":1},{"type":8}]},"desktopRemain":{"dirty":true,"value":2}}}}]})
            },
            _ => {
                assert_eq!(packet.method.as_ref(), ".lq.Lobby.amuletActivityGameOperate");
                assert_eq!(packet.data["type"], 8);
                json!({})
            },
        };
        let reply = codec.build(MessageType::Response, packet.id, &packet.method, &response).unwrap();
        processor.lock().unwrap().process(&reply, Direction::Inbound).unwrap();
    }
    tokio::time::timeout(Duration::from_secs(2), async {
        while state.automation.autorun_status()["round_decision"]["actual_wins"] != 1 { tokio::task::yield_now().await; }
    }).await.unwrap();
    assert_eq!(state.automation.autorun_status()["running"], true);
    assert_eq!(state.automation.autorun_status()["last_error"], "");
    assert_eq!(state.automation.autorun_status()["email_status"], "");
    state.automation.stop_autorun();
    let mut replanned = false;
    while let Ok(event) = updates.try_recv() {
        if event["type"] == "autorun_status" && event["data"]["round_decision"]["reason"] == "round.state_changed" { replanned = true; }
    }
    assert!(replanned);
    assert!(outgoing.try_recv().is_err());
    state.proxy.unregister_direct();
}

#[tokio::test]
async fn autoplay_executes_top_action_waits_for_state_and_stops_on_goal_or_error() {
    const CHILD_ROOT: &str = "SHANTEN_AUTOPLAY_TEST_ROOT";
    let root = match std::env::var_os(CHILD_ROOT) {
        Some(root) => PathBuf::from(root),
        None => {
            let root = std::env::temp_dir().join(format!("lens-autoplay-{}", std::process::id()));
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "embedded::autorun_tests::autoplay_executes_top_action_waits_for_state_and_stops_on_goal_or_error", "--nocapture"])
                .env(CHILD_ROOT, &root).output().unwrap();
            assert!(output.status.success(), "{output:?}");
            std::fs::remove_dir_all(root).unwrap();
            return;
        }
    };
    let runtime = tokio::task::spawn_blocking(move || BackendRuntime::load(root.join("configs"))).await.unwrap().unwrap();
    let state = &runtime.state;
    let mut book: Value = serde_json::from_str(include_str!("../../tests/fixtures/playbook.json")).unwrap();
    book["selected_profile_id"] = json!("opening-profile");
    book["profiles"][1]["end_count"] = json!(1);
    book["profiles"][1]["targets"] = json!([{"kind":"amulet","id":224,"plus":false,"value":1}]);
    state.services.patch_config(&json!({"autorun":{"playbook":book,"op_interval_ms":100}})).unwrap();
    let (processor, mut outgoing, _) = state.proxy.register_direct("test", state.module_registry.clone()).await;
    let flow = processor.lock().unwrap().flow_id;
    let publish = |game: Value| state.services.update_game_state_from_flow(&Packet {
        direction:Direction::Inbound,packet_type:"Res".into(),id:Some(1),
        method:".lq.Lobby.fetchAmuletActivityData".into(),data:json!({"game":game}),
    }, flow);
    publish(json!({"state":{"current":100,"interrupt":0},"map":{"level":13001,"node":1}}));
    assert!(runtime.autorun(AutorunAction::Start, true).await.ok);
    assert!(state.services.autorun_active());
    let mut codec = LiqiCodec::new();
    for (method, kind, args) in [
        (".lq.Lobby.amuletActivityStartGame", None, json!(null)),
        (".lq.Lobby.amuletActivityOperate", Some(1), json!([201])),
        (".lq.Lobby.amuletActivityOperate", Some(16), json!([1])),
    ] {
        let Message::Binary(frame) = tokio::time::timeout(Duration::from_secs(4), outgoing.recv()).await.unwrap().unwrap()
            else { panic!("expected binary request") };
        let packet = codec.parse(&frame).unwrap();
        assert_eq!(packet.method.as_ref(), method);
        if let Some(kind) = kind {
            assert_eq!(packet.data["type"], kind);
            assert_eq!(packet.data["args"], args);
        }
        let reply = codec.build(MessageType::Response, packet.id, &packet.method, &json!({})).unwrap();
        processor.lock().unwrap().process(&reply, Direction::Inbound).unwrap();
        assert!(tokio::time::timeout(Duration::from_millis(250), outgoing.recv()).await.is_err(),
            "a response without fresh state must not cause another operation");
        match kind {
            None => publish(json!({"state":{"current":1,"interrupt":0},"map":{"level":13001,"node":1}})),
            Some(1) => publish(json!({"state":{"current":9,"interrupt":1000},"map":{"level":13001,"node":1},
                "effect":{"effectList":[],"maxEffectVolume":8,"packCandidates":[
                    {"id":10,"uid":1,"volume":1},{"id":2240,"uid":2,"volume":1}]}})),
            _ => publish(json!({"state":{"current":3,"interrupt":0},"map":{"level":13001,"node":1},
                "effect":{"effectList":[{"id":2240,"uid":2,"volume":1}]}})),
        };
    }
    tokio::time::timeout(Duration::from_secs(2), async {
        while state.automation.autorun_status()["running"] == true { tokio::task::yield_now().await; }
    }).await.unwrap();
    assert_eq!(state.automation.autorun_status()["current_step"], "goal_met");
    assert!(!state.services.autorun_active());
    assert!(outgoing.try_recv().is_err());

    publish(json!({"state":{"current":5,"interrupt":0},"map":{"level":13001,"node":1},
        "effect":{"effectList":[]},"round":{"hands":(1..=13).collect::<Vec<_>>(),
            "totalChangeTileCount":10,"changeTileCount":0,"nextOperation":[{"type":101,"value":3}]}}));
    let game = state.services.game_state();
    let plan = json!({"plan_id":"packet-exchange-limit","generation":*state.switch_generation.borrow(),"session_id":game["session_id"]});
    *state.switch_plan.write().await = Some(plan.clone());
    state.automation.start_autorun().unwrap();
    assert_eq!(super::switch::send_game_operation(state, &plan, &game, 101, &[1,2]).await.unwrap_err(), "blackhole.execute_reason_exchange_limit");
    assert!(outgoing.try_recv().is_err(), "a swap of 11 tiles must be rejected locally when the limit is 3");
    let finish_trace: Value = serde_json::from_str(include_str!("../../tests/fixtures/finish-exchange.json")).unwrap();
    for (kind, tiles) in [(101,(1..=10).collect::<Vec<_>>()),(100,vec![])] {
        let operation = super::switch::send_game_operation(state, &plan, &game, kind, &tiles);
        let reply = async {
            let Message::Binary(frame) = tokio::time::timeout(Duration::from_secs(4), outgoing.recv()).await.unwrap().unwrap()
                else { panic!("expected game operation") };
            let packet = codec.parse(&frame).unwrap();
            let response = if kind == 100 {
                assert_eq!(packet.method.as_ref(), finish_trace["request"]["method"].as_str().unwrap());
                assert_eq!(packet.data, finish_trace["request"]["data"]);
                finish_trace["response"].clone()
            } else {
                assert_eq!(packet.method.as_ref(), ".lq.Lobby.amuletActivityGameOperate");
                assert_eq!(packet.data["type"], kind);
                assert_eq!(packet.data["tileList"], json!(tiles));
                json!({})
            };
            let reply = codec.build(MessageType::Response, packet.id, &packet.method, &response).unwrap();
            processor.lock().unwrap().process(&reply, Direction::Inbound).unwrap();
        };
        let (result, ()) = tokio::join!(operation, reply);
        result.unwrap();
    }
    assert_eq!(state.services.game_state()["stage"], 6, "the recorded finish response must enter battle");
    assert_eq!(state.services.game_state()["hand_tiles"].as_array().unwrap().len(), 14);
    assert_eq!(state.services.game_state()["next_operation"][0]["type"], 1);
    state.automation.stop_autorun();
    *state.switch_plan.write().await = None;

    publish(json!({"state":{"current":6,"interrupt":0},"map":{"level":13001,"node":1},
        "effect":{"effectList":[{"id":10,"uid":66,"volume":1,"badgeId":600070}]}}));
    state.automation.start_autorun().unwrap();
    let (generation, profile) = state.automation.round_context().unwrap();
    let game = state.services.game_state();
    let limit = json!({"status":"impossible","reason":"custom-search-limit"});
    for (profile, plan) in [
        (json!({"strategy":"full"}), limit.clone()),
        (json!({"strategy":"full"}), json!({"reason":"invalid-search-tiles"})),
        (profile.clone(), json!({"reason":"custom-not-reachable"})),
    ] {
        assert!(!super::round::remake_after_search_failure(state, generation, &profile, &plan, &game).await.unwrap());
        assert!(outgoing.try_recv().is_err());
    }
    let recovery = super::round::remake_after_search_failure(state, generation, &profile, &limit, &game);
    let reply = async {
        let Message::Binary(frame) = tokio::time::timeout(Duration::from_secs(4), outgoing.recv()).await.unwrap().unwrap()
            else { panic!("expected remake request") };
        let packet = codec.parse(&frame).unwrap();
        assert_eq!(packet.method.as_ref(), ".lq.Lobby.amuletActivityGiveup");
        assert_eq!(state.automation.autorun_status()["round_decision"]["reason"], "round.search_limit_remake");
        assert!(tokio::time::timeout(Duration::from_millis(250), outgoing.recv()).await.is_err(), "remake must not be submitted twice");
        let reply = codec.build(MessageType::Response, packet.id, &packet.method, &json!({})).unwrap();
        processor.lock().unwrap().process(&reply, Direction::Inbound).unwrap();
    };
    let (recovered, ()) = tokio::join!(recovery, reply);
    assert!(recovered.unwrap());
    assert_eq!(state.services.game_state()["ended"], true);
    assert!(state.automation.round_is_current(generation));
    assert!(state.services.autorun_active());
    assert_eq!(state.automation.autorun_status()["last_error"], "");
    assert_eq!(state.automation.autorun_status()["email_status"], "");
    let records = state.automation.autorun_status()["remake_records"].clone();
    assert_eq!(records.as_array().unwrap().len(), 1);
    assert_eq!(records[0]["reason"], "custom-search-limit");
    assert_eq!(records[0]["stage"], 6);
    assert_eq!(records[0]["node"], 1);
    assert_eq!(records[0]["amulet_count"], 1);
    assert_eq!(records[0]["effect_list"][0]["id"], 10, "capture inventory before giveup clears the live state");
    super::round::start(state);
    let Message::Binary(frame) = tokio::time::timeout(Duration::from_secs(4), outgoing.recv()).await.unwrap().unwrap()
        else { panic!("expected next game to start automatically") };
    let packet = codec.parse(&frame).unwrap();
    assert_eq!(packet.method.as_ref(), ".lq.Lobby.amuletActivityStartGame");
    let reply = codec.build(MessageType::Response, packet.id, &packet.method, &json!({})).unwrap();
    processor.lock().unwrap().process(&reply, Direction::Inbound).unwrap();
    assert_eq!(state.automation.autorun_status()["runs"], 2);
    state.automation.stop_autorun();
    assert!(super::round::remake_after_search_failure(state, generation, &profile, &limit, &game).await.unwrap());
    assert!(outgoing.try_recv().is_err(), "a stopped run must not remake");

    publish(json!({"state":{"current":5,"interrupt":0},"map":{"level":13002,"node":4},
        "effect":{"effectList":[{"id":10,"uid":77,"volume":1}]},
        "round":{"hands":(1..=13).collect::<Vec<_>>(),"totalChangeTileCount":10,"nextOperation":[{"type":101,"value":3}]}}));
    assert!(runtime.autorun(AutorunAction::Start, true).await.ok);
    let Message::Binary(frame) = tokio::time::timeout(Duration::from_secs(4), outgoing.recv()).await.unwrap().unwrap()
        else { panic!("expected cutoff remake") };
    let packet = codec.parse(&frame).unwrap();
    assert_eq!(packet.method.as_ref(), ".lq.Lobby.amuletActivityGiveup");
    let reply = codec.build(MessageType::Response, packet.id, &packet.method, &json!({})).unwrap();
    processor.lock().unwrap().process(&reply, Direction::Inbound).unwrap();
    let Message::Binary(frame) = tokio::time::timeout(Duration::from_secs(4), outgoing.recv()).await.unwrap().unwrap()
        else { panic!("expected a new game after cutoff remake") };
    let packet = codec.parse(&frame).unwrap();
    assert_eq!(packet.method.as_ref(), ".lq.Lobby.amuletActivityStartGame");
    let reply = codec.build(MessageType::Response, packet.id, &packet.method, &json!({})).unwrap();
    processor.lock().unwrap().process(&reply, Direction::Inbound).unwrap();
    state.automation.stop_autorun();
    let status = state.automation.autorun_status();
    assert_eq!(status["remake_records"].as_array().unwrap().len(), 2);
    assert_eq!(status["remake_records"][1]["reason"], "remake_at_cutoff");
    assert_eq!(status["remake_records"][1]["stage"], 5, "cutoff remake must also run during the exchange phase");
    assert_eq!(status["remake_records"][1]["effect_list"][0]["uid"], 77);

    publish(json!({"state":{"current":1,"interrupt":0},"map":{"level":13001,"node":1},"effect":{"effectList":[]}}));
    assert!(runtime.autorun(AutorunAction::Start, true).await.ok);
    let Message::Binary(frame) = tokio::time::timeout(Duration::from_secs(4), outgoing.recv()).await.unwrap().unwrap()
        else { panic!("expected binary request") };
    let packet = codec.parse(&frame).unwrap();
    let reply = codec.build(MessageType::Response, packet.id, &packet.method, &json!({"error":{"code":1004}})).unwrap();
    processor.lock().unwrap().process(&reply, Direction::Inbound).unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while state.automation.autorun_status()["running"] == true { tokio::task::yield_now().await; }
    }).await.unwrap();
    let status = state.automation.autorun_status();
    assert_eq!(status["current_step"], "failed");
    assert!(status["last_error"].as_str().unwrap().contains("1004"));
    assert!(!state.services.autorun_active());
    assert!(outgoing.try_recv().is_err(), "errors must stop execution without retries");
    publish(json!({"state":{"current":3,"interrupt":0},"map":{"level":13002,"node":4},"effect":{"effectList":[]}}));
    assert!(runtime.autorun(AutorunAction::Start, true).await.ok);
    let Message::Binary(frame) = tokio::time::timeout(Duration::from_secs(4), outgoing.recv()).await.unwrap().unwrap()
        else { panic!("expected rejected remake") };
    let packet = codec.parse(&frame).unwrap();
    assert_eq!(packet.method.as_ref(), ".lq.Lobby.amuletActivityGiveup");
    let reply = codec.build(MessageType::Response, packet.id, &packet.method, &json!({"error":{"code":1004}})).unwrap();
    processor.lock().unwrap().process(&reply, Direction::Inbound).unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while state.automation.autorun_status()["running"] == true { tokio::task::yield_now().await; }
    }).await.unwrap();
    assert_eq!(state.automation.autorun_status()["remake_records"].as_array().unwrap().len(), 2, "failed remakes must not create history entries");
    state.proxy.unregister_direct();
}

#[tokio::test]
async fn invalid_search_tiles_remakes_and_continues_for_any_amulet() {
    const CHILD_ROOT: &str = "SHANTEN_INVALID_SEARCH_TEST_ROOT";
    let root = match std::env::var_os(CHILD_ROOT) {
        Some(root) => PathBuf::from(root),
        None => {
            let root = std::env::temp_dir().join(format!("lens-invalid-search-{}",std::process::id()));
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact","embedded::autorun_tests::invalid_search_tiles_remakes_and_continues_for_any_amulet","--nocapture"])
                .env(CHILD_ROOT,&root).output().unwrap();
            assert!(output.status.success(),"{output:?}");
            std::fs::remove_dir_all(root).unwrap();
            return;
        }
    };
    let runtime = tokio::task::spawn_blocking(move || BackendRuntime::load(root.join("configs"))).await.unwrap().unwrap();
    let state = &runtime.state;
    let mut book: Value = serde_json::from_str(include_str!("../../tests/fixtures/playbook.json")).unwrap();
    book["selected_profile_id"] = json!("opening-profile");
    state.services.patch_config(&json!({"autorun":{"playbook":book,"op_interval_ms":100}})).unwrap();
    let (processor,mut outgoing,_) = state.proxy.register_direct("test",state.module_registry.clone()).await;
    let flow = processor.lock().unwrap().flow_id;
    let trace: Value = serde_json::from_str(include_str!("../../tests/fixtures/mini-dragon-opening.json")).unwrap();
    let mut codec = LiqiCodec::new();
    for id in [3590,3730,620] {
        let mut round = trace["round"].clone();
        // Model another effect leaving duplicate physical tiles after opening normalization.
        round["pool"][30] = round["pool"][29].clone();
        state.services.update_game_state_from_flow(&Packet {
            direction:Direction::Inbound,packet_type:"Res".into(),id:Some(1),method:".lq.Lobby.fetchAmuletActivityData".into(),
            data:json!({"game":{"state":{"current":5,"interrupt":0},"map":{"level":13001,"node":4},
                "effect":{"effectList":[{"id":id,"uid":17,"volume":1}]},"round":round}}),
        },flow);
        let mut updates = state.events.subscribe();
        assert!(runtime.autorun(AutorunAction::Start,true).await.ok);
        let (generation,profile) = state.automation.round_context().unwrap();
        let game = state.services.game_state();
        let Message::Binary(frame) = tokio::time::timeout(Duration::from_secs(15),outgoing.recv()).await.unwrap().unwrap()
            else { panic!("expected invalid search to remake") };
        let packet = codec.parse(&frame).unwrap();
        assert_eq!(packet.method.as_ref(),".lq.Lobby.amuletActivityGiveup");
        assert!(tokio::time::timeout(Duration::from_millis(250),outgoing.recv()).await.is_err(),"do not repeat a pending remake");
        let reply = codec.build(MessageType::Response,packet.id,&packet.method,&json!({})).unwrap();
        processor.lock().unwrap().process(&reply,Direction::Inbound).unwrap();
        let Message::Binary(frame) = tokio::time::timeout(Duration::from_secs(4),outgoing.recv()).await.unwrap().unwrap()
            else { panic!("expected the next opening run to start") };
        let packet = codec.parse(&frame).unwrap();
        assert_eq!(packet.method.as_ref(),".lq.Lobby.amuletActivityStartGame");
        let reply = codec.build(MessageType::Response,packet.id,&packet.method,&json!({})).unwrap();
        processor.lock().unwrap().process(&reply,Direction::Inbound).unwrap();
        let status = state.automation.autorun_status();
        assert_eq!(status["running"],true);
        assert_eq!(status["last_error"],"");
        assert_eq!(status["email_status"],"");
        let record = status["remake_records"].as_array().unwrap().last().unwrap();
        assert_eq!(record["reason"],"invalid-search-tiles");
        assert_eq!(record["effect_list"][0]["id"],id);
        state.automation.stop_autorun();
        assert!(super::round::remake_after_search_failure(state,generation,&profile,
            &json!({"reason":"invalid-search-tiles"}),&game).await.unwrap());
        assert!(outgoing.try_recv().is_err(),"a stopped run must not send another remake");
        let mut recovered = false;
        while let Ok(event) = updates.try_recv() {
            if event["type"] == "autorun_status" && event["data"]["round_decision"]["reason"] == "round.invalid_tiles_remake" { recovered = true; }
        }
        assert!(recovered);
    }
    state.proxy.unregister_direct();
}
