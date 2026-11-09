use super::*;
use crate::{ipc::AutorunAction, pipeline::{Direction, Packet}, protocol::{LiqiCodec, MessageType}};
use hudsucker::tokio_tungstenite::tungstenite::Message;

#[tokio::test]
async fn opening_post_win_discards_continue_to_shop_instead_of_remaking() {
    const CHILD_ROOT: &str = "SHANTEN_POST_WIN_TEST_ROOT";
    let root = match std::env::var_os(CHILD_ROOT) {
        Some(root) => PathBuf::from(root),
        None => {
            let root = std::env::temp_dir().join(format!("lens-post-win-run-{}",std::process::id()));
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact","embedded::autorun_tests::opening_post_win_discards_continue_to_shop_instead_of_remaking","--nocapture"])
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
    let mut data: Value = serde_json::from_str(include_str!("../../tests/fixtures/opening-post-win-1.json")).unwrap();
    // Start at the preceding winning draw, then replay the captured non-winning continuation.
    let winning = data["game"]["round"]["hands"].as_array().unwrap().last().unwrap().clone();
    for tile in data["game"]["round"]["pool"].as_array_mut().unwrap() {
        if tile["id"] == winning { tile["tile"] = json!("6s"); }
    }
    data["game"]["record"]["levelHuCount"] = json!(0);
    data["game"]["round"]["nextOperation"] = json!([{"type":1},{"type":8}]);
    state.services.update_game_state_from_flow(&Packet {
        direction:Direction::Inbound,packet_type:"Res".into(),id:Some(1),method:".lq.Lobby.fetchAmuletActivityData".into(),data,
    },flow);
    assert!(runtime.autorun(AutorunAction::Start,true).await.ok);
    let mut codec = LiqiCodec::new();
    for step in 0..4 {
        let Message::Binary(frame) = tokio::time::timeout(Duration::from_secs(5),outgoing.recv()).await.unwrap().unwrap()
            else { panic!("expected discard or pack purchase") };
        let packet = codec.parse(&frame).unwrap();
        let game = state.services.game_state();
        let response = if step == 0 {
            assert_eq!(packet.method.as_ref(),".lq.Lobby.amuletActivityGameOperate");
            assert_eq!(packet.data["type"],8);
            let mut hand = game["hand_tiles"].as_array().unwrap().clone();
            let next = game["wall_tiles"].as_array().unwrap().iter().find(|tile|
                !hand.contains(tile) && !matches!(game["deck_map"][tile.to_string()].as_str(),Some("6s"|"3s"))).unwrap();
            *hand.last_mut().unwrap() = next.clone();
            json!({"events":[{"type":18,"state":{"current":6,"interrupt":0},"valueChanges":{
                "record":{"levelHuCount":{"dirty":true,"value":1}}}},
                {"type":10,"state":{"current":6,"interrupt":0},"valueChanges":{
                    "round":{"hands":{"dirty":true,"value":hand},"nextOperation":{"dirty":true,"value":[{"type":1}]}}}}]})
        } else if step < 3 {
            assert_eq!(packet.method.as_ref(),".lq.Lobby.amuletActivityGameOperate");
            assert_eq!(packet.data["type"],1,"an unreachable post-win target must never cause a remake");
            assert_eq!(packet.data["tileList"],json!([game["hand_tiles"].as_array().unwrap().last().unwrap()]));
            if step == 1 {
                let mut hand = game["hand_tiles"].as_array().unwrap().clone();
                *hand.last_mut().unwrap() = game["wall_tiles"][0].clone();
                json!({"events":[{"type":10,"state":{"current":6,"interrupt":0},"valueChanges":{
                    "round":{"hands":{"dirty":true,"value":hand},"desktopRemain":{"dirty":true,"value":8},
                        "nextOperation":{"dirty":true,"value":[{"type":1}]}}}}]})
            } else {
                json!({"events":[{"type":30,"state":{"current":9,"interrupt":0},"valueChanges":{
                    "game":{"coin":{"dirty":true,"value":"60"}},
                    "shop":{"goods":{"dirty":true,"value":[{"id":1,"goodsId":103,"price":15,"sold":false}]},
                        "upgradeGoods":{"dirty":true,"value":[]},"refreshPrice":{"dirty":true,"value":3}}}}]})
            }
        } else {
            assert_eq!(packet.method.as_ref(),".lq.Lobby.amuletActivityOperate");
            assert_eq!(packet.data["type"],4);
            assert_eq!(packet.data["args"],json!([1]),"affordable gold packs must still be bought");
            json!({})
        };
        let reply = codec.build(MessageType::Response,packet.id,&packet.method,&response).unwrap();
        processor.lock().unwrap().process(&reply,Direction::Inbound).unwrap();
    }
    assert_eq!(state.automation.autorun_status()["running"],true);
    assert!(state.automation.autorun_status()["remake_records"].as_array().unwrap().is_empty());
    state.automation.stop_autorun();
    state.proxy.unregister_direct();
}

#[tokio::test]
async fn opening_autoplay_sends_the_selected_initial_character_and_defaults_to_201() {
    const CHILD_ROOT: &str = "SHANTEN_CHARACTER_TEST_ROOT";
    let root = match std::env::var_os(CHILD_ROOT) {
        Some(root) => PathBuf::from(root),
        None => {
            let root = std::env::temp_dir().join(format!("lens-character-{}",std::process::id()));
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact","embedded::autorun_tests::opening_autoplay_sends_the_selected_initial_character_and_defaults_to_201","--nocapture"])
                .env(CHILD_ROOT,&root).output().unwrap();
            assert!(output.status.success(),"{output:?}");
            std::fs::remove_dir_all(root).unwrap();
            return;
        }
    };
    let runtime = tokio::task::spawn_blocking(move || BackendRuntime::load(root.join("configs"))).await.unwrap().unwrap();
    let state = &runtime.state;
    let (processor,mut outgoing,_) = state.proxy.register_direct("test",state.module_registry.clone()).await;
    let flow = processor.lock().unwrap().flow_id;
    let mut book: Value = serde_json::from_str(include_str!("../../tests/fixtures/playbook.json")).unwrap();
    book["selected_profile_id"] = book["profiles"][1]["id"].clone();
    let mut codec = LiqiCodec::new();
    for character in [None,Some(201),Some(202),Some(203),Some(204)] {
        if let Some(id) = character { book["profiles"][1]["character_id"] = json!(id); }
        else { book["profiles"][1].as_object_mut().unwrap().remove("character_id"); }
        state.services.patch_config(&json!({"autorun":{"playbook":book,"op_interval_ms":100}})).unwrap();
        state.services.update_game_state_from_flow(&Packet {
            direction:Direction::Inbound,packet_type:"Res".into(),id:Some(1),
            method:".lq.Lobby.fetchAmuletActivityData".into(),data:json!({"game":{
                "state":{"current":1,"interrupt":0},"map":{"level":13001,"node":1}}}),
        },flow);
        assert!(runtime.autorun(AutorunAction::Start,true).await.ok);
        let Message::Binary(frame) = tokio::time::timeout(Duration::from_secs(4),outgoing.recv()).await.unwrap().unwrap()
            else { panic!("expected character selection") };
        let packet = codec.parse(&frame).unwrap();
        assert_eq!(packet.method.as_ref(),".lq.Lobby.amuletActivityOperate");
        assert_eq!(packet.data["type"],1);
        assert_eq!(packet.data["args"],json!([character.unwrap_or(201)]));
        let response = codec.build(MessageType::Response,packet.id,&packet.method,&json!({})).unwrap();
        processor.lock().unwrap().process(&response,Direction::Inbound).unwrap();
        if character.is_none() {
            for selected in [false, true] {
                state.services.update_game_state_from_flow(&Packet {
                    direction:Direction::Inbound,packet_type:"Res".into(),id:Some(1),
                    method:".lq.Lobby.fetchAmuletActivityData".into(),data:json!({"game":{
                        "state":{"current":if selected {3} else {9},"interrupt":if selected {0} else {1000}},
                        "map":{"level":13001,"node":1,"mapNodes":[{"subType":0},{"subType":3002},{"subType":2}]},
                        "effect":{"effectList":if selected {json!([{"id":10,"uid":7,"volume":1}])} else {json!([])},
                            "packCandidates":[{"id":10,"uid":7,"volume":1}],"maxEffectVolume":8}}}),
                },flow);
                let Message::Binary(frame) = tokio::time::timeout(Duration::from_secs(4),outgoing.recv()).await.unwrap().unwrap()
                    else { panic!("expected opening map selection or remake") };
                let packet = codec.parse(&frame).unwrap();
                if selected {
                    assert_eq!(packet.method.as_ref(),".lq.Lobby.amuletActivityGiveup");
                    let game = state.services.game_state();
                    assert!(game["decision_report"]["actions"].as_array().unwrap().iter().any(|score|
                        score["total"] == 90000.0 && score["rules"].as_array().unwrap().iter().any(|rule| rule["rule_id"] == "opening_map_remake")));
                    state.automation.stop_autorun();
                } else {
                    assert_eq!(packet.method.as_ref(),".lq.Lobby.amuletActivityOperate");
                    assert_eq!(packet.data["type"],16,"map rejection must wait until the initial amulet is selected");
                }
                let response = codec.build(MessageType::Response,packet.id,&packet.method,&json!({})).unwrap();
                processor.lock().unwrap().process(&response,Direction::Inbound).unwrap();
            }
        }
        state.automation.stop_autorun();
    }
    state.proxy.unregister_direct();
}

#[tokio::test]
async fn mystery_shop_refreshes_once_sells_for_capacity_and_stops_immediately_after_target_purchase() {
    const CHILD_ROOT: &str = "SHANTEN_MYSTERY_SHOP_TEST_ROOT";
    let root = match std::env::var_os(CHILD_ROOT) {
        Some(root) => PathBuf::from(root),
        None => {
            let root = std::env::temp_dir().join(format!("lens-mystery-shop-{}",std::process::id()));
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact","embedded::autorun_tests::mystery_shop_refreshes_once_sells_for_capacity_and_stops_immediately_after_target_purchase","--nocapture"])
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
    book["profiles"][1]["end_count"] = json!(1);
    book["profiles"][1]["cutoff_level"] = json!(13001);
    book["profiles"][1]["cutoff_node"] = json!(5);
    book["profiles"][1]["targets"] = json!([{"kind":"amulet","id":169,"plus":true,"badge":600070,"value":1}]);
    let mut mail = state.services.table("autorun")["email_notify"].clone();
    mail["enabled"] = json!(false);
    state.services.patch_config(&json!({"autorun":{"playbook":book,"op_interval_ms":100,"email_notify":mail}})).unwrap();
    let (processor,mut outgoing,_) = state.proxy.register_direct("test",state.module_registry.clone()).await;
    let flow = processor.lock().unwrap().flow_id;
    let publish = |page: u64, effects: Value, goods: Value, coin: u64| state.services.update_game_state_from_flow(&Packet {
        direction:Direction::Inbound,packet_type:"Res".into(),id:Some(1),
        method:".lq.Lobby.fetchAmuletActivityData".into(),data:json!({"game":{
            "state":{"current":11,"interrupt":0},"map":{"level":13001,"node":3},"game":{"coin":coin},
            "effect":{"effectList":effects,"maxEffectVolume":8},"character":{"hp":50},
            "nodeEvent":{"advancedShopGoods":goods,"advancedShopUpgrade":[],
                "advancedShopRefreshCount":page,"advancedShopRefreshPrice":1}}}),
    },flow);
    let mut goods = json!([{"id":1,"type":1,"effect":{"id":1691,"badgeId":600070,"volume":1},"price":24,"sold":false}]);
    publish(0,json!([]),json!([]),25);
    assert!(runtime.autorun(AutorunAction::Start,true).await.ok);
    let mut codec = LiqiCodec::new();
    for (kind,args) in [(9,json!([])),(5,json!([7])),(10,json!([1]))] {
        let Message::Binary(frame) = tokio::time::timeout(Duration::from_secs(4),outgoing.recv()).await.unwrap().unwrap()
            else { panic!("expected shop operation") };
        let packet = codec.parse(&frame).unwrap();
        assert_eq!(packet.method.as_ref(),".lq.Lobby.amuletActivityOperate");
        assert_eq!(packet.data["type"],kind);
        assert_eq!(packet.data["args"],args);
        let response = codec.build(MessageType::Response,packet.id,&packet.method,&json!({})).unwrap();
        processor.lock().unwrap().process(&response,Direction::Inbound).unwrap();
        match kind {
            9 => publish(1,json!([{"id":1050,"uid":7,"volume":8,"badgeId":0}]),goods.clone(),24),
            5 => publish(1,json!([]),goods.clone(),27),
            _ => {
                goods[0]["sold"] = json!(true);
                publish(1,json!([{"id":1691,"uid":8,"volume":1,"badge":{"id":600070}}]),goods.clone(),3);
            }
        }
    }
    tokio::time::timeout(Duration::from_secs(2),async {
        while state.automation.autorun_status()["running"] == true { tokio::task::yield_now().await; }
    }).await.unwrap();
    assert_eq!(state.automation.autorun_status()["current_step"],"goal_met");
    assert!(!state.services.autorun_active());
    assert!(tokio::time::timeout(Duration::from_millis(300),outgoing.recv()).await.is_err(),
        "no refresh, sale or exit may be sent after reaching the target");
    state.proxy.unregister_direct();
}

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
    let faces = ["3p","3p","3p","4p","5p","6p","7p","8p","8p","8p","7p","4s","9m"];
    let mut pool = faces.iter().enumerate().map(|(i, face)|json!({"id":i+1,"tile":face})).collect::<Vec<_>>();
    pool.extend((101..=110).map(|id|json!({"id":id,"tile":"1z"})));
    pool.extend((201..=236).map(|id|json!({"id":id,"tile":if id == 234 {"6s"} else {"9s"}})));
    pool.extend([json!({"id":301,"tile":"9s"}),json!({"id":302,"tile":"5s"})]);
    state.services.update_game_state_from_flow(&Packet {
        direction:Direction::Inbound,packet_type:"Res".into(),id:Some(1),method:".lq.Lobby.fetchAmuletActivityData".into(),
        data:json!({"game":{"state":{"current":5,"interrupt":0},"map":{"level":13001,"node":2},
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

    publish(json!({"state":{"current":6,"interrupt":0},"map":{"level":13001,"node":2},
        "effect":{"effectList":[{"id":10,"uid":66,"volume":1,"badgeId":600070}]}}));
    state.automation.start_autorun().unwrap();
    let (generation, profile) = state.automation.round_context().unwrap();
    let game = state.services.game_state();
    let limit = json!({"status":"impossible","reason":"custom-search-limit"});
    let mut later = game.clone();
    later["node"] = json!(4);
    assert!(!super::round::remake_after_search_failure(state, generation, &profile, &limit, &later).await.unwrap());
    assert!(outgoing.try_recv().is_err(), "later-round search failures must not submit remake");
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
    assert_eq!(records[0]["node"], 2);
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
    for (id, reason) in [(3590,"invalid-search-tiles"),(3730,"invalid-search-tiles"),(620,"invalid-search-tiles"),(2240,"opening-tanyao-unreachable")] {
        let mut round = trace["round"].clone();
        if reason == "invalid-search-tiles" {
            // Model another effect leaving duplicate physical tiles after opening normalization.
            round["pool"][30] = round["pool"][29].clone();
        } else {
            for tile in round["pool"].as_array_mut().unwrap() { tile["tile"] = json!("1z"); }
        }
        state.services.update_game_state_from_flow(&Packet {
            direction:Direction::Inbound,packet_type:"Res".into(),id:Some(1),method:".lq.Lobby.fetchAmuletActivityData".into(),
            data:json!({"game":{"state":{"current":5,"interrupt":0},"map":{"level":13001,"node":2},
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
        let failed = state.services.game_state();
        assert_eq!(failed["opening_search_failure"]["reason"], reason);
        let score = failed["decision_report"]["actions"].as_array().unwrap().iter()
            .find(|score| score["rules"].as_array().unwrap().iter().any(|rule| rule["rule_id"] == "opening_search_remake")).unwrap();
        assert_eq!(score["total"], 90000.0);
        assert!(tokio::time::timeout(Duration::from_millis(250),outgoing.recv()).await.is_err(),"do not repeat a pending remake");
        let reply = codec.build(MessageType::Response,packet.id,&packet.method,&json!({})).unwrap();
        processor.lock().unwrap().process(&reply,Direction::Inbound).unwrap();
        let Message::Binary(frame) = tokio::time::timeout(Duration::from_secs(4),outgoing.recv()).await.unwrap().unwrap()
            else { panic!("expected the next opening run to start") };
        let packet = codec.parse(&frame).unwrap();
        assert_eq!(packet.method.as_ref(),".lq.Lobby.amuletActivityStartGame");
        assert!(state.services.game_state()["opening_search_failure"].is_null(), "a fresh game must clear the failure score");
        let reply = codec.build(MessageType::Response,packet.id,&packet.method,&json!({})).unwrap();
        processor.lock().unwrap().process(&reply,Direction::Inbound).unwrap();
        let status = state.automation.autorun_status();
        assert_eq!(status["running"],true);
        assert_eq!(status["last_error"],"");
        assert_eq!(status["email_status"],"");
        let record = status["remake_records"].as_array().unwrap().last().unwrap();
        assert_eq!(record["reason"],reason);
        assert_eq!(record["effect_list"][0]["id"],id);
        state.automation.stop_autorun();
        assert!(super::round::remake_after_search_failure(state,generation,&profile,
            &json!({"reason":"invalid-search-tiles"}),&game).await.unwrap());
        assert!(outgoing.try_recv().is_err(),"a stopped run must not send another remake");
        let mut recovered = false;
        while let Ok(event) = updates.try_recv() {
            if event["type"] == "autorun_status" && event["data"]["round_decision"]["reason"] ==
                if reason == "invalid-search-tiles" { "round.invalid_tiles_remake" } else { "round.opening_tanyao_remake" } { recovered = true; }
        }
        assert!(recovered);
    }
    state.proxy.unregister_direct();
}

#[tokio::test]
async fn vine_farming_shop_protocol_respects_dynamic_budget_and_echo_handoff() {
    const CHILD_ROOT: &str = "SHANTEN_VINE_TEST_ROOT";
    let root = match std::env::var_os(CHILD_ROOT) {
        Some(root) => PathBuf::from(root),
        None => {
            let root = std::env::temp_dir().join(format!("lens-vine-{}",std::process::id()));
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact","embedded::autorun_tests::vine_farming_shop_protocol_respects_dynamic_budget_and_echo_handoff","--nocapture"])
                .env(CHILD_ROOT,&root).output().unwrap();
            assert!(output.status.success(),"{output:?}");
            std::fs::remove_dir_all(root).unwrap();
            return;
        }
    };
    let runtime = tokio::task::spawn_blocking(move || BackendRuntime::load(root.join("configs"))).await.unwrap().unwrap();
    let state = &runtime.state;
    let book = json!({"version":1,"selected_profile_id":"vine","profiles":[{"id":"vine","name":"Vine","strategy":"vine_farming"}]});
    let mut mail = state.services.table("autorun")["email_notify"].clone();
    mail["enabled"] = json!(false);
    state.services.patch_config(&json!({"autorun":{"playbook":book,"op_interval_ms":100,"email_notify":mail}})).unwrap();
    let (processor,mut outgoing,_) = state.proxy.register_direct("test",state.module_registry.clone()).await;
    let flow = processor.lock().unwrap().flow_id;
    let effects: Vec<_> = [3771,2321,1691,1661,1501,621,3561,3760].into_iter()
        .map(|id|json!({"id":id,"uid":id,"volume":1,"badgeId":600070})).collect();
    let publish = |price: u64, coin: u64, pack: bool, candidates: Value| state.services.update_game_state_from_flow(&Packet {
        direction:Direction::Inbound,packet_type:"Res".into(),id:Some(1),
        method:".lq.Lobby.fetchAmuletActivityData".into(),data:json!({"game":{
            "state":{"current":9,"interrupt":if candidates.is_null() {0} else {1000}},"map":{"level":14016,"node":1},
            "game":{"coin":coin},"character":{"hp":50},"record":{"levelHuCount":0},
            "effect":{"effectList":effects,"maxEffectVolume":8,"shopBuffList":[],"packGoodsId":103,"packDisableSkip":false,
                "packCandidates":if candidates.is_null() {json!([])} else {candidates}},
            "shop":{"goods":if pack {json!([{"id":7,"goodsId":103,"price":15,"sold":false}])} else {json!([])},
                "upgradeGoods":[],"refreshPrice":price}}}),
    },flow);
    publish(140,169,false,Value::Null);
    let result = runtime.autorun(AutorunAction::Start,true).await;
    assert!(result.ok,"{result:?}");
    let mut codec = LiqiCodec::new();
    let mut events = state.events.subscribe();
    for (index,kind) in [3,9,4,3,4].into_iter().enumerate() {
        let Message::Binary(frame) = tokio::time::timeout(Duration::from_secs(5),outgoing.recv()).await.unwrap().unwrap()
            else { panic!("expected farming operation") };
        let packet = codec.parse(&frame).unwrap();
        assert_eq!(packet.method.as_ref(),".lq.Lobby.amuletActivityOperate","never give up a formed game");
        assert_eq!(packet.data["type"],kind,"operation {index}");
        if kind == 4 { assert_eq!(packet.data["args"],json!([7])); }
        let response = codec.build(MessageType::Response,packet.id,&packet.method,&json!({})).unwrap();
        processor.lock().unwrap().process(&response,Direction::Inbound).unwrap();
        match index {
            0 => publish(210,240,false,Value::Null),
            1 => publish(473,30,true,Value::Null),
            2 => publish(473,15,false,json!([{"id":3390,"badgeId":0,"volume":1}])),
            3 => publish(710,15,true,Value::Null),
            _ => publish(710,0,false,json!([{"id":3760,"badgeId":600180,"volume":1}])),
        }
    }
    tokio::time::timeout(Duration::from_secs(2),async {
        while state.automation.autorun_status()["running"] == true { tokio::task::yield_now().await; }
    }).await.unwrap();
    assert_eq!(state.automation.autorun_status()["current_step"],"vine_echo");
    assert!(!state.services.autorun_active());
    let mut notified = false;
    while let Ok(event) = events.try_recv() {
        if event["type"] == "ui_toast" && event["data"]["msg_key"] == "vine.echo_found" { notified = true; }
    }
    assert!(notified,"Echo handoff must notify the user");
    assert!(tokio::time::timeout(Duration::from_millis(300),outgoing.recv()).await.is_err(),"No buy/skip/remake after Echo appears");

    // Sufficient first wins continue; insufficient first wins stop before another operation.
    for (first_score, should_stop) in [("1E100",false),("1",true)] {
        let faces = ["1p","1p","1p","2p","2p","2p","3p","3p","3p","4p","4p","4p"];
        let mut pool: Vec<_> = faces.iter().enumerate().map(|(i,face)|json!({"id":i+1,"tile":face})).collect();
        pool.extend((101..=110).map(|id|json!({"id":id,"tile":"1z"})));
        pool.extend((201..=236).map(|id|json!({"id":id,"tile":"5p"})));
        pool.push(json!({"id":1000,"tile":"bd"}));
        state.services.update_game_state_from_flow(&Packet {direction:Direction::Inbound,packet_type:"Res".into(),id:Some(1),
            method:".lq.Lobby.fetchAmuletActivityData".into(),data:json!({"game":{
                "state":{"current":5,"interrupt":0},"map":{"level":14016,"node":1},"record":{"levelHuCount":0},
                "round":{"pool":pool,"hands":(1..=12).chain([1000]).collect::<Vec<_>>(),"dora":[101],"desktopRemain":36,
                    "totalChangeTileCount":2,"changeTileCount":0,"used":[],"ming":[],"nextOperation":[{"type":101,"value":0}]},
                "effect":{"effectList":effects,"packCandidates":[]}}})},flow);
        let game = state.services.game_state();
        assert!(runtime.autorun(AutorunAction::Start,true).await.ok);
        let remake = game["action_report"]["global_actions"].as_array().unwrap().iter().find(|a| a["kind"] == "remake").unwrap();
        let denied = state.action_executor.execute(&state.services,&state.proxy,crate::action_executor::ActionExecutionRequest {
            action_id:remake["id"].as_str().unwrap().into(), session_id:game["session_id"].as_u64().unwrap(),revision:game["revision"].as_u64().unwrap()
        }).await;
        assert_eq!(denied.reason_key.as_deref(),Some("vine.remake_forbidden"));
        for kind in [3,1,8] {
            let Message::Binary(frame) = tokio::time::timeout(Duration::from_secs(5),outgoing.recv()).await.unwrap_or_else(|_| panic!("waiting for {kind}: status={}, game={}",state.automation.autorun_status(),state.services.game_state())).unwrap()
                else { panic!("expected finish exchange, unavailable win continuation, then first win") };
            let packet = codec.parse(&frame).unwrap();
            assert_eq!(packet.data["type"],kind);
            assert_eq!(packet.method.as_ref(),if kind == 3 {".lq.Lobby.amuletActivityOperate"} else {".lq.Lobby.amuletActivityGameOperate"});
            let response = if kind == 3 {
                json!({"events":[{"type":10,"state":{"current":6,"interrupt":0},"result":{"dealResult":{"tile":201}},
                    "valueChanges":{"round":{"hands":{"dirty":true,"value":(1..=12).chain([1000,201]).collect::<Vec<_>>()},
                        "nextOperation":{"dirty":true,"value":[{"type":1}]},"desktopRemain":{"dirty":true,"value":35}}}}]})
            } else if kind == 1 {
                assert_eq!(packet.data["tileList"],json!([201]),"retain the Wanxiang target when no win is available");
                json!({"events":[{"type":10,"state":{"current":6,"interrupt":0},"result":{"dealResult":{"tile":202}},
                    "valueChanges":{"round":{"hands":{"dirty":true,"value":(1..=12).chain([1000,202]).collect::<Vec<_>>()},
                        "nextOperation":{"dirty":true,"value":[{"type":1},{"type":8}]},"desktopRemain":{"dirty":true,"value":34}}}}]})
            } else {
                json!({"events":[{"type":17,"valueChanges":{"record":{}}},
                    {"type":18,"result":{"huResult":{"huFinal":{"point":first_score}}},
                    "valueChanges":{"record":{"levelHuCount":{"dirty":true,"value":1}}}},
                    {"type":19,"valueChanges":{"record":{}}}]})
            };
            let reply = codec.build(MessageType::Response,packet.id,&packet.method,&response).unwrap();
            processor.lock().unwrap().process(&reply,Direction::Inbound).unwrap();
        }
        if should_stop {
            tokio::time::timeout(Duration::from_secs(2),async {
                while state.automation.autorun_status()["running"] == true { tokio::task::yield_now().await; }
            }).await.unwrap();
            assert_eq!(state.automation.autorun_status()["current_step"],"vine_goal");
            assert!(tokio::time::timeout(Duration::from_millis(300),outgoing.recv()).await.is_err(),"No request after an insufficient first win");
        } else {
            let Message::Binary(frame) = tokio::time::timeout(Duration::from_secs(5),outgoing.recv()).await.unwrap().unwrap()
                else { panic!("Sufficient first win must continue") };
            let packet = codec.parse(&frame).unwrap();
            assert_eq!(packet.method.as_ref(),".lq.Lobby.amuletActivityGameOperate");
            assert_eq!(packet.data["type"],8);
            assert_eq!(state.automation.autorun_status()["running"],true);
            state.automation.stop_autorun();
            let reply = codec.build(MessageType::Response,packet.id,&packet.method,&json!({})).unwrap();
            processor.lock().unwrap().process(&reply,Direction::Inbound).unwrap();
        }
    }
    state.proxy.unregister_direct();
}
