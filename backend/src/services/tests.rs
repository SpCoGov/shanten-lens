use super::game_state::{apply_fetch_state, rebuild_pool_sections};
use super::*;
use crate::pipeline::Packet;

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
