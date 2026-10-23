use super::*;
use crate::{action_executor::{ActionExecutionRequest, ActionExecutor}, pipeline::Packet, protocol::{LiqiCodec, MessageType}};

fn publish(services: &Services, flow: u64, stage: u64) {
    services.update_game_state_from_flow(&Packet {
        direction: Direction::Inbound, packet_type: "Res".into(), id: Some(1),
        method: ".lq.Lobby.fetchAmuletActivityData".into(),
        data: json!({"game":{"state":{"current":stage,"interrupt":0},"map":{"level":13001,"node":1}}}),
    }, flow);
}

fn request(services: &Services, kind: &str) -> ActionExecutionRequest {
    let state = services.game_state();
    let report = &state["action_report"];
    let action = ["actions", "global_actions"].iter().flat_map(|key| report[key].as_array().unwrap())
        .find(|action| action["kind"] == kind).unwrap();
    ActionExecutionRequest { action_id: action["id"].as_str().unwrap().into(), session_id: state["session_id"].as_u64().unwrap(), revision: state["revision"].as_u64().unwrap() }
}

async fn setup(direct: bool) -> (std::path::PathBuf, Arc<Services>, ProxyControl, Arc<ModuleRegistry>, SharedFlow, mpsc::UnboundedReceiver<Message>) {
    let root = std::env::temp_dir().join(format!("lens-executor-{}-{}", std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    let (events, _) = tokio::sync::broadcast::channel(128);
    let services = Arc::new(Services::load(root.join("configs"), events).unwrap());
    let config = crate::runtime::normalize_config(PipelineConfig::default());
    let registry = Arc::new(ModuleRegistry::default());
    crate::runtime::register_builtin_modules(&registry, &config);
    let proxy = ProxyControl::new(services.clone(), Arc::new(RwLock::new(config)));
    let (processor, outgoing) = if direct {
        let (processor, outgoing, _) = proxy.register_direct("test", registry.clone()).await;
        (processor, outgoing)
    } else {
        let handler = Handler { config: proxy.config.clone(), module_registry: registry.clone(), services: services.clone(), control: proxy.clone(), http_request: None };
        let (sender, receiver) = mpsc::unbounded_channel();
        let address = "127.0.0.1:24567".parse().unwrap();
        let processor = handler.register(address, "test", Direction::Outbound, sender.clone()).await;
        handler.register(address, "test", Direction::Inbound, sender).await;
        (processor, receiver)
    };
    publish(&services, processor.lock().unwrap().flow_id, 1);
    (root, services, proxy, registry, processor, outgoing)
}

#[tokio::test]
async fn executes_on_both_transports_and_blocks_duplicate_submission_without_retrying_errors() {
    for direct in [false, true] {
        let (root, services, proxy, registry, processor, mut outgoing) = setup(direct).await;
        let executor = ActionExecutor::default();
        let original = request(&services, "select_character");
        let responder = async {
            let Message::Binary(frame) = outgoing.recv().await.unwrap() else { panic!("expected request") };
            let mut codec = LiqiCodec::new();
            let packet = codec.parse(&frame).unwrap();
            assert_eq!(packet.method.as_ref(), ".lq.Lobby.amuletActivityOperate");
            assert_eq!(packet.data, json!({"activityId":261081,"type":1,"args":[201]}));
            let busy = executor.execute(&services, &proxy, original.clone()).await;
            assert_eq!(busy.reason_key.as_deref(), Some("action_provider.execution.busy"));
            let reply = codec.build(MessageType::Response, packet.id, &packet.method, &json!({"error":{"code":1004}})).unwrap();
            processor.lock().unwrap().process(&reply, Direction::Inbound).unwrap();
        };
        let (result, _) = tokio::join!(executor.execute(&services, &proxy, original.clone()), responder);
        assert!(!result.ok);
        assert_eq!(result.reason_key.as_deref(), Some("action_provider.execution.rejected"));
        assert_eq!(result.reason_values.unwrap()["code"],1004);
        assert_eq!(executor.execute(&services, &proxy, original.clone()).await.reason_key.as_deref(), Some("action_provider.execution.awaiting_state"));
        assert!(outgoing.try_recv().is_err(), "Rejected or repeated operations must not be resent");
        publish(&services, processor.lock().unwrap().flow_id, 3);
        // Giveup publishes an ended state, which immediately supplies start_game.
        let remake = request(&services, "remake");
        let responder = async {
            let Message::Binary(frame) = outgoing.recv().await.unwrap() else { panic!("expected request") };
            let mut codec = LiqiCodec::new();
            let packet = codec.parse(&frame).unwrap();
            assert_eq!(packet.method.as_ref(), ".lq.Lobby.amuletActivityGiveup");
            let reply = codec.build(MessageType::Response, packet.id, &packet.method, &json!({})).unwrap();
            processor.lock().unwrap().process(&reply, Direction::Inbound).unwrap();
        };
        let (result, _) = tokio::join!(executor.execute(&services, &proxy, remake), responder);
        assert!(result.ok);
        assert_eq!(services.game_state()["ended"], true);
        assert!(request(&services,"start_game").revision > original.revision);
        drop((processor, outgoing, proxy, registry, services));
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[tokio::test]
async fn rechecks_state_after_pipeline_processing_before_sending_and_tracks_uncertain_results() {
    struct ChangeState(Arc<Services>);
    impl crate::pipeline::ExternalPacketModule for ChangeState {
        fn notify(&self, _: &Packet, _: u64) -> Result<(), String> { Ok(()) }
        fn decide(&self, _: &Packet, _: u64, _: Duration) -> Result<crate::pipeline::ExternalDecision, String> {
            publish(&self.0, self.0.active_flow_id(), 3);
            Ok(crate::pipeline::ExternalDecision::Pass)
        }
    }
    let (root, services, proxy, registry, processor, mut outgoing) = setup(true).await;
    let mut info = crate::pipeline::PacketModuleInfo::builtin("change-state-test", vec![crate::pipeline::PacketSubscription {
        direction: Some(Direction::Outbound), packet_type: Some("Req".into()), method: Some(".lq.Lobby.amuletActivityOperate".into()), operations: vec![crate::pipeline::PacketOperation::Edit],
    }]);
    info.builtin = false;
    info.provider_id = "test".into();
    registry.register_external(info, Arc::new(ChangeState(services.clone()))).unwrap();
    proxy.config.write().await.modules.push(crate::pipeline::ModuleConfig { id:"change-state-test".into(), enabled:true, options:Value::Null });
    let executor = ActionExecutor::default();
    let result = executor.execute(&services, &proxy, request(&services,"select_character")).await;
    assert_eq!(result.reason_key.as_deref(), Some("action_provider.execution.stale"));
    assert!(outgoing.try_recv().is_err(), "A stale operation must not enter the send queue");
    proxy.config.write().await.modules.retain(|module| module.id != "change-state-test");
    let current = request(&services,"next_node");
    let mut submitted = None;
    let timeout = proxy.request_action(".lq.Lobby.amuletActivityOperate", &json!({"activityId":261081,"type":2,"args":[]}), Duration::from_millis(20), &current, &mut submitted).await;
    assert_eq!(timeout.unwrap_err(), "timeout");
    assert_eq!(submitted, Some((current.session_id, current.revision)), "An uncertain result must retain the submission stamp");
    assert!(outgoing.try_recv().is_ok());
    proxy.unregister_direct();
    drop((processor, outgoing, proxy, registry, services));
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn cancelling_a_sent_operation_still_prevents_duplicate_execution() {
    let (root, services, proxy, registry, processor, mut outgoing) = setup(true).await;
    let executor = ActionExecutor::default();
    let current = request(&services,"select_character");
    let mut execution = Box::pin(executor.execute(&services, &proxy, current.clone()));
    tokio::select! {
        _ = &mut execution => panic!("request should be waiting for a reply"),
        frame = outgoing.recv() => assert!(frame.is_some()),
    }
    drop(execution);
    assert_eq!(executor.execute(&services, &proxy, current).await.reason_key.as_deref(), Some("action_provider.execution.awaiting_state"));
    assert!(outgoing.try_recv().is_err());
    proxy.unregister_direct();
    drop((processor, outgoing, proxy, registry, services));
    std::fs::remove_dir_all(root).unwrap();
}
