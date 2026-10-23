use super::*;

fn action(kind: &str, params: Value) -> ActionCandidate {
    ActionCandidate { id: kind.into(), kind: kind.into(), params, cost: None, cost_resource: "coin".into(), availability: Availability::Available, reasons: vec![] }
}

#[test]
fn maps_every_provider_operation_to_protocol_arguments() {
    let mut state = json!({"stage":9,"level":13001});
    for (kind, params, operation, args) in [
        ("select_character", json!({"id":203}), 1, vec![203]),
        ("next_node", json!({}), 3, vec![]),
        ("skip", json!({}), 3, vec![]),
        ("buy_pack", json!({"id":2,"goods_id":100}), 4, vec![2]),
        ("sell_effect", json!({"uid":77,"effect":{"id":3020}}), 5, vec![77]),
        ("shop_upgrade", json!({"id":2,"upgrade_id":8002}), 6, vec![2]),
        ("transport", json!({}), 7, vec![]),
        ("refresh_shop", json!({}), 9, vec![]),
        ("buy_effect", json!({"id":4,"effect":{"id":3020}}), 10, vec![4]),
        ("buy_rune", json!({"id":5,"rune_id":701}), 10, vec![5]),
        ("advanced_upgrade", json!({"id":3,"upgrade_id":9001}), 11, vec![3]),
        ("trade", json!({"uids":[77,88],"hp_cost":"12"}), 12, vec![77,88]),
        ("trade", json!({"uids":[]}), 12, vec![]),
        ("bonfire", json!({"id":8}), 13, vec![8]),
        ("forge_effect", json!({"uid":77,"effect":{"id":3020}}), 14, vec![77]),
        ("gamble", json!({}), 15, vec![]),
        ("select_candidate", json!({"index":0,"effect":{"id":3020}}), 16, vec![0]),
    ] {
        let (method, payload) = command(&action(kind, params), &state).unwrap();
        assert_eq!(method, ".lq.Lobby.amuletActivityOperate", "{kind}");
        assert_eq!(payload, json!({"activityId":261081,"type":operation,"args":args}), "{kind}");
    }
    state["stage"] = json!(3);
    assert_eq!(command(&action("next_node",json!({})), &state).unwrap().1["type"], 2);
    state["stage"] = json!(11);
    assert!(command(&action("next_node",json!({})), &state).is_err());
    for (kind, method) in [("start_game",".lq.Lobby.amuletActivityStartGame"),("remake",".lq.Lobby.amuletActivityGiveup")] {
        assert_eq!(command(&action(kind,json!({})), &state).unwrap(), (method,json!({"activityId":261081})));
    }
    state["level"] = json!(11001);
    assert_eq!(command(&action("start_game",json!({})), &state).unwrap().1["activityId"], 260511);
    for params in [json!({}), json!({"uid":-1}), json!({"uid":4294967296u64})] {
        assert!(command(&action("sell_effect",params), &state).is_err());
    }
    assert!(command(&action("trade",json!({"uids":[null]})), &state).is_err());
    assert!(command(&action("not_supported",json!({})), &state).is_err());
}

#[test]
fn accepts_only_current_available_candidates_including_global_actions() {
    let request = ActionExecutionRequest { action_id: "skip".into(), session_id: 8, revision: 12 };
    let mut state = json!({"source":"packet","session_id":8,"revision":12,"action_report":{
        "actions":[action("skip",json!({}))], "global_actions":[action("remake",json!({}))]
    }});
    assert_eq!(candidate(&state,&request).unwrap().kind,"skip");
    assert_eq!(candidate(&state,&ActionExecutionRequest {action_id:"remake".into(), ..request.clone()}).unwrap().kind,"remake");
    for changed in [ActionExecutionRequest { revision: 11, ..request.clone() },
        ActionExecutionRequest { session_id: 7, ..request.clone() },
        ActionExecutionRequest { action_id: "forged-id".into(), ..request.clone() }] {
        assert_eq!(candidate(&state,&changed).unwrap_err(),"stale");
    }
    for availability in ["unavailable","unknown"] {
        state["action_report"]["actions"][0]["availability"] = json!(availability);
        assert_eq!(candidate(&state,&request).unwrap_err(),"unavailable");
    }
    state["source"] = json!("qyzz");
    assert_eq!(candidate(&state,&request).unwrap_err(),"unsupported");
}
