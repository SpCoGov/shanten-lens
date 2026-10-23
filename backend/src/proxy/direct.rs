use super::*;
use crate::direct_connection::{Socket, REQUEST_TIMEOUT};

pub(super) fn capture_region(endpoint: &str) -> Option<String> {
    let url = reqwest::Url::parse(endpoint).ok()?;
    let host = url.host_str()?;
    let region = match host {
        "route-2.maj-soul.com"
        | "route-3.maj-soul.com"
        | "route-4.maj-soul.com"
        | "route-5.maj-soul.com"
        | "route-6.maj-soul.com" => "cn",
        "jpgs.mahjongsoul.com" | "jpgsbk.mahjongsoul.com" => "jp",
        "engs.mahjongsoul.com" | "engsbk.mahjongsoul.com" => "global",
        _ => return None,
    };
    (matches!(url.scheme(), "wss" | "https")
        && url.path() == "/gateway"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port_or_known_default()
            == Some(if host == "route-3.maj-soul.com" {
                8443
            } else {
                443
            }))
    .then(|| region.into())
}

impl ProxyControl {
    pub(crate) async fn register_direct(
        &self,
        endpoint: &str,
        registry: Arc<ModuleRegistry>,
    ) -> (
        SharedFlow,
        mpsc::UnboundedReceiver<Message>,
        mpsc::UnboundedReceiver<Message>,
    ) {
        let mut processor = FlowProcessor::with_registry(
            self.config.read().await.clone(),
            self.services.clone(),
            registry,
        );
        processor.direct = true;
        let id = processor.flow_id;
        let processor = Arc::new(Mutex::new(processor));
        let (outbound, outgoing) = mpsc::unbounded_channel();
        let (inbound, local) = mpsc::unbounded_channel();
        *self.direct.lock().unwrap() = Some(FlowEntry {
            processor: processor.clone(),
            info: FlowInfo {
                id,
                peer_key: "direct".into(),
                client: "Shanten Lens".into(),
                server: endpoint.into(),
            },
            outbound: Some(outbound),
            inbound: Some(inbound),
            activity: 0,
            business_activity: 0,
        });
        self.services.select_packet_transport(Some(id));
        (processor, outgoing, local)
    }

    pub(crate) fn unregister_direct(&self) {
        let entry = self.direct.lock().unwrap().take();
        if let Some(entry) = entry {
            entry.processor.lock().unwrap().cancel_requests();
            self.services.disconnect_game_flow(entry.info.id);
        }
    }

    pub(crate) async fn direct_request(&self, method: &str, data: &Value) -> Result<Value, String> {
        let config = self.config.read().await.clone();
        self.request_inner(method, data, REQUEST_TIMEOUT, false, config, None, true, None)
            .await
            .map(|(_, response)| response)
    }

    pub(crate) async fn run_direct_socket(
        &self,
        mut socket: Socket,
        processor: SharedFlow,
        mut outgoing: mpsc::UnboundedReceiver<Message>,
        mut local: mpsc::UnboundedReceiver<Message>,
        notices: Vec<Vec<u8>>,
    ) -> Result<(), String> {
        for notice in notices {
            self.process_direct_frame(&processor, &mut socket, notice)
                .await?;
        }
        loop {
            tokio::select! {
                message = outgoing.recv() => {
                    let message = message.ok_or("direct-connection-closed")?;
                    tokio::time::timeout(REQUEST_TIMEOUT, socket.send(message)).await
                        .map_err(|_| "direct-send-timeout")?.map_err(|_| "direct-send-failed")?;
                }
                // Inbound injections have already passed through the pipeline; there is no game client to forward them to.
                message = local.recv() => { message.ok_or("direct-connection-closed")?; }
                message = socket.next() => match message {
                    Some(Ok(Message::Binary(bytes))) => self.process_direct_frame(&processor, &mut socket, bytes.to_vec()).await?,
                    Some(Ok(Message::Ping(bytes))) => {
                        tokio::time::timeout(REQUEST_TIMEOUT, socket.send(Message::Pong(bytes))).await
                            .map_err(|_| "direct-send-timeout")?.map_err(|_| "direct-send-failed")?;
                    }
                    Some(Ok(Message::Close(_))) | None => return Err("direct-connection-closed".into()),
                    Some(Err(_)) => return Err("direct-receive-failed".into()),
                    _ => {}
                }
            }
        }
    }

    async fn process_direct_frame(
        &self,
        processor: &SharedFlow,
        socket: &mut Socket,
        frame: Vec<u8>,
    ) -> Result<(), String> {
        let config = self.config.read().await.clone();
        let processor = processor.clone();
        let outcome = tokio::task::spawn_blocking(move || {
            let mut processor = processor.lock().map_err(|_| "flow processor poisoned")?;
            processor.set_config(config)?;
            processor
                .process(&frame, Direction::Inbound)
                .map_err(|error| error.to_string())
        })
        .await
        .map_err(|_| "direct-frame-processing-failed")??;
        if let FrameOutcome::Inject { injected, .. } = outcome {
            for frame in injected
                .into_iter()
                .filter(|frame| frame.direction == Direction::Outbound)
            {
                tokio::time::timeout(
                    REQUEST_TIMEOUT,
                    socket.send(Message::Binary(frame.frame.into())),
                )
                .await
                .map_err(|_| "direct-send-timeout")?
                .map_err(|_| "direct-send-failed")?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{LiqiCodec, MessageType};
    use hudsucker::tokio_tungstenite::{accept_async, connect_async_tls_with_config};

    fn setup() -> (
        std::path::PathBuf,
        Arc<Services>,
        ProxyControl,
        Arc<ModuleRegistry>,
    ) {
        let root = std::env::temp_dir().join(format!(
            "lens-direct-flow-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let (events, _) = tokio::sync::broadcast::channel(128);
        let services = Arc::new(Services::load(root.join("configs"), events).unwrap());
        let config = crate::runtime::normalize_config(PipelineConfig::default());
        let registry = Arc::new(ModuleRegistry::default());
        crate::runtime::register_builtin_modules(&registry, &config);
        let proxy = ProxyControl::new(services.clone(), Arc::new(RwLock::new(config)));
        (root, services, proxy, registry)
    }

    fn publish_stage(processor: &mut FlowProcessor, stage: u64) {
        let method = ".lq.Lobby.fetchAmuletActivityData";
        let request = processor
            .build_request(7, method, &json!({"activityId":261081}))
            .unwrap();
        processor.process(&request, Direction::Outbound).unwrap();
        let response = LiqiCodec::new()
            .build(
                MessageType::Response,
                Some(7),
                method,
                &json!({"data":{"game":{"state":{"current":stage}}}}),
            )
            .unwrap();
        processor.process(&response, Direction::Inbound).unwrap();
    }

    #[test]
    fn capture_only_accepts_official_gateways() {
        assert_eq!(
            capture_region("wss://route-3.maj-soul.com:8443/gateway").as_deref(),
            Some("cn")
        );
        assert_eq!(
            capture_region("wss://engs.mahjongsoul.com/gateway").as_deref(),
            Some("global")
        );
        assert!(capture_region("wss://route-3.maj-soul.com.evil.test/gateway").is_none());
        assert!(capture_region("ws://jpgs.mahjongsoul.com/gateway").is_none());
    }

    #[tokio::test]
    async fn direct_and_mitm_flows_never_publish_each_others_state() {
        let (root, services, proxy, registry) = setup();
        let mut mitm =
            FlowProcessor::with_services(proxy.config.read().await.clone(), services.clone());
        publish_stage(&mut mitm, 5);
        assert_eq!(services.game_state()["stage"], 5);
        let (direct, _, _) = proxy
            .register_direct("wss://jpgs.mahjongsoul.com/gateway", registry)
            .await;
        assert_eq!(services.game_state()["stage"], -1);
        publish_stage(&mut mitm, 4);
        assert_eq!(services.game_state()["stage"], -1);
        publish_stage(&mut direct.lock().unwrap(), 2);
        assert_eq!(services.game_state()["stage"], 2);
        publish_stage(&mut mitm, 3);
        assert_eq!(services.game_state()["stage"], 2);
        services.disconnect_game_flow(mitm.flow_id);
        assert_eq!(services.game_state()["stage"], 2);
        proxy.unregister_direct();
        services.select_packet_transport(Some(0));
        publish_stage(&mut mitm, 4);
        assert_eq!(services.game_state()["stage"], -1);
        services.select_packet_transport(None);
        publish_stage(&mut direct.lock().unwrap(), 3);
        assert_eq!(services.game_state()["stage"], -1);
        publish_stage(&mut mitm, 5);
        assert_eq!(services.game_state()["stage"], 5);
        drop((direct, mitm, proxy, services));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn direct_socket_correlates_concurrent_requests_and_disconnect_releases_waiters() {
        let (root, services, proxy, registry) = setup();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("ws://{}/gateway", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let mut socket = accept_async(listener.accept().await.unwrap().0)
                .await
                .unwrap();
            let mut codec = LiqiCodec::new();
            let mut requests = Vec::new();
            for _ in 0..2 {
                let Message::Binary(frame) = socket.next().await.unwrap().unwrap() else {
                    panic!("expected request")
                };
                requests.push(codec.parse(&frame).unwrap());
            }
            for request in requests.into_iter().rev() {
                let response = codec
                    .build(
                        MessageType::Response,
                        request.id,
                        &request.method,
                        &json!({"error":{"code":request.data["delay"]}}),
                    )
                    .unwrap();
                socket.send(Message::Binary(response.into())).await.unwrap();
            }
            let Message::Binary(_) = socket.next().await.unwrap().unwrap() else {
                panic!("expected pending request")
            };
            socket.close(None).await.unwrap();
        });
        let (socket, _) = connect_async_tls_with_config(&endpoint, None, false, None)
            .await
            .unwrap();
        let (processor, outgoing, local) = proxy.register_direct(&endpoint, registry).await;
        let runner = proxy.clone();
        let flow = processor.clone();
        let run = tokio::spawn(async move {
            runner
                .run_direct_socket(socket, flow, outgoing, local, vec![])
                .await
        });
        let one = json!({"delay":11});
        let two = json!({"delay":22});
        let (one, two) = tokio::join!(
            proxy.request(".lq.Route.heartbeat", &one, Duration::from_secs(2)),
            proxy.request(".lq.Route.heartbeat", &two, Duration::from_secs(2)),
        );
        assert_eq!(one.unwrap().1["error"]["code"], 11);
        assert_eq!(two.unwrap().1["error"]["code"], 22);
        let pending_proxy = proxy.clone();
        let pending = tokio::spawn(async move {
            pending_proxy
                .direct_request(".lq.Route.heartbeat", &json!({}))
                .await
        });
        assert!(tokio::time::timeout(Duration::from_secs(2), run)
            .await
            .unwrap()
            .unwrap()
            .is_err());
        proxy.unregister_direct();
        assert!(tokio::time::timeout(Duration::from_secs(2), pending)
            .await
            .unwrap()
            .unwrap()
            .is_err());
        server.await.unwrap();
        drop((processor, proxy, services));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn direct_heartbeat_works_while_qyzz_is_selected_and_user_requests_stay_blocked() {
        let (root, services, proxy, registry) = setup();
        services.publish_qyzz_state(&json!({})).unwrap();
        let (processor, mut outgoing, _local) = proxy
            .register_direct("wss://jpgs.mahjongsoul.com/gateway", registry)
            .await;
        assert!(!services.packet_processing_allowed());
        assert!(proxy
            .request(".lq.Route.heartbeat", &json!({}), Duration::from_secs(1))
            .await
            .is_err());
        let responder = async {
            let Message::Binary(frame) = outgoing.recv().await.unwrap() else {
                panic!("expected heartbeat")
            };
            let mut codec = LiqiCodec::new();
            let request = codec.parse(&frame).unwrap();
            assert_eq!(request.method.as_ref(), ".lq.Route.heartbeat");
            let reply = codec
                .build(
                    MessageType::Response,
                    request.id,
                    &request.method,
                    &json!({}),
                )
                .unwrap();
            processor
                .lock()
                .unwrap()
                .process(&reply, Direction::Inbound)
                .unwrap();
        };
        let payload = json!({});
        let (response, _) = tokio::join!(
            proxy.direct_request(".lq.Route.heartbeat", &payload),
            responder
        );
        assert!(response.is_ok());
        assert_eq!(services.game_state()["source"], "qyzz");
        proxy.unregister_direct();
        drop((processor, outgoing, _local, proxy, services));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn changing_transport_during_a_request_never_sends_to_the_previous_connection() {
        struct PausedModule {
            entered: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
            resume: Mutex<std::sync::mpsc::Receiver<()>>,
        }
        impl crate::pipeline::ExternalPacketModule for PausedModule {
            fn notify(&self, _: &crate::pipeline::Packet, _: u64) -> Result<(), String> {
                Ok(())
            }
            fn decide(
                &self,
                _: &crate::pipeline::Packet,
                _: u64,
                _: Duration,
            ) -> Result<crate::pipeline::ExternalDecision, String> {
                self.entered
                    .lock()
                    .unwrap()
                    .take()
                    .unwrap()
                    .send(())
                    .unwrap();
                self.resume
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(3))
                    .unwrap();
                Ok(crate::pipeline::ExternalDecision::Pass)
            }
        }
        let (root, services, proxy, registry) = setup();
        let (entered, wait_entered) = tokio::sync::oneshot::channel();
        let (resume, wait_resume) = std::sync::mpsc::channel();
        let mut info = crate::pipeline::PacketModuleInfo::builtin(
            "pause-transport-test",
            vec![crate::pipeline::PacketSubscription {
                direction: Some(Direction::Outbound),
                packet_type: Some("Req".into()),
                method: Some(".lq.Lobby.fetchConnectionInfo".into()),
                operations: vec![crate::pipeline::PacketOperation::Edit],
            }],
        );
        info.builtin = false;
        info.provider_id = "test".into();
        registry
            .register_external(
                info,
                Arc::new(PausedModule {
                    entered: Mutex::new(Some(entered)),
                    resume: Mutex::new(wait_resume),
                }),
            )
            .unwrap();
        proxy
            .config
            .write()
            .await
            .modules
            .push(crate::pipeline::ModuleConfig {
                id: "pause-transport-test".into(),
                enabled: true,
                options: Value::Null,
            });
        let handler = Handler {
            config: proxy.config.clone(),
            module_registry: registry,
            services: services.clone(),
            control: proxy.clone(),
            http_request: None,
        };
        let (outbound, mut sent) = mpsc::unbounded_channel();
        let (inbound, _received) = mpsc::unbounded_channel();
        let address = "127.0.0.1:23456".parse().unwrap();
        handler
            .register(
                address,
                "wss://jpgs.mahjongsoul.com/gateway",
                Direction::Outbound,
                outbound,
            )
            .await;
        handler
            .register(
                address,
                "wss://jpgs.mahjongsoul.com/gateway",
                Direction::Inbound,
                inbound,
            )
            .await;
        let automation = crate::automation::Automation::new(services.clone(), proxy.clone());
        assert_eq!(automation.autorun_status()["preferred_flow_ready"], true);
        let requester = proxy.clone();
        let request = tokio::spawn(async move {
            requester
                .request(
                    ".lq.Lobby.fetchConnectionInfo",
                    &json!({}),
                    Duration::from_secs(1),
                )
                .await
        });
        tokio::time::timeout(Duration::from_secs(2), wait_entered)
            .await
            .unwrap()
            .unwrap();
        services.select_packet_transport(Some(0));
        assert_eq!(automation.autorun_status()["game_ready"], false);
        assert_eq!(automation.autorun_status()["preferred_flow_ready"], false);
        services.select_packet_transport(None);
        resume.send(()).unwrap();
        assert!(tokio::time::timeout(Duration::from_secs(2), request)
            .await
            .unwrap()
            .unwrap()
            .is_err());
        assert!(
            sent.try_recv().is_err(),
            "A stale operation reached the old MITM client after a transport switch"
        );
        drop((automation, handler, proxy, services));
        std::fs::remove_dir_all(root).unwrap();
    }
}
