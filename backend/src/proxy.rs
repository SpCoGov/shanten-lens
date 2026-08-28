use crate::{
    ipc::FlowDumpItem,
    pipeline::{Direction, PipelineConfig},
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
    services: Arc<Services>,
}

impl ProxyControl {
    pub fn new(services: Arc<Services>) -> Self {
        Self {
            flows: Arc::new(Mutex::new(HashMap::new())),
            next_id: Arc::new(AtomicU16::new(u16::MAX)),
            activity: Arc::new(std::sync::atomic::AtomicU64::new(1)),
            services,
        }
    }

    pub async fn request(
        &self,
        method: &str,
        data: &Value,
        timeout: Duration,
    ) -> Result<(u16, Value), String> {
        self.request_inner(method, data, timeout, false, None).await
    }

    pub async fn replay(
        &self,
        method: &str,
        data: &Value,
        timeout: Duration,
        config: PipelineConfig,
    ) -> Result<(u16, Value), String> {
        self.request_inner(method, data, timeout, true, Some(config))
            .await
    }

    async fn request_inner(
        &self,
        method: &str,
        data: &Value,
        timeout: Duration,
        replay_id: bool,
        pipeline_config: Option<PipelineConfig>,
    ) -> Result<(u16, Value), String> {
        let (processor, sender) = {
            let flows = self.flows.lock().map_err(|_| "flow map poisoned")?;
            flows
                .values()
                .filter_map(|flow| {
                    flow.outbound.as_ref().map(|sender| {
                        (
                            flow.business_activity,
                            flow.activity,
                            flow.processor.clone(),
                            sender.clone(),
                        )
                    })
                })
                .max_by_key(|(business, activity, _, _)| (*business > 0, *business, *activity))
                .map(|(_, _, processor, sender)| (processor, sender))
                .ok_or("no-preferred-websocket-flow")?
        };
        let mut candidate = if replay_id {
            0
        } else {
            let start = self.next_id.fetch_sub(1, Ordering::Relaxed);
            if start == 0 {
                u16::MAX
            } else {
                start
            }
        };
        let (id, frame, receiver) = loop {
            let built = {
                let mut processor = processor.lock().map_err(|_| "flow processor poisoned")?;
                if replay_id {
                    processor.build_replay_request(method, data)
                } else {
                    processor.build_request_auto(candidate, method, data)
                }
                .map_err(|e| e.to_string())?
            };
            if let Some(receiver) = self.services.try_response_waiter(built.0.into()) {
                break (built.0, built.1, receiver);
            }
            processor
                .lock()
                .map_err(|_| "flow processor poisoned")?
                .cancel_request(built.0);
            if replay_id {
                return Err("replay-message-id-in-use".into());
            }
            let previous = built.0.wrapping_sub(1);
            candidate = if previous == 0 { u16::MAX } else { previous };
        };
        let frames = if let Some(config) = pipeline_config {
            let outcome: Result<FrameOutcome, String> = (|| {
                let mut processor = processor.lock().map_err(|_| "flow processor poisoned")?;
                processor.set_config(config)?;
                let outcome = processor
                    .process_replay(&frame)
                    .map_err(|error| error.to_string())?;
                Ok(outcome)
            })();
            let outcome = match outcome {
                Ok(outcome) => outcome,
                Err(error) => {
                    self.services.cancel_waiter(id.into());
                    if let Ok(mut processor) = processor.lock() {
                        processor.cancel_request(id);
                    }
                    return Err(error);
                }
            };
            match outcome {
                FrameOutcome::Forward(frame) => vec![frame],
                FrameOutcome::Inject {
                    frame,
                    mut injected,
                } => {
                    let mut frames = vec![frame];
                    frames.append(&mut injected);
                    frames
                }
                FrameOutcome::Drop => {
                    self.services.cancel_waiter(id.into());
                    return Err("packet-dropped-by-pipeline".into());
                }
            }
        } else {
            vec![frame]
        };
        for frame in frames {
            if sender.send(Message::Binary(frame.into())).is_err() {
                self.services.cancel_waiter(id.into());
                if let Ok(mut processor) = processor.lock() {
                    processor.cancel_request(id);
                }
                return Err("websocket-flow-closed".into());
            }
        }
        match tokio::time::timeout(timeout, receiver).await {
            Ok(Ok(response)) => Ok((id, response)),
            Ok(Err(_)) => Err("response-waiter-closed".into()),
            Err(_) => {
                self.services.cancel_waiter(id.into());
                if let Ok(mut processor) = processor.lock() {
                    processor.cancel_request(id);
                }
                Err("timeout".into())
            }
        }
    }

    pub fn flows(&self) -> Vec<FlowDumpItem> {
        let flows = self.flows.lock().unwrap();
        let preferred = flows
            .iter()
            .filter(|(_, flow)| flow.outbound.is_some())
            .max_by_key(|(_, flow)| {
                (
                    flow.business_activity > 0,
                    flow.business_activity,
                    flow.activity,
                )
            })
            .map(|(address, _)| *address);
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
        self.request(
            ".lq.Lobby.fetchAmuletActivityData",
            &json!({"activityId":260511}),
            timeout,
        )
        .await?;
        self.request(method, data, timeout).await
    }
}

#[derive(Clone)]
struct Handler {
    config: Arc<RwLock<PipelineConfig>>,
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
            let info = FlowInfo {
                id: self.control.activity.fetch_add(1, Ordering::Relaxed),
                peer_key: client.to_string(),
                client: client.to_string(),
                server: server.to_owned(),
            };
            let processor = FlowProcessor::with_services(config, self.services.clone());
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
            let outcome = {
                let mut processor = flow.lock().expect("flow processor poisoned");
                if processor.set_config(config).is_err() {
                    FrameOutcome::Forward(bytes.to_vec())
                } else {
                    processor
                        .process(&bytes, direction)
                        .unwrap_or_else(|_| FrameOutcome::Forward(bytes.to_vec()))
                }
            };
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
                    if sink.send(Message::Binary(frame.into())).await.is_err() {
                        break;
                    }
                    let mut result = Ok(());
                    for frame in injected {
                        result = sink.send(Message::Binary(frame.into())).await;
                        if result.is_err() {
                            break;
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
            processor.cancel_suppressed_requests();
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
    let handler = Handler {
        config,
        services,
        control,
    };
    Proxy::builder()
        .with_addr(address)
        .with_ca(ca)
        .with_http_connector(http_connector)
        .with_http_handler(handler.clone())
        .with_websocket_handler(handler)
        .with_websocket_connector(websocket_connector)
        .build()
        .context("failed to build MITM proxy")?
        .start()
        .await
        .context("MITM proxy stopped")
}

#[cfg(test)]
mod tests {
    use super::*;

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
