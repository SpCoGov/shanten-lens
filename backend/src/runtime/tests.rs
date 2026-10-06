use super::builtin::subscription;
use super::fuse::FuseRules;
use super::*;
use crate::pipeline::{ModuleAction, PacketModule, PacketModuleInfo, PacketOperation};
use prost::Message;
use serde_json::json;
use std::{fs, path::PathBuf};
use tokio::sync::broadcast;

#[test]
fn retired_activity_module_is_removed_without_changing_plugin_settings() {
    let mut config = PipelineConfig::default();
    config.modules.push(crate::pipeline::ModuleConfig {
        id: "limited_time_activity".into(),
        enabled: true,
        options: Value::Null,
    });
    let plugin = crate::pipeline::ModuleConfig {
        id: "plugin:shanten-lens.activity:main:activities".into(),
        enabled: false,
        options: json!({"timeout_ms": 750}),
    };
    config.modules.push(plugin.clone());
    let config = normalize_config(config);
    assert!(!config
        .modules
        .iter()
        .any(|module| module.id == "limited_time_activity"));
    assert_eq!(config.modules.last(), Some(&plugin));
    assert!(!builtin_modules(None, 0)
        .iter()
        .any(|module| module.id() == "limited_time_activity"));
}

#[derive(Clone, PartialEq, Message)]
struct Wrapper {
    #[prost(string, tag = "1")]
    name: String,
    #[prost(bytes = "vec", tag = "2")]
    data: Vec<u8>,
}

fn request_frame() -> Vec<u8> {
    let wrapper = Wrapper {
        name: ".lq.Lobby.fetchConnectionInfo".into(),
        data: Vec::new(),
    };
    let mut frame = vec![2, 7, 0];
    wrapper.encode(&mut frame).unwrap();
    frame
}

fn temp_root(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "shanten-lens-runtime-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

#[test]
fn external_request_replacement_preserves_id_and_receives_server_response() {
    struct Replace;
    impl crate::pipeline::ExternalPacketModule for Replace {
        fn notify(&self, _: &Packet, _: u64) -> Result<(), String> {
            Ok(())
        }
        fn decide(
            &self,
            _: &Packet,
            revision: u64,
            _: std::time::Duration,
        ) -> Result<crate::pipeline::ExternalDecision, String> {
            Ok(crate::pipeline::ExternalDecision::Edit {
                method: Some(".lq.Lobby.loginBeat".into()),
                data: json!({"contract":"test"}),
                expected_revision: revision,
            })
        }
    }
    let registry = Arc::new(ModuleRegistry::default());
    let mut info = PacketModuleInfo::builtin(
        "soulmax",
        vec![subscription(
            Some(Direction::Outbound),
            Some("Req"),
            Some(".lq.Lobby.fetchConnectionInfo"),
            &[PacketOperation::Edit],
        )],
    );
    info.builtin = false;
    info.provider_id = "test".into();
    registry.register_external(info, Arc::new(Replace)).unwrap();
    let mut processor = FlowProcessor::build(
        PipelineConfig {
            schema: 1,
            modules: vec![crate::pipeline::ModuleConfig {
                id: "soulmax".into(),
                enabled: true,
                options: Value::Null,
            }],
        },
        None,
        registry,
    );
    let FrameOutcome::Forward(frame) = processor
        .process(&request_frame(), Direction::Outbound)
        .unwrap()
    else {
        panic!("request not forwarded")
    };
    let mut server = LiqiCodec::new();
    let request = server.parse(&frame).unwrap();
    assert_eq!(request.id, Some(7));
    assert_eq!(request.method.as_ref(), ".lq.Lobby.loginBeat");
    assert_eq!(request.data["contract"], "test");
    let response = server
        .build(
            MessageType::Response,
            Some(7),
            ".lq.Lobby.loginBeat",
            &json!({}),
        )
        .unwrap();
    assert_eq!(
        processor.process(&response, Direction::Inbound).unwrap(),
        FrameOutcome::Forward(response)
    );
    assert!(!processor.codec.has_pending(7));
}

#[test]
fn hanabi_guard_distinguishes_game_operate_from_sort_effect() {
    let root = temp_root("hanabi");
    let (events, mut receiver) = broadcast::channel(32);
    let services = Arc::new(Services::load(root.join("configs"), events).unwrap());
    services.update_game_state(&Packet {direction:Direction::Inbound,packet_type:"Res".into(),method:".lq.Lobby.fetchAmuletActivityData".into(),id:Some(1),data:json!({"game":{"state":{"current":5},"effect":{"effectList":[{"id":2221}]},"round":{"ming":[]}}})});
    let mut guard = FuseRules(Some(services.clone()));
    let mut packet = Packet {
        direction: Direction::Outbound,
        packet_type: "Req".into(),
        method: ".lq.Lobby.amuletActivityOperate".into(),
        id: Some(2),
        data: json!({"type":8,"args":[]}),
    };
    assert!(matches!(
        guard.process(&mut packet, &Value::Null),
        ModuleAction::Forward
    ));
    packet.method = ".lq.Lobby.amuletActivityGameOperate".into();
    let responder = services.clone();
    let answer = std::thread::spawn(move || {
        while let Ok(event) = receiver.blocking_recv() {
            if event["type"] == "msgbox" {
                assert_eq!(event["data"]["title"], "fuse.guard.hanabiWin.title");
                responder.resolve_confirmation(event["data"]["id"].as_str().unwrap(), false);
                return;
            }
        }
    });
    assert!(matches!(
        guard.process(&mut packet, &Value::Null),
        ModuleAction::Drop
    ));
    answer.join().unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn waiters_are_per_flow_and_complete_after_state_commit() {
    let base = temp_root("flow-waiters");
    let (events, _) = broadcast::channel(16);
    let services = Arc::new(Services::load(base.clone(), events).unwrap());
    let config = PipelineConfig {
        schema: 1,
        modules: vec![crate::pipeline::ModuleConfig {
            id: "game_state".into(),
            enabled: true,
            options: Value::Null,
        }],
    };
    let mut a = FlowProcessor::with_services(config.clone(), services.clone());
    let mut b = FlowProcessor::with_services(config, services.clone());
    let method = ".lq.Lobby.amuletActivityOperate";
    for processor in [&mut a, &mut b] {
        processor
            .build_request(42, method, &json!({"type":3}))
            .unwrap();
    }
    let mut waiting_a = a.response_waiter(42);
    let mut waiting_b = b.response_waiter(42);
    let response = LiqiCodec::new()
        .build(
            MessageType::Response,
            Some(42),
            method,
            &json!({"events":[{"type":2,"state":{"current":3}}]}),
        )
        .unwrap();
    b.process(&response, Direction::Inbound).unwrap();
    assert!(waiting_a.try_recv().is_err());
    assert!(waiting_b.try_recv().is_ok());
    assert_eq!(services.game_state()["stage"], 3);
    assert_eq!(services.game_state()["flow_id"], b.flow_id);
    b.build_request(42, method, &json!({"type":3})).unwrap();
    b.cancel_waiting_request(42);
    assert!(
        b.codec.has_pending(42),
        "completed request cleanup must not erase a reused client ID"
    );
    a.cancel_requests();
    assert!(matches!(
        waiting_a.try_recv(),
        Err(tokio::sync::oneshot::error::TryRecvError::Closed)
    ));
    assert!(!a.codec.has_pending(42));
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn unmodified_frame_roundtrips_exactly() {
    let frame = request_frame();
    let output = FlowProcessor::new(PipelineConfig {
        schema: 1,
        modules: vec![],
    })
    .process(&frame, Direction::Outbound)
    .unwrap();
    assert_eq!(output, FrameOutcome::Forward(frame));
}

#[test]
fn real_protobuf_event_response_updates_game_state_end_to_end() {
    let base = temp_root("event");
    let (events, _) = broadcast::channel(4);
    let services = Arc::new(Services::load(base.join("configs"), events).unwrap());
    let config = PipelineConfig {
        schema: 1,
        modules: vec![crate::pipeline::ModuleConfig {
            id: "game_state".into(),
            enabled: true,
            options: Value::Null,
        }],
    };
    let mut processor = FlowProcessor::with_services(config, services.clone());
    let request = processor
        .build_request(
            23,
            ".lq.Lobby.amuletActivityOperate",
            &json!({"activityId":260511,"type":3,"args":[]}),
        )
        .unwrap();
    processor.process(&request, Direction::Outbound).unwrap();
    let response = processor
        .codec
        .build(
            MessageType::Response,
            Some(23),
            ".lq.Lobby.amuletActivityOperate",
            &json!({"events":[{
                "type":2,
                "state":{"current":3},
                "valueChanges":{
                    "character":{
                        "characterId":{"dirty":true,"value":200001},
                        "hp":{"dirty":true,"value":8},
                        "maxHp":{"dirty":true,"value":10}
                    },
                    "map":{"node":{"dirty":true,"value":2}},
                    "round":{"enemy":{"dirty":true,"value":{"hp":"90","maxHp":"120"}}}
                }
            }]}),
        )
        .unwrap();
    processor.process(&response, Direction::Inbound).unwrap();
    assert_eq!(processor.next_replay_id(), 24);
    assert_eq!(
        processor
            .build_replay_request(
                ".lq.Lobby.amuletActivityOperate",
                &json!({"activityId":260511,"type":3,"args":[]}),
            )
            .unwrap()
            .0,
        24
    );

    let state = services.game_state();
    assert_eq!(state["stage"], 3);
    assert_eq!(state["character_id"], 200001);
    assert_eq!(state["hp"], 8);
    assert_eq!(state["max_hp"], 10);
    assert_eq!(state["node"], 2);
    assert_eq!(state["point"], "90");
    assert_eq!(state["target_point"], "120");

    fs::remove_dir_all(base).unwrap();
}

#[test]
fn unselected_packet_source_forwards_without_running_modules() {
    struct UnexpectedModule;
    impl PacketModule for UnexpectedModule {
        fn id(&self) -> &'static str { "unselected-module" }
        fn subscriptions(&self, _: &Value) -> Vec<crate::pipeline::PacketSubscription> {
            vec![subscription(None, None, None, &[PacketOperation::Read])]
        }
        fn process(&mut self, _: &mut Packet, _: &Value) -> ModuleAction {
            panic!("Unselected source reached a packet module");
        }
    }
    let root = temp_root("source-bypass");
    let (events, _) = broadcast::channel(32);
    let services = Arc::new(Services::load(root.join("configs"), events).unwrap());
    services.publish_qyzz_state(&json!({})).unwrap();
    let config = PipelineConfig { schema: 1, modules: vec![crate::pipeline::ModuleConfig {
        id: "unselected-module".into(), enabled: true, options: Value::Null,
    }] };
    let mut processor = FlowProcessor::with_services(config.clone(), services.clone());
    processor.pipeline = Pipeline::new(config, vec![Box::new(UnexpectedModule) as Box<dyn PacketModule>]);
    assert_eq!(processor.process(&request_frame(), Direction::Outbound).unwrap(), FrameOutcome::Forward(request_frame()));
    let request = processor.build_request(8, ".lq.Lobby.fetchAmuletActivityData", &json!({"activityId":260511})).unwrap();
    let mut remote = LiqiCodec::new();
    remote.parse(&request).unwrap();
    let reply = remote.build(MessageType::Response, Some(8), ".lq.Lobby.fetchAmuletActivityData",
        &json!({"data":{"game":{"state":{"current":5}}}})).unwrap();
    assert_eq!(processor.process(&reply, Direction::Inbound).unwrap(), FrameOutcome::Forward(reply));
    assert!(services.data_source_status().pending);
    assert_eq!(services.game_state()["stage"], -1);
    assert_eq!(processor.process(&request_frame(), Direction::Outbound).unwrap(), FrameOutcome::Forward(request_frame()));
    drop(processor);
    drop(services);
    fs::remove_dir_all(root).unwrap();
}
