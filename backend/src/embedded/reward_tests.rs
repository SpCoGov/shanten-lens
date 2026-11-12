//! Offline server simulation: no socket, account, or SMTP connection is created.
use super::*;
use crate::{ipc::AutorunAction, pipeline::{Direction, Packet}, protocol::{LiqiCodec, MessageType}};
use hudsucker::tokio_tungstenite::tungstenite::Message;

struct Rewards {
    runtime: BackendRuntime,
    processor: std::sync::Arc<std::sync::Mutex<crate::runtime::FlowProcessor>>,
    outgoing: tokio::sync::mpsc::UnboundedReceiver<Message>,
    codec: LiqiCodec,
    effects: Vec<Value>,
    candidates: Value,
    index: u64,
    stage: u64,
    pack: u64,
    goods: Value,
}

impl Rewards {
    fn core() -> Vec<Value> {
        [3771,2321,1691,1661,1501,621,3561,3760].into_iter()
            .map(|id| Self::effect(id,id,if id == 3760 {0} else {600070})).collect()
    }
    fn effect(id: u64, uid: u64, badge: u64) -> Value {
        json!({"id":id,"uid":uid,"volume":if badge == 600160 {2} else {1},"badgeId":badge})
    }
    fn remove(&mut self, family: u64) { self.effects.retain(|e| e["id"].as_u64().unwrap()/10 != family); }
    fn queue(&self) -> Value { json!([{"packId":103,"count":3u64.saturating_sub(self.index),"source":1}]) }
    fn publish(&self) {
        let flow = self.processor.lock().unwrap().flow_id;
        self.runtime.state.services.update_game_state_from_flow(&Packet {direction:Direction::Inbound,packet_type:"Res".into(),id:Some(1),
            method:".lq.Lobby.fetchAmuletActivityData".into(),data:json!({"game":{
                "state":{"current":self.stage,"interrupt":0},"map":{"level":14017,"node":2},"game":{"coin":"1000"},
                "character":{"hp":50},"record":{"levelHuCount":0},
                "effect":{"effectList":self.effects,"maxEffectVolume":8,"shopBuffList":[],"packDisableSkip":false,
                    "packGoodsId":self.pack,"packCandidates":self.candidates,"currentLevelRewardPack":self.index,"packQueue":self.queue()},
                "shop":{"goods":self.goods,"upgradeGoods":[],"refreshPrice":710}}})},flow);
    }
    async fn start(&mut self) { self.start_with_support(356).await; }
    async fn start_with_support(&mut self, family: u64) {
        self.effects=Self::core();
        let support = self.effects.iter_mut().find(|effect| effect["id"] == 3561).unwrap();
        support["id"]=json!(family * 10 + 1); support["uid"]=json!(family * 10 + 1);
        self.index=0; self.stage=16; self.pack=103; self.goods=json!([]);
        self.runtime.state.automation.capture_stop_mail();
        self.publish();
        assert!(self.runtime.autorun(AutorunAction::Start,true).await.ok);
    }
    async fn request(&mut self, kind: u64, args: Value) -> crate::protocol::ParsedMessage {
        let message = tokio::time::timeout(Duration::from_secs(4),self.outgoing.recv()).await
            .unwrap_or_else(|_| panic!("No expected {kind}: {}",self.runtime.state.automation.autorun_status())).unwrap();
        let Message::Binary(frame) = message else { panic!("expected binary game request") };
        let packet = self.codec.parse(&frame).unwrap();
        assert_eq!(packet.method.as_ref(),".lq.Lobby.amuletActivityOperate");
        assert_eq!(packet.data["type"],kind);
        assert_eq!(packet.data["args"],args);
        assert!(tokio::time::timeout(Duration::from_millis(120),self.outgoing.recv()).await.is_err(),"Must wait for acknowledgement, not send another selection/sale");
        packet
    }
    fn reply(&mut self, packet: crate::protocol::ParsedMessage) {
        let effects: Vec<_> = self.effects.iter().map(|effect| {
            let mut effect=effect.clone();
            let badge=effect.as_object_mut().unwrap().remove("badgeId").unwrap();
            effect["badge"]=json!({"id":badge});effect
        }).collect();
        let candidates: Vec<_> = self.candidates.as_array().unwrap().iter().map(|effect|
            json!({"id":effect["id"],"badgeId":effect["badgeId"]})).collect();
        let response=json!({"events":[{"state":{"current":self.stage,"interrupt":if self.stage==9 && self.candidates.as_array().is_some_and(|a|!a.is_empty()) {1000} else {0}},
            "valueChanges":{"effect":{"effectList":{"dirty":true,"value":effects},"packCandidates":{"dirty":true,"value":candidates},
                "packGoodsId":{"dirty":true,"value":self.pack},"currentLevelRewardPack":{"dirty":true,"value":self.index},"packQueue":{"dirty":true,"value":self.queue()}},
                "shop":{"goods":{"dirty":true,"value":self.goods}}}}]});
        let bytes=self.codec.build(MessageType::Response,packet.id,&packet.method,&response).unwrap();
        self.processor.lock().unwrap().process(&bytes,Direction::Inbound).unwrap();
    }
    async fn stopped_mail(&self, step: &str) {
        tokio::time::timeout(Duration::from_secs(2),async {
            loop {
                let status=self.runtime.state.automation.autorun_status();
                if status["running"]==false && status["email_status"]=="sent" {assert_eq!(status["current_step"],step);break;}
                tokio::task::yield_now().await;
            }
        }).await.unwrap();
        assert_eq!(self.runtime.state.automation.captured_stop_mail().len(),1);
    }
    async fn stop(&mut self) {
        self.runtime.state.automation.stop_autorun();
        assert_eq!(self.runtime.state.automation.autorun_status()["email_status"], "");
        assert!(self.runtime.state.automation.captured_stop_mail().is_empty());
        assert!(tokio::time::timeout(Duration::from_millis(200),self.outgoing.recv()).await.is_err());
    }
}

#[tokio::test]
async fn three_reward_packs_select_replace_recover_and_echo_without_live_account() {
    const ROOT: &str="SHANTEN_REWARD_TEST_ROOT";
    let root=match std::env::var_os(ROOT) {
        Some(root)=>PathBuf::from(root),
        None=>{
            let root=std::env::temp_dir().join(format!("lens-rewards-{}",std::process::id()));
            let result=std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact","embedded::reward_tests::three_reward_packs_select_replace_recover_and_echo_without_live_account","--nocapture"])
                .env(ROOT,&root).output().unwrap();
            assert!(result.status.success(),"{result:?}"); std::fs::remove_dir_all(root).unwrap(); return;
        }
    };
    let runtime=tokio::task::spawn_blocking(move||BackendRuntime::load(root.join("configs"))).await.unwrap().unwrap();
    let mut mail=runtime.state.services.table("autorun")["email_notify"].clone(); mail["enabled"]=json!(true);
    runtime.state.services.patch_config(&json!({"autorun":{"op_interval_ms":100,"email_notify":mail,
        "playbook":{"version":1,"selected_profile_id":"vine","profiles":[{"id":"vine","name":"Vine rewards","strategy":"vine_farming"}]}}})).unwrap();
    let (processor,outgoing,_)=runtime.state.proxy.register_direct("test",runtime.state.module_registry.clone()).await;
    let mut sim=Rewards {runtime,processor,outgoing,codec:LiqiCodec::new(),effects:vec![],candidates:json!([]),index:0,stage:16,pack:103,goods:json!([])};

    // Three consecutive rewards: vine replacement, keep the formed Knight, another vine replacement.
    sim.candidates=json!([Rewards::effect(3760,0,600070)]);sim.start().await;
    let request=sim.request(5,json!([3760])).await;sim.remove(376);sim.reply(request);
    let request=sim.request(16,json!([0])).await;sim.effects.push(Rewards::effect(3760,9100,600070));
    sim.index=1;sim.candidates=json!([Rewards::effect(3560,0,600150)]);sim.reply(request);
    let request=sim.request(3,json!([])).await;
    assert!(sim.effects.iter().any(|e|e["id"]==3561 && e["badgeId"]==600070),"Keep the formed guidance Knight when a friend Knight appears");
    sim.index=2;sim.candidates=json!([Rewards::effect(3760,0,0)]);sim.reply(request);
    let request=sim.request(5,json!([9100])).await;sim.remove(376);sim.reply(request);
    let request=sim.request(16,json!([0])).await;sim.effects.push(Rewards::effect(3760,9102,0));
    sim.index=3;sim.stage=9;sim.candidates=json!([]);sim.reply(request);sim.stop().await;
    assert_eq!(sim.effects.iter().filter(|e|e["id"]==3760).count(),1);
    assert!(!sim.effects.iter().any(|e|e["id"]==3761));

    for family in [356,351,160] {
        let temporary=if family == 356 {351} else {356};
        // Expansion and ordinary vine, then a gold reward selected and sold to recover coins.
        sim.candidates=json!([Rewards::effect(3760,0,600160)]);sim.start_with_support(family).await;
        let request=sim.request(5,json!([family * 10 + 1])).await;sim.remove(family);sim.reply(request);
        let request=sim.request(5,json!([3760])).await;sim.remove(376);sim.reply(request);
        let request=sim.request(16,json!([0])).await;sim.effects.push(Rewards::effect(3760,9200,600160));
        sim.index=1;sim.candidates=json!([Rewards::effect(3760,0,0)]);sim.reply(request);
        let request=sim.request(5,json!([9200])).await;sim.remove(376);sim.reply(request);
        let request=sim.request(16,json!([0])).await;sim.effects.push(Rewards::effect(3760,9201,0));
        sim.index=2;sim.candidates=json!([Rewards::effect(3390,0,0)]);sim.reply(request);
        let request=sim.request(16,json!([0])).await;sim.effects.push(Rewards::effect(3390,9202,0));
        sim.index=3;sim.stage=9;sim.candidates=json!([]);
        sim.goods=json!([{"id":20,"goodsId":103,"price":15,"sold":false},
            {"id":21,"goodsId":102,"price":10,"sold":false},{"id":22,"goodsId":102,"price":10,"sold":false},
            {"id":23,"goodsId":102,"price":10,"sold":false},{"id":24,"goodsId":103,"price":15,"sold":false}]);sim.reply(request);
        let request=sim.request(5,json!([9202])).await;sim.remove(339);sim.reply(request);
        let request=sim.request(4,json!([21])).await;sim.goods[1]["sold"]=json!(true);sim.pack=102;
        sim.candidates=json!([Rewards::effect(temporary * 10,0,0)]);sim.reply(request);
        let request=sim.request(16,json!([0])).await;sim.effects.push(Rewards::effect(temporary * 10,9203,0));sim.candidates=json!([]);sim.reply(request);
        assert_eq!(sim.runtime.state.automation.autorun_status()["active_profile"]["support_amulet"],family);
        let request=sim.request(4,json!([22])).await;sim.goods[2]["sold"]=json!(true);
        sim.candidates=json!([Rewards::effect(family * 10,0,0)]);sim.reply(request);
        let request=sim.request(5,json!([9203])).await;sim.remove(temporary);sim.reply(request);
        let request=sim.request(16,json!([0])).await;sim.effects.push(Rewards::effect(family * 10,9204,0));sim.candidates=json!([]);sim.reply(request);
        let request=sim.request(4,json!([23])).await;sim.goods[3]["sold"]=json!(true);
        sim.candidates=json!([Rewards::effect(family * 10,0,600150)]);sim.reply(request);
        let request=sim.request(16,json!([0])).await;sim.remove(family);sim.effects.push(Rewards::effect(family * 10 + 1,9204,600150));sim.candidates=json!([]);sim.reply(request);
        // Friend PLUS remains temporary: find guidance in a gold pack after blue packs are exhausted.
        let request=sim.request(4,json!([20])).await;sim.goods[0]["sold"]=json!(true);sim.pack=103;
        sim.candidates=json!([Rewards::effect(family * 10,0,600070)]);sim.reply(request);
        let request=sim.request(16,json!([0])).await;sim.remove(family);sim.effects.push(Rewards::effect(family * 10 + 1,9204,600070));sim.candidates=json!([]);sim.reply(request);
        let request=sim.request(4,json!([24])).await;sim.candidates=json!([Rewards::effect(3390,0,0)]);sim.reply(request);sim.stop().await;
        for core in [377,232,169,166,150,62] {assert!(sim.effects.iter().any(|e|e["id"].as_u64().unwrap()/10==core));}
        assert!(sim.effects.iter().any(|e|e["id"]==family * 10 + 1 && e["badgeId"]==600070));
    }

    // Echo at any reward position follows normal vine replacement and continues through the queue.
    for echo_index in 0..3 {
        sim.candidates=if echo_index==0 {json!([Rewards::effect(3760,0,600180)])} else {json!([Rewards::effect(3390,0,0)])};
        sim.start().await;
        for index in 0..echo_index {
            let request=sim.request(3,json!([])).await;sim.index=index+1;
            sim.candidates=if sim.index==echo_index {json!([Rewards::effect(3760,0,600180)])} else {json!([Rewards::effect(3390,0,0)])};sim.reply(request);
        }
        let request=sim.request(5,json!([3760])).await;sim.remove(376);sim.reply(request);
        let request=sim.request(16,json!([0])).await;
        sim.effects.push(Rewards::effect(3760,9300+echo_index,600180));sim.index=echo_index+1;
        sim.candidates=if sim.index < 3 {json!([Rewards::effect(3390,0,0)])} else {json!([])};
        if sim.index==3 {sim.stage=9;} sim.reply(request);
        for index in (echo_index+1)..3 {
            let request=sim.request(3,json!([])).await;sim.index=index+1;
            sim.candidates=if sim.index < 3 {json!([Rewards::effect(3390,0,0)])} else {json!([])};
            if sim.index==3 {sim.stage=9;} sim.reply(request);
        }
        assert_eq!(sim.runtime.state.automation.autorun_status()["running"],true);
        assert!(sim.runtime.state.automation.captured_stop_mail().is_empty());
        assert!(sim.effects.iter().any(|e|e["id"]==3760 && e["badgeId"]==600180));
        sim.stop().await;
    }

    // Ex1 event and health stops must leave the real game untouched and send one notification.
    let flow=sim.processor.lock().unwrap().flow_id;
    let publish_stage = |runtime: &BackendRuntime, stage: u64, event: Value, hp: u64, max_hp: u64| {
        runtime.state.services.update_game_state_from_flow(&Packet {direction:Direction::Inbound,packet_type:"Res".into(),id:Some(1),
            method:".lq.Lobby.fetchAmuletActivityData".into(),data:json!({"game":{
                "state":{"current":stage,"interrupt":0},"map":{"level":14001,"node":2},
                "character":{"hp":hp,"maxHp":max_hp},"game":{"coin":"1000"},"record":{"levelHuCount":0},
                "nodeEvent":event,"effect":{"effectList":Rewards::core(),"maxEffectVolume":8,"shopBuffList":[],
                    "packDisableSkip":false,"packCandidates":[{"id":3760,"badgeId":0}],"packGoodsId":103},
                "shop":{"goods":[],"upgradeGoods":[],"refreshPrice":710}}})},flow);
    };
    for (stage,event,hp,max_hp,step) in [
        (11,json!({}),50,50,"vine_mystery"),
        (12,json!({"tradeReward":{"type":1,"effectId":3760,"badgeId":0}}),50,50,"vine_trade"),
        (6,json!({}),24,50,"vine_low_hp"),
    ] {
        sim.runtime.state.automation.capture_stop_mail();
        publish_stage(&sim.runtime,stage,event,hp,max_hp);
        assert!(sim.runtime.autorun(AutorunAction::Start,true).await.ok);
        sim.stopped_mail(step).await;
        assert!(tokio::time::timeout(Duration::from_millis(200),sim.outgoing.recv()).await.is_err(),"No operation after protected event or low HP");
    }
    sim.runtime.state.services.patch_config(&json!({"autorun":{"playbook":{"version":1,"selected_profile_id":"vine",
        "profiles":[{"id":"vine","name":"Ex1 cutoff","strategy":"vine_farming","stop_ex":1}]}}})).unwrap();
    sim.runtime.state.automation.capture_stop_mail();
    publish_stage(&sim.runtime,13,json!({"bonfireSelection":[1,2,3]}),50,50);
    assert!(sim.runtime.autorun(AutorunAction::Start,true).await.ok);
    for (kind,args,next_stage) in [(13,json!([2]),15),(3,json!([]),14),(3,json!([]),12),(3,json!([]),16)] {
        let packet=sim.request(kind,args).await;
        let bytes=sim.codec.build(MessageType::Response,packet.id,&packet.method,&json!({})).unwrap();
        sim.processor.lock().unwrap().process(&bytes,Direction::Inbound).unwrap();
        publish_stage(&sim.runtime,next_stage,json!({"gamblePrice":5,"tradeReward":{"type":1,"effectId":3390}}),50,50);
    }
    sim.stopped_mail("vine_cutoff").await;
    assert!(tokio::time::timeout(Duration::from_millis(200),sim.outgoing.recv()).await.is_err(),"Cutoff leaves the reward unselected");
    sim.runtime.state.proxy.unregister_direct();
}
