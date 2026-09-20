use crate::{
    ipc::FlowDumpItem,
    pipeline::{Direction, ModuleRegistry, PipelineConfig, REPLAY_INJECTOR},
    runtime::{FlowProcessor, FrameOutcome},
    services::Services,
};
use anyhow::{Context, Result};
use hudsucker::{
    certificate_authority::CertificateAuthority,
    futures::{Sink, SinkExt, Stream, StreamExt},
    hyper::http::uri::Authority,
    hyper::Request,
    rcgen::{
        date_time_ymd, string::Ia5String, BasicConstraints, CertificateParams, DistinguishedName,
        DnType, IsCa, Issuer, KeyPair, KeyUsagePurpose, SanType,
    },
    rustls::{
        crypto::{aws_lc_rs, CryptoProvider},
        pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
        server::{ClientHello, ResolvesServerCert},
        sign::CertifiedKey,
        ServerConfig,
    },
    tokio_tungstenite::tungstenite::{self, Message},
    Body, HttpContext, HttpHandler, Proxy, RequestOrResponse, WebSocketContext, WebSocketHandler,
};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    path::Path,
    sync::{
        atomic::{AtomicU16, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, info, warn};

type SharedFlow = Arc<Mutex<FlowProcessor>>;

struct PendingRequest {
    processor: SharedFlow,
    id: u16,
}

impl Drop for PendingRequest {
    fn drop(&mut self) {
        if let Ok(mut processor) = self.processor.try_lock() {
            processor.cancel_waiting_request(self.id);
        } else {
            let processor = self.processor.clone();
            let id = self.id;
            tokio::task::spawn_blocking(move || {
                if let Ok(mut processor) = processor.lock() {
                    processor.cancel_waiting_request(id);
                }
            });
        }
    }
}

struct FlowEntry {
    processor: SharedFlow,
    info: FlowInfo,
    outbound: Option<mpsc::UnboundedSender<Message>>,
    inbound: Option<mpsc::UnboundedSender<Message>>,
    activity: u64,
    business_activity: u64,
}

#[derive(Clone)]
struct FlowInfo {
    id: u64,
    peer_key: String,
    client: String,
    server: String,
}

#[derive(Clone)]
pub struct ProxyControl {
    flows: Arc<Mutex<HashMap<SocketAddr, FlowEntry>>>,
    next_id: Arc<AtomicU16>,
    activity: Arc<std::sync::atomic::AtomicU64>,
    config: Arc<RwLock<PipelineConfig>>,
    services: Arc<Services>,
}

impl ProxyControl {
    pub fn new(services: Arc<Services>, config: Arc<RwLock<PipelineConfig>>) -> Self {
        Self {
            flows: Arc::new(Mutex::new(HashMap::new())),
            next_id: Arc::new(AtomicU16::new(u16::MAX)),
            activity: Arc::new(std::sync::atomic::AtomicU64::new(1)),
            config,
            services,
        }
    }

    pub async fn request(
        &self,
        method: &str,
        data: &Value,
        timeout: Duration,
    ) -> Result<(u16, Value), String> {
        let config = self.config.read().await.clone();
        self.request_inner(method, data, timeout, false, config, None)
            .await
    }

    pub async fn replay(
        &self,
        method: &str,
        data: &Value,
        timeout: Duration,
        config: PipelineConfig,
    ) -> Result<(u16, Value), String> {
        self.request_inner(method, data, timeout, true, config, Some(REPLAY_INJECTOR))
            .await
    }

    pub async fn inject(
        &self,
        method: &str,
        data: &Value,
        timeout: Duration,
        config: PipelineConfig,
        source: &'static str,
    ) -> Result<(u16, Value), String> {
        self.request_inner(method, data, timeout, false, config, Some(source))
            .await
    }

    async fn request_inner(
        &self,
        method: &str,
        data: &Value,
        timeout: Duration,
        replay_id: bool,
        config: PipelineConfig,
        source: Option<&'static str>,
    ) -> Result<(u16, Value), String> {
        let (processor, outbound, inbound) = {
            let flows = self.flows.lock().map_err(|_| "flow map poisoned")?;
            self.preferred_flow(&flows)
                .map(|(_, flow)| {
                    (
                        flow.processor.clone(),
                        flow.outbound.as_ref().unwrap().clone(),
                        flow.inbound.clone(),
                    )
                })
                .ok_or("no-preferred-websocket-flow")?
        };
        let candidate = if replay_id {
            0
        } else {
            let start = self.next_id.fetch_sub(1, Ordering::Relaxed);
            if start == 0 {
                u16::MAX
            } else {
                start
            }
        };
        let (id, frame, receiver) = {
            let mut processor = processor.lock().map_err(|_| "flow processor poisoned")?;
            let (id, frame) = if replay_id {
                processor.build_replay_request(method, data)
            } else {
                processor.build_request_auto(candidate, method, data)
            }
            .map_err(|e| e.to_string())?;
            (id, frame, processor.response_waiter(id))
        };
        let _pending = PendingRequest {
            processor: processor.clone(),
            id,
        };
        let frames = {
            let pipeline_processor = processor.clone();
            let pipeline_frame = frame;
            let outcome = tokio::task::spawn_blocking(move || {
                let mut processor = pipeline_processor
                    .lock()
                    .map_err(|_| "flow processor poisoned")?;
                processor.set_config(config)?;
                match source {
                    Some(source) => processor.process_injected(&pipeline_frame, source),
                    None => processor.process(&pipeline_frame, Direction::Outbound),
                }
                .map_err(|error| error.to_string())
            })
            .await
            .unwrap_or_else(|error| Err(error.to_string()));
            let outcome = outcome?;
            match outcome {
                FrameOutcome::Forward(frame) => vec![(Direction::Outbound, frame)],
                FrameOutcome::Inject { frame, injected } => {
                    let mut frames = frame
                        .into_iter()
                        .map(|frame| (Direction::Outbound, frame))
                        .collect::<Vec<_>>();
                    frames.extend(
                        injected
                            .into_iter()
                            .map(|injected| (injected.direction, injected.frame)),
                    );
                    frames
                }
                FrameOutcome::Drop => {
                    return Err("packet-dropped-by-pipeline".into());
                }
            }
        };
        for (direction, frame) in frames {
            let sender = match direction {
                Direction::Outbound => Some(&outbound),
                Direction::Inbound => inbound.as_ref(),
            };
            if sender.is_none_or(|sender| sender.send(Message::Binary(frame.into())).is_err()) {
                return Err("websocket-flow-closed".into());
            }
        }
        match tokio::time::timeout(timeout, receiver).await {
            Ok(Ok(response)) => Ok((id, response)),
            Ok(Err(_)) => Err("response-waiter-closed".into()),
            Err(_) => Err("timeout".into()),
        }
    }

    fn preferred_flow<'a>(
        &self,
        flows: &'a HashMap<SocketAddr, FlowEntry>,
    ) -> Option<(&'a SocketAddr, &'a FlowEntry)> {
        let active = self.services.active_flow_id();
        flows
            .iter()
            .filter(|(_, flow)| {
                flow.outbound.is_some()
                    && flow.inbound.is_some()
                    && (active == 0 || active == flow.info.id)
            })
            .max_by_key(|(_, flow)| {
                (
                    flow.business_activity > 0,
                    flow.business_activity,
                    flow.activity,
                )
            })
    }

    pub fn flows(&self) -> Vec<FlowDumpItem> {
        let flows = self.flows.lock().unwrap();
        let preferred = self.preferred_flow(&flows).map(|(address, _)| *address);
        flows
            .iter()
            .map(|(address, flow)| FlowDumpItem {
                id: flow.info.id,
                peer_key: flow.info.peer_key.clone(),
                client: flow.info.client.clone(),
                server: flow.info.server.clone(),
                websocket: true,
                to_server: flow.outbound.is_some(),
                to_client: flow.inbound.is_some(),
                is_preferred: preferred == Some(*address),
                activity: flow.activity,
                business_activity: flow.business_activity,
            })
            .collect()
    }

    pub async fn request_with_retry(
        &self,
        method: &str,
        data: &Value,
        timeout: Duration,
    ) -> Result<(u16, Value), String> {
        let first = self.request(method, data, timeout).await?;
        let code = first
            .1
            .get("error")
            .and_then(|error| error.get("code"))
            .and_then(Value::as_i64);
        if code != Some(1004) || method == ".lq.Lobby.fetchAmuletActivityData" {
            return Ok(first);
        }
        let (_, refresh) = self
            .request(
                ".lq.Lobby.fetchAmuletActivityData",
                &json!({"activityId":260511}),
                timeout,
            )
            .await?;
        if refresh.get("error").is_some() {
            return Err(format!("resync-failed: {}", refresh["error"]));
        }
        self.request(method, data, timeout).await
    }
}

#[derive(Clone)]
struct Handler {
    config: Arc<RwLock<PipelineConfig>>,
    module_registry: Arc<ModuleRegistry>,
    services: Arc<Services>,
    control: ProxyControl,
}

impl Handler {
    async fn register(
        &self,
        client: SocketAddr,
        server: &str,
        direction: Direction,
        sender: mpsc::UnboundedSender<Message>,
    ) -> SharedFlow {
        let config = self.config.read().await.clone();
        let mut flows = self.control.flows.lock().expect("flow map poisoned");
        let entry = flows.entry(client).or_insert_with(|| {
            let processor = FlowProcessor::with_registry(
                config,
                self.services.clone(),
                self.module_registry.clone(),
            );
            let info = FlowInfo {
                id: processor.flow_id,
                peer_key: client.to_string(),
                client: client.to_string(),
                server: server.to_owned(),
            };
            FlowEntry {
                processor: Arc::new(Mutex::new(processor)),
                info,
                outbound: None,
                inbound: None,
                activity: 0,
                business_activity: 0,
            }
        });
        match direction {
            Direction::Outbound => entry.outbound = Some(sender),
            Direction::Inbound => entry.inbound = Some(sender),
        }
        entry.processor.clone()
    }
}

impl HttpHandler for Handler {
    async fn handle_request(
        &mut self,
        _context: &HttpContext,
        mut request: Request<Body>,
    ) -> RequestOrResponse {
        let uri_host_is_ip = request
            .uri()
            .host()
            .is_some_and(|host| host.trim_matches(['[', ']']).parse::<IpAddr>().is_ok());
        let host = request
            .headers()
            .get(hudsucker::hyper::header::HOST)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<Authority>().ok());
        if uri_host_is_ip
            && host.as_ref().is_some_and(|host| {
                host.host()
                    .trim_matches(['[', ']'])
                    .parse::<IpAddr>()
                    .is_err()
            })
        {
            let (mut parts, body) = request.into_parts();
            let mut uri = parts.uri.clone().into_parts();
            uri.authority = host;
            if let Ok(rewritten) = hudsucker::hyper::Uri::from_parts(uri) {
                parts.uri = rewritten;
            }
            request = Request::from_parts(parts, body);
        }
        request.into()
    }

    async fn should_intercept(&mut self, _context: &HttpContext, request: &Request<Body>) -> bool {
        let Some(authority) = request.uri().authority() else {
            return true;
        };
        let intercept = should_intercept_authority(authority);
        if !intercept {
            info!(target: "shanten_backend::hudsucker", %authority, "raw-tunneling CONNECT");
        }
        intercept
    }
}

fn should_intercept_authority(authority: &Authority) -> bool {
    authority.port_u16().unwrap_or(443) != 80
}

impl WebSocketHandler for Handler {
    async fn handle_websocket(
        self,
        context: WebSocketContext,
        mut stream: impl Stream<Item = Result<Message, tungstenite::Error>> + Unpin + Send + 'static,
        mut sink: impl Sink<Message, Error = tungstenite::Error> + Unpin + Send + 'static,
    ) {
        let (client, direction, uri, arrow) = match &context {
            WebSocketContext::ClientToServer { src, dst, .. } => {
                (*src, Direction::Outbound, dst.to_string(), '↑')
            }
            WebSocketContext::ServerToClient { src, dst, .. } => {
                (*dst, Direction::Inbound, src.to_string(), '↓')
            }
        };
        let (injected_tx, mut injected_rx) = mpsc::unbounded_channel();
        let flow = self.register(client, &uri, direction, injected_tx).await;
        let direction_name = match direction {
            Direction::Outbound => "outbound",
            Direction::Inbound => "inbound",
        };
        info!(target: "shanten_backend::hudsucker", %client, direction = direction_name, %uri, "WebSocket flow opened");
        loop {
            let message = tokio::select! {
                message = stream.next() => match message {
                    Some(Ok(message)) => message,
                    Some(Err(error)) => {
                        warn!(target: "shanten_backend::hudsucker", %client, direction = direction_name, %uri, %error, "WebSocket receive failed");
                        break;
                    }
                    None => break,
                },
                // Only frames already processed by request_inner or pipeline injection enter this queue.
                message = injected_rx.recv() => match message {
                    Some(message) => {
                        log_websocket_message(arrow, &uri, client, direction_name, &message, true);
                        if let Err(error) = sink.send(message).await {
                            warn!(target: "shanten_backend::hudsucker", %client, direction = direction_name, %uri, %error, "injected WebSocket send failed");
                            break;
                        }
                        continue
                    },
                    None => break,
                },
            };
            log_websocket_message(arrow, &uri, client, direction_name, &message, false);
            let Message::Binary(bytes) = message else {
                if let Err(error) = sink.send(message).await {
                    warn!(target: "shanten_backend::hudsucker", %client, direction = direction_name, %uri, %error, "WebSocket send failed");
                    break;
                }
                continue;
            };
            let config = self.config.read().await.clone();
            let process_flow = flow.clone();
            let original = bytes.to_vec();
            let fallback = original.clone();
            let outcome = tokio::task::spawn_blocking(move || {
                let mut processor = process_flow.lock().expect("flow processor poisoned");
                if processor.set_config(config).is_err() {
                    FrameOutcome::Forward(original)
                } else {
                    processor
                        .process(&original, direction)
                        .unwrap_or(FrameOutcome::Forward(original))
                }
            })
            .await
            .unwrap_or(FrameOutcome::Forward(fallback));
            if let Ok(mut flows) = self.control.flows.lock() {
                if let Some(entry) = flows.get_mut(&client) {
                    let sequence = self.control.activity.fetch_add(1, Ordering::Relaxed);
                    entry.activity = sequence;
                    let business_signal = flow
                        .lock()
                        .is_ok_and(|processor| processor.last_business_signal());
                    if business_signal {
                        entry.business_activity = sequence;
                    }
                }
            }
            let result = match outcome {
                FrameOutcome::Forward(frame) => sink.send(Message::Binary(frame.into())).await,
                FrameOutcome::Drop => continue,
                FrameOutcome::Inject { frame, injected } => {
                    if let Some(frame) = frame {
                        if sink.send(Message::Binary(frame.into())).await.is_err() {
                            break;
                        }
                    }
                    let mut result = Ok(());
                    for injected in injected {
                        if injected.direction == direction {
                            result = sink.send(Message::Binary(injected.frame.into())).await;
                            if result.is_err() {
                                break;
                            }
                            continue;
                        }
                        let target = self.control.flows.lock().ok().and_then(|flows| {
                            flows
                                .get(&client)
                                .and_then(|flow| match injected.direction {
                                    Direction::Outbound => flow.outbound.clone(),
                                    Direction::Inbound => flow.inbound.clone(),
                                })
                        });
                        if target.is_none_or(|sender| {
                            sender.send(Message::Binary(injected.frame.into())).is_err()
                        }) {
                            warn!(target: "shanten_backend::hudsucker", %client, direction = ?injected.direction, "injected WebSocket target unavailable");
                        }
                    }
                    result
                }
            };
            if let Err(error) = result {
                warn!(target: "shanten_backend::hudsucker", %client, direction = direction_name, %uri, %error, "WebSocket send failed");
                break;
            }
        }
        if let Ok(mut processor) = flow.lock() {
            processor.cancel_requests();
            self.services.disconnect_game_flow(processor.flow_id);
        }
        if let Ok(mut flows) = self.control.flows.lock() {
            if let Some(entry) = flows.get_mut(&client) {
                match direction {
                    Direction::Outbound => entry.outbound = None,
                    Direction::Inbound => entry.inbound = None,
                }
                if entry.outbound.is_none() && entry.inbound.is_none() {
                    flows.remove(&client);
                }
            }
        }
    }
}

fn log_websocket_message(
    arrow: char,
    uri: &str,
    client: SocketAddr,
    direction: &str,
    message: &Message,
    injected: bool,
) {
    match message {
        Message::Binary(_) | Message::Text(_) | Message::Ping(_) | Message::Pong(_) => {}
        Message::Close(_) => {
            debug!(target: "shanten_backend::hudsucker", %client, direction, uri, injected, "{arrow} {uri} close")
        }
        Message::Frame(_) => {
            warn!(target: "shanten_backend::hudsucker", %client, direction, uri, injected, "{arrow} {uri} unexpected raw frame")
        }
    }
}

struct IpAwareAuthority {
    issuer: Arc<Issuer<'static, KeyPair>>,
    private_key: PrivateKeyDer<'static>,
    cache: Mutex<HashMap<String, Arc<ServerConfig>>>,
    provider: Arc<CryptoProvider>,
}

impl IpAwareAuthority {
    fn new(issuer: Issuer<'static, KeyPair>, provider: CryptoProvider) -> Self {
        let issuer = Arc::new(issuer);
        let private_key =
            PrivateKeyDer::from(PrivatePkcs8KeyDer::from(issuer.key().serialize_der()));
        Self {
            issuer,
            private_key,
            cache: Mutex::new(HashMap::new()),
            provider: Arc::new(provider),
        }
    }
}

impl CertificateAuthority for IpAwareAuthority {
    async fn gen_server_config(&self, authority: &Authority) -> Arc<ServerConfig> {
        let host = authority.host().to_owned();
        if let Some(config) = self.cache.lock().unwrap().get(&host).cloned() {
            return config;
        }
        let mut config = ServerConfig::builder_with_provider(self.provider.clone())
            .with_safe_default_protocol_versions()
            .expect("failed to set TLS protocol versions")
            .with_no_client_auth()
            .with_cert_resolver(Arc::new(SniResolver {
                fallback: host.clone(),
                issuer: self.issuer.clone(),
                private_key: self.private_key.clone_key(),
                provider: self.provider.clone(),
                cache: Mutex::new(HashMap::new()),
            }));
        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        let config = Arc::new(config);
        self.cache.lock().unwrap().insert(host, config.clone());
        config
    }
}

struct SniResolver {
    fallback: String,
    issuer: Arc<Issuer<'static, KeyPair>>,
    private_key: PrivateKeyDer<'static>,
    provider: Arc<CryptoProvider>,
    cache: Mutex<HashMap<String, Arc<CertifiedKey>>>,
}

impl std::fmt::Debug for SniResolver {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SniResolver")
            .finish_non_exhaustive()
    }
}

impl ResolvesServerCert for SniResolver {
    fn resolve(&self, client_hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        let host = client_hello.server_name().unwrap_or(&self.fallback);
        if let Some(key) = self.cache.lock().unwrap().get(host).cloned() {
            return Some(key);
        }
        debug!(target: "shanten_backend::hudsucker", connect_host = %self.fallback, sni = %host, "issuing TLS leaf certificate");
        let certificate: CertificateDer<'static> = leaf_params(host)
            .signed_by(self.issuer.key(), self.issuer.as_ref())
            .ok()?
            .into();
        let key = Arc::new(
            CertifiedKey::from_der(
                vec![certificate],
                self.private_key.clone_key(),
                self.provider.as_ref(),
            )
            .ok()?,
        );
        self.cache
            .lock()
            .unwrap()
            .insert(host.to_owned(), key.clone());
        Some(key)
    }
}

fn leaf_params(host: &str) -> CertificateParams {
    let mut params = CertificateParams::default();
    params.not_before = date_time_ymd(2020, 1, 1);
    params.not_after = date_time_ymd(4096, 1, 1);
    let mut name = DistinguishedName::new();
    name.push(DnType::CommonName, host);
    params.distinguished_name = name;
    params.subject_alt_names.push(match host.parse::<IpAddr>() {
        Ok(ip) => SanType::IpAddress(ip),
        Err(_) => SanType::DnsName(Ia5String::try_from(host).expect("invalid DNS name")),
    });
    params
}

fn load_or_create_ca(directory: &Path) -> Result<IpAwareAuthority> {
    std::fs::create_dir_all(directory)?;
    let certificate_path = directory.join("shanten-lens-ca.cer");
    let key_path = directory.join("shanten-lens-ca.key");
    let (certificate, key) = if certificate_path.exists() && key_path.exists() {
        (
            std::fs::read_to_string(&certificate_path)?,
            std::fs::read_to_string(&key_path)?,
        )
    } else {
        let key = KeyPair::generate()?;
        let mut params = CertificateParams::default();
        let mut name = DistinguishedName::new();
        name.push(DnType::CommonName, "Shanten Lens 3.0 CA");
        name.push(DnType::OrganizationName, "Shanten Lens");
        params.distinguished_name = name;
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params.key_usages = vec![
            KeyUsagePurpose::DigitalSignature,
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::CrlSign,
        ];
        let certificate = params.self_signed(&key)?.pem();
        let key = key.serialize_pem();
        std::fs::write(&certificate_path, &certificate)?;
        std::fs::write(&key_path, &key)?;
        (certificate, key)
    };
    let key = KeyPair::from_pem(&key)?;
    let issuer = Issuer::from_ca_cert_pem(&certificate, key)?;
    Ok(IpAwareAuthority::new(issuer, aws_lc_rs::default_provider()))
}

pub async fn run(
    address: SocketAddr,
    data_root: &Path,
    config: Arc<RwLock<PipelineConfig>>,
    module_registry: Arc<ModuleRegistry>,
    services: Arc<Services>,
    control: ProxyControl,
) -> Result<()> {
    let ca = load_or_create_ca(&data_root.join("ca"))?;
    let backend = services.table("backend");
    let upstream = if backend
        .get("enable_upstream_proxy")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        backend
            .get("upstream_proxy")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(str::parse)
            .transpose()
            .context("invalid upstream proxy URI")?
    } else {
        None
    };
    let http_connector = crate::upstream::http_connector(upstream)?;
    let websocket_connector = crate::upstream::websocket_connector()?;
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .context("failed to listen for MITM connections")?;
    let status = services.clone();
    let handler = Handler {
        config,
        module_registry,
        services,
        control,
    };
    let proxy = Proxy::builder()
        .with_listener(listener)
        .with_ca(ca)
        .with_http_connector(http_connector)
        .with_http_handler(handler.clone())
        .with_websocket_handler(handler)
        .with_websocket_connector(websocket_connector)
        .build()
        .context("failed to build MITM proxy")?;
    status.set_proxy_status(true, None);
    proxy.start().await.context("MITM proxy stopped")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn tsumo_stop_restart_cancels_old_waiter_and_failures_back_off() {
        use crate::{
            automation::Automation,
            protocol::{LiqiCodec, MessageType},
        };
        let root = std::env::temp_dir().join(format!(
            "shanten-tsumo-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let (events, _) = tokio::sync::broadcast::channel(32);
        let services = Arc::new(Services::load(root.join("configs"), events).unwrap());
        struct Counter(Arc<std::sync::atomic::AtomicU64>);
        impl crate::pipeline::ExternalPacketModule for Counter {
            fn notify(&self, _: &crate::pipeline::Packet, _: u64) -> Result<(), String> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
            fn decide(
                &self,
                _: &crate::pipeline::Packet,
                _: u64,
                _: Duration,
            ) -> Result<crate::pipeline::ExternalDecision, String> {
                unreachable!("read-only module")
            }
        }
        let calls = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let registry = Arc::new(ModuleRegistry::default());
        let mut info = crate::pipeline::PacketModuleInfo::builtin(
            "audit_counter",
            vec![crate::pipeline::PacketSubscription {
                direction: Some(Direction::Outbound),
                packet_type: Some("Req".into()),
                method: None,
                operations: vec![crate::pipeline::PacketOperation::Read],
            }],
        );
        info.builtin = false;
        info.provider_id = "audit".into();
        registry
            .register_external(info, Arc::new(Counter(calls.clone())))
            .unwrap();
        let config = Arc::new(RwLock::new(PipelineConfig {
            schema: 1,
            modules: vec![crate::pipeline::ModuleConfig {
                id: "audit_counter".into(),
                enabled: true,
                options: Value::Null,
            }],
        }));
        let control = ProxyControl::new(services.clone(), config.clone());
        let handler = Handler {
            config,
            module_registry: registry,
            services: services.clone(),
            control: control.clone(),
        };
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let address = "127.0.0.1:12345".parse().unwrap();
        let flow = handler
            .register(address, "test", Direction::Outbound, sender.clone())
            .await;
        handler
            .register(address, "test", Direction::Inbound, sender)
            .await;
        let automation = Automation::new(services, control);
        async fn receive(receiver: &mut mpsc::UnboundedReceiver<Message>) -> u16 {
            let Message::Binary(frame) =
                tokio::time::timeout(Duration::from_secs(2), receiver.recv())
                    .await
                    .unwrap()
                    .unwrap()
            else {
                panic!("binary request expected")
            };
            let request = LiqiCodec::new().parse(&frame).unwrap();
            assert_eq!(
                request.method.as_ref(),
                ".lq.Lobby.amuletActivityGameOperate"
            );
            assert_eq!(request.data["type"], 8);
            request.id.unwrap()
        }
        let respond = |id, data: Value| {
            let frame = LiqiCodec::new()
                .build(
                    MessageType::Response,
                    Some(id),
                    ".lq.Lobby.amuletActivityGameOperate",
                    &data,
                )
                .unwrap();
            flow.lock().unwrap().process(&frame, Direction::Inbound)
        };
        automation.start_tsumo(10_000, true);
        automation.start_tsumo(10_000, true);
        let old = receive(&mut receiver).await;
        automation.stop_tsumo();
        automation.start_tsumo(10_000, true);
        let current = receive(&mut receiver).await;
        assert_ne!(old, current);
        let _ = respond(old, json!({}));
        assert_eq!(automation.tsumo_status().win_count, 0);
        respond(current, json!({})).unwrap();
        tokio::task::yield_now().await;
        assert_eq!(automation.tsumo_status().win_count, 1);
        automation.stop_tsumo();
        automation.start_tsumo(50, false);
        let failed = receive(&mut receiver).await;
        respond(failed, json!({"error":{"code":1}})).unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(250), receiver.recv())
                .await
                .is_err()
        );
        assert_eq!(automation.tsumo_status().win_count, 1);
        assert!(!automation.tsumo_status().last_reason.is_empty());
        automation.stop_tsumo();
        tokio::task::yield_now().await;
        assert!(!automation.tsumo_status().running);
        assert_eq!(
            calls.load(Ordering::SeqCst),
            3,
            "each application request must traverse the pipeline exactly once"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn intercepts_ip_literal_https_and_tunnels_plain_http() {
        fn intercept(authority: &str) -> bool {
            let authority: Authority = authority.parse().unwrap();
            should_intercept_authority(&authority)
        }

        assert!(!intercept("161.117.125.216:80"));
        assert!(intercept("23.62.231.48:443"));
        assert!(intercept("game.example.com:443"));
        assert!(intercept("23.62.231.48:8443"));
    }
}
