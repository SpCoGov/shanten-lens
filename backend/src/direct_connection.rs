use crate::protocol::{LiqiCodec, MessageType};
use anyhow::{bail, ensure, Context, Result};
use hmac::{Hmac, Mac};
use hudsucker::{
    futures::{future::join_all, stream::FuturesUnordered, SinkExt, StreamExt},
    tokio_tungstenite::{
        connect_async_tls_with_config, tungstenite::Message, MaybeTlsStream, WebSocketStream,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::Sha256;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::{
    net::TcpStream,
    time::{timeout, Instant},
};

mod versions;
pub(crate) mod yostar;

pub type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
pub const HEARTBEAT_METHOD: &str = ".lq.Route.heartbeat";
pub const LOGIN_CONTRACT: &str = "DF2vkXCnfeXp4WoGrBGNcJBufZiMN3uP";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum Region {
    Cn,
    Jp,
    En,
    Kr,
}

impl Region {
    pub fn tag(self) -> &'static str {
        match self {
            Self::Cn => "cn",
            Self::Jp => "jp",
            Self::En => "en",
            Self::Kr => "kr",
        }
    }

    fn web_page(self) -> &'static str {
        match self {
            Self::Cn => "https://game.maj-soul.com/1/",
            Self::Jp => "https://game.mahjongsoul.com/",
            Self::En => "https://mahjongsoul.game.yo-star.com/",
            Self::Kr => "https://mahjongsoul.game.yo-star.com/kr/index.html",
        }
    }

    fn gateway_hosts(self) -> &'static [&'static str] {
        match self {
            Self::Cn => &[
                "route-5.maj-soul.com",
                "route-2.maj-soul.com",
                "route-3.maj-soul.com",
                "route-4.maj-soul.com",
                "route-6.maj-soul.com",
            ],
            Self::Jp => &["jpgs.mahjongsoul.com", "jpgsbk.mahjongsoul.com"],
            Self::En | Self::Kr => &["engs.mahjongsoul.com", "engsbk.mahjongsoul.com"],
        }
    }

    fn gateway_port(self, host: &str) -> u16 {
        if self == Self::Cn && host == "route-3.maj-soul.com" {
            8443
        } else {
            443
        }
    }

    fn valid_gateway(self, endpoint: &str) -> bool {
        let Ok(url) = reqwest::Url::parse(endpoint) else {
            return false;
        };
        url.scheme() == "wss"
            && url.username().is_empty()
            && url.password().is_none()
            && url.path() == "/gateway"
            && url.query().is_none()
            && url.fragment().is_none()
            && url.host_str().is_some_and(|host| {
                self.gateway_hosts().contains(&host)
                    && url.port_or_known_default() == Some(self.gateway_port(host))
            })
    }
}

// Credentials deliberately do not implement Debug or Serialize.
pub enum LoginCredentials {
    Password {
        account: String,
        password: String,
    },
    OAuth {
        uid: String,
        code: String,
        oauth_type: u32,
        client_version_string: String,
    },
}

pub struct DirectLoginOptions {
    pub region: Region,
    pub route_id: Option<String>,
    pub credentials: LoginCredentials,
    pub device_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
pub struct Route {
    pub id: String,
    pub endpoint: String,
    #[serde(default)]
    pub latency_ms: Option<u32>,
}

pub struct Connected {
    pub socket: Socket,
    pub route: Route,
    pub client_version_string: String,
    pub login: Value,
    pub notices: Vec<Vec<u8>>,
}

pub fn heartbeat_payload() -> Value {
    json!({"platform":1})
}

pub fn validate_response(method: &str, response: &Value) -> Result<()> {
    let code = response
        .pointer("/error/code")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    ensure!(code == 0, "{method}: server error {code}");
    Ok(())
}

fn http_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()?)
}

async fn fetch_document(request: reqwest::RequestBuilder, limit: usize) -> Result<Vec<u8>> {
    let mut response = request
        .header(reqwest::header::CACHE_CONTROL, "no-cache")
        .send()
        .await
        .context("official endpoint request failed")?;
    ensure!(
        response.status().is_success(),
        "official endpoint HTTP {}",
        response.status().as_u16()
    );
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            body.len().saturating_add(chunk.len()) <= limit,
            "official response too large"
        );
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

pub async fn discover_routes(region: Region) -> Result<Vec<Route>> {
    let http = http_client()?;
    let package = versions::package(&http, region).await?;
    let routes = fetch_routes(&http, region, &package, "Web").await?;
    Ok(measure_routes(routes).await)
}

async fn measure_routes(routes: Vec<Route>) -> Vec<Route> {
    join_all(routes.into_iter().map(|route| async {
        let (route, _connection) = probe_route(route, "Web").await;
        route
    }))
    .await
}

fn parse_routes(body: &Value, region: Region) -> Result<Vec<Route>> {
    let rows = body
        .pointer("/data/routes")
        .and_then(Value::as_array)
        .context("official routes missing")?;
    let mut routes: Vec<Route> = Vec::new();
    for row in rows {
        let Some(id) = row["id"].as_str().filter(|id| {
            !id.is_empty()
                && id.len() <= 64
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        }) else {
            continue;
        };
        let Some(domain) = row["domain"].as_str() else {
            continue;
        };
        let endpoint = format!("wss://{domain}/gateway");
        if row["ssl"] != true || !region.valid_gateway(&endpoint) {
            continue;
        }
        let endpoint = reqwest::Url::parse(&endpoint)?.to_string();
        if !routes
            .iter()
            .any(|route| route.id == id && route.endpoint == endpoint)
        {
            routes.push(Route {
                id: id.into(),
                endpoint,
                latency_ms: None,
            });
        }
    }
    ensure!(!routes.is_empty(), "no official secure routes available");
    Ok(routes)
}

async fn fetch_routes(
    http: &reqwest::Client,
    region: Region,
    package: &str,
    platform: &str,
) -> Result<Vec<Route>> {
    let mut queries: FuturesUnordered<_> = region
        .gateway_hosts()
        .iter()
        .map(|host| async move {
            let url = format!(
                "https://{host}:{}/api/clientgate/routes",
                region.gateway_port(host)
            );
            let body = fetch_document(
                http.get(url).query(&[
                    ("platform", platform),
                    ("version", package),
                    (
                        "lang",
                        if region == Region::Cn {
                            "chs_t"
                        } else {
                            region.tag()
                        },
                    ),
                ]),
                512 * 1024,
            )
            .await?;
            parse_routes(&serde_json::from_slice(&body)?, region)
        })
        .collect();
    while let Some(result) = queries.next().await {
        if let Ok(routes) = result {
            return Ok(routes);
        }
    }
    bail!("official route discovery failed")
}

struct Client {
    socket: Socket,
    codec: LiqiCodec,
    next_id: u16,
    notices: Vec<Vec<u8>>,
}

impl Client {
    async fn open(endpoint: &str) -> Result<Self> {
        let (socket, _) = timeout(
            REQUEST_TIMEOUT,
            connect_async_tls_with_config(
                endpoint,
                None,
                false,
                Some(crate::upstream::websocket_connector()?),
            ),
        )
        .await
        .context("websocket handshake timed out")?
        .context("websocket connection failed")?;
        Ok(Self {
            socket,
            codec: LiqiCodec::new(),
            next_id: 1,
            notices: Vec::new(),
        })
    }

    async fn rpc(&mut self, method: &str, payload: &Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id = id % u16::MAX + 1;
        let frame = self
            .codec
            .build(MessageType::Request, Some(id), method, payload)?;
        let result = timeout(REQUEST_TIMEOUT, async {
            self.socket
                .send(Message::Binary(frame.into()))
                .await
                .context("websocket send failed")?;
            loop {
                match self
                    .socket
                    .next()
                    .await
                    .context("websocket disconnected")?
                    .context("websocket receive failed")?
                {
                    Message::Binary(frame) => {
                        let packet = self.codec.parse(&frame).context("invalid login response")?;
                        if packet.message_type == MessageType::Response && packet.id == Some(id) {
                            validate_response(method, &packet.data)?;
                            return Ok(packet.data);
                        }
                        if packet.message_type == MessageType::Notify {
                            ensure!(
                                !matches!(
                                    packet.method.as_ref(),
                                    ".lq.NotifyAccountLogout"
                                        | ".lq.NotifyAnotherLogin"
                                        | ".lq.NotifyConnectionShutdown"
                                ),
                                "session ended during login"
                            );
                            ensure!(self.notices.len() < 128, "too many notices during login");
                            self.notices.push(frame.to_vec());
                        }
                    }
                    Message::Ping(_) => self.socket.flush().await?,
                    Message::Pong(_) => {}
                    Message::Close(_) => bail!("websocket closed during login"),
                    _ => bail!("unexpected websocket message during login"),
                }
            }
        })
        .await
        .with_context(|| format!("{method}: request timed out"))?;
        result
    }

    async fn request_connection(&mut self, route: &Route, platform: &str) -> Result<()> {
        let response = self
            .rpc(
                ".lq.Route.requestConnection",
                &json!({
                    "type":1, "routeId":route.id, "platform":platform,
                    "timestamp":SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
                }),
            )
            .await?;
        ensure!(
            response["result"] == 1,
            "official route rejected connection"
        );
        Ok(())
    }
}

async fn probe_route(mut route: Route, platform: &str) -> (Route, Result<Client>) {
    let started = Instant::now();
    let result = timeout(Duration::from_secs(8), async {
        let mut client = Client::open(&route.endpoint).await?;
        client.request_connection(&route, platform).await?;
        Ok(client)
    })
    .await
    .context("route probe timed out")
    .and_then(|result| result);
    route.latency_ms = result
        .as_ref()
        .ok()
        .map(|_| started.elapsed().as_millis().max(1) as u32);
    (route, result)
}

async fn select_route(
    routes: Vec<Route>,
    selected: Option<&str>,
    platform: &str,
) -> Result<(Route, Client)> {
    let routes: Vec<_> = routes
        .into_iter()
        .filter(|route| selected.is_none_or(|id| id == route.id))
        .collect();
    ensure!(
        !routes.is_empty(),
        "selected route is unavailable; refresh the route list"
    );
    // Probes only negotiate routes; credentials are sent once, to the winning connection.
    let probes = routes.into_iter().map(|route| probe_route(route, platform));
    let mut best: Option<(Route, Client)> = None;
    for (route, result) in join_all(probes).await {
        if let Ok(client) = result {
            if best
                .as_ref()
                .is_none_or(|(best_route, _)| route.latency_ms < best_route.latency_ms)
            {
                best = Some((route, client));
            }
        }
    }
    best.context("all selected routes are unreachable")
}

pub fn password_hash(password: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(b"lailai").expect("HMAC accepts any key length");
    mac.update(password.as_bytes());
    format!("{:x}", mac.finalize().into_bytes())
}

pub async fn connect(options: DirectLoginOptions) -> Result<Connected> {
    ensure!(
        !options.device_id.is_empty() && options.device_id.len() <= 128,
        "invalid login device ID"
    );
    let native = match &options.credentials {
        LoginCredentials::Password { account, password } => {
            ensure!(
                options.region == Region::Cn,
                "password login is only supported by the CN server"
            );
            ensure!(
                !account.trim().is_empty()
                    && account.len() <= 256
                    && !password.is_empty()
                    && password.len() <= 4096,
                "invalid password credentials"
            );
            false
        }
        LoginCredentials::OAuth {
            uid,
            code,
            oauth_type,
            client_version_string,
        } => {
            ensure!(
                !uid.is_empty()
                    && uid.len() <= 128
                    && uid.bytes().all(|b| b.is_ascii_digit())
                    && !code.is_empty()
                    && code.len() <= 4096
                    && *oauth_type > 0,
                "invalid OAuth credentials"
            );
            ensure!(
                client_version_string.is_empty()
                    || versions::valid_client_version(client_version_string),
                "invalid captured client version"
            );
            if client_version_string.is_empty() {
                *oauth_type == 21
            } else {
                client_version_string.starts_with("StandaloneWindows64_")
            }
        }
    };
    let http = http_client()?;
    let (package, web_resource) = versions::fetch(&http, options.region).await?;
    let client_version_string = {
        format!(
            "{}_2022-{web_resource}",
            if native {
                "StandaloneWindows64"
            } else {
                "WebGL"
            }
        )
    };
    let resource = client_version_string
        .split_once("_2022-")
        .context("invalid client version")?
        .1;
    let platform = if native { "Win" } else { "Web" };
    let routes = fetch_routes(&http, options.region, &package, platform).await?;
    let (route, mut client) = select_route(routes, options.route_id.as_deref(), platform).await?;
    let mut payload = json!({
        "device":{"platform":"pc","os":"windows","isBrowser":!native,"salePlatform":if native {"pc"} else {"web"},"screenWidth":911,"screenHeight":512,"screenType":1,"deviceId":options.device_id},
        "randomKey":options.device_id, "clientVersion":{"package":package,"resource":resource},
        "clientVersionString":client_version_string,"tag":options.region.tag(),
        "currencyPlatforms":if options.region == Region::Cn { vec![1,2,5,6,8,10,11] } else if native { vec![1,3,5,9,12] } else { vec![1,4,5,9,12] },
    });
    let login = match options.credentials {
        LoginCredentials::Password { account, password } => {
            payload["account"] = json!(account);
            payload["password"] = json!(password_hash(&password));
            payload["genAccessToken"] = json!(true);
            client.rpc(".lq.Lobby.login", &payload).await?
        }
        LoginCredentials::OAuth {
            uid,
            code,
            oauth_type,
            ..
        } => {
            let auth = client.rpc(".lq.Lobby.oauth2Auth", &json!({"type":oauth_type,"uid":uid,"code":code,"clientVersionString":client_version_string})).await?;
            let token = auth["accessToken"]
                .as_str()
                .filter(|token| !token.is_empty())
                .context("OAuth returned no access token")?;
            let check = client
                .rpc(
                    ".lq.Lobby.oauth2Check",
                    &json!({"type":oauth_type,"accessToken":token}),
                )
                .await?;
            ensure!(
                check["hasAccount"] == true,
                "create a Mahjong Soul account in the official client first"
            );
            payload["type"] = json!(oauth_type);
            payload["accessToken"] = json!(token);
            client.rpc(".lq.Lobby.oauth2Login", &payload).await?
        }
    };
    ensure!(
        login["accountId"].as_u64().unwrap_or(0) != 0 && login["account"].is_object(),
        "login returned incomplete account information"
    );
    client
        .rpc(".lq.Lobby.loginBeat", &json!({"contract":LOGIN_CONTRACT}))
        .await?;
    client.rpc(".lq.Lobby.loginSuccess", &json!({})).await?;
    Ok(Connected {
        socket: client.socket,
        route,
        client_version_string,
        login,
        notices: client.notices,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_reject_untrusted_hosts_ports_and_credentials() {
        let routes = parse_routes(
            &json!({"data":{"routes":[
                {"id":"route-3","domain":"route-3.maj-soul.com:8443","ssl":true},
                {"id":"wrong-port","domain":"route-3.maj-soul.com","ssl":true},
                {"id":"other-host","domain":"route-5.maj-soul.com.attacker.test","ssl":true},
                {"id":"insecure","domain":"route-5.maj-soul.com","ssl":false}
            ]}}),
            Region::Cn,
        )
        .unwrap();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].id, "route-3");
        assert!(!Region::Jp.valid_gateway("wss://user@jpgs.mahjongsoul.com/gateway"));
        assert!(!Region::Jp.valid_gateway("wss://jpgs.mahjongsoul.com/gateway?token=x"));
        assert!(!Region::Jp.valid_gateway("wss://engs.mahjongsoul.com/gateway"));
    }

    #[test]
    fn login_payloads_match_embedded_protocol() {
        assert_eq!(
            password_hash("password"),
            "9cf8f0f148916a48ce4384991b0f90422dc1c061dab59b54b37e04edf5de5adb"
        );
        let mut codec = LiqiCodec::new();
        for (method, payload) in [
            (
                ".lq.Route.requestConnection",
                json!({"type":1,"routeId":"jp-1","timestamp":1,"platform":"Win"}),
            ),
            (
                ".lq.Lobby.oauth2Auth",
                json!({"type":21,"uid":"123","code":"secret","clientVersionString":"StandaloneWindows64_2022-0.16.272"}),
            ),
            (
                ".lq.Lobby.login",
                json!({"account":"test","password":password_hash("password"),"genAccessToken":true,"device":{"isBrowser":true,"deviceId":"test"},"clientVersion":{"package":"4.0.1","resource":"0.16.1"}}),
            ),
            (HEARTBEAT_METHOD, heartbeat_payload()),
        ] {
            let frame = codec
                .build(MessageType::Request, Some(1), method, &payload)
                .unwrap();
            assert_eq!(codec.parse(&frame).unwrap().method.as_ref(), method);
            codec.cancel_pending(1);
        }
        assert!(validate_response("test", &json!({"error":{"code":0}})).is_ok());
        assert!(
            validate_response("test", &json!({"error":{"code":110,"message":"secret"}}))
                .unwrap_err()
                .to_string()
                .contains("110")
        );
        assert!(
            !validate_response("test", &json!({"error":{"code":110,"message":"secret"}}))
                .unwrap_err()
                .to_string()
                .contains("secret")
        );
    }

    #[tokio::test]
    async fn route_probes_never_send_credentials_and_manual_selection_is_honored() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("ws://{}/gateway", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut socket = hudsucker::tokio_tungstenite::accept_async(socket)
                .await
                .unwrap();
            let mut codec = LiqiCodec::new();
            let frame = socket.next().await.unwrap().unwrap().into_data();
            let request = codec.parse(&frame).unwrap();
            assert_eq!(request.method.as_ref(), ".lq.Route.requestConnection");
            assert_eq!(request.data["routeId"], "chosen");
            assert_eq!(request.data["platform"], "Win");
            let response = codec
                .build(
                    MessageType::Response,
                    request.id,
                    request.method.as_ref(),
                    &json!({"result":1}),
                )
                .unwrap();
            socket.send(Message::Binary(response.into())).await.unwrap();
            assert!(timeout(Duration::from_millis(100), socket.next())
                .await
                .is_err());
        });
        let (route, _client) = select_route(
            vec![
                Route {
                    id: "unused".into(),
                    endpoint: "ws://127.0.0.1:1/gateway".into(),
                    latency_ms: None,
                },
                Route {
                    id: "chosen".into(),
                    endpoint,
                    latency_ms: None,
                },
            ],
            Some("chosen"),
            "Win",
        )
        .await
        .unwrap();
        assert_eq!(route.id, "chosen");
        assert!(route.latency_ms.is_some_and(|ms| ms > 0));
        server.await.unwrap();
        assert!(select_route(Vec::new(), Some("missing"), "Web")
            .await
            .is_err());
    }

    #[tokio::test]
    async fn route_measurement_keeps_unreachable_routes_and_reports_negotiation_latency() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("ws://{}/gateway", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut socket = hudsucker::tokio_tungstenite::accept_async(socket)
                .await
                .unwrap();
            let mut codec = LiqiCodec::new();
            let frame = socket.next().await.unwrap().unwrap().into_data();
            let request = codec.parse(&frame).unwrap();
            assert_eq!(request.method.as_ref(), ".lq.Route.requestConnection");
            assert_eq!(request.data["platform"], "Web");
            tokio::time::sleep(Duration::from_millis(25)).await;
            let response = codec
                .build(
                    MessageType::Response,
                    request.id,
                    request.method.as_ref(),
                    &json!({"result":1}),
                )
                .unwrap();
            socket.send(Message::Binary(response.into())).await.unwrap();
        });
        let routes = measure_routes(vec![
            Route {
                id: "online".into(),
                endpoint,
                latency_ms: None,
            },
            Route {
                id: "offline".into(),
                endpoint: "ws://127.0.0.1:1/gateway".into(),
                latency_ms: Some(1),
            },
        ])
        .await;
        assert_eq!(routes.len(), 2);
        assert!(routes[0].latency_ms.is_some_and(|ms| ms >= 25));
        assert_eq!(routes[1].latency_ms, None);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn rpc_rejects_forced_logout_while_waiting_for_authentication() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("ws://{}/gateway", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut socket = hudsucker::tokio_tungstenite::accept_async(socket)
                .await
                .unwrap();
            let _ = socket.next().await.unwrap().unwrap();
            let notice = LiqiCodec::new()
                .build(
                    MessageType::Notify,
                    None,
                    ".lq.NotifyAnotherLogin",
                    &json!({}),
                )
                .unwrap();
            socket.send(Message::Binary(notice.into())).await.unwrap();
        });
        let mut client = Client::open(&endpoint).await.unwrap();
        let error = client
            .rpc(
                ".lq.Lobby.oauth2Auth",
                &json!({"type":21,"uid":"123","code":"fake-private-code"}),
            )
            .await
            .unwrap_err();
        assert_eq!(error.to_string(), "session ended during login");
        assert!(!error.to_string().contains("fake-private-code"));
        server.await.unwrap();
    }
}
