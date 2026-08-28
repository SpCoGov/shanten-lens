use anyhow::{Context, Result};
use hudsucker::{
    hyper::{HeaderMap, Uri},
    hyper_util::client::legacy::connect::{proxy::Tunnel, Connect, HttpConnector},
    rustls::{crypto::aws_lc_rs, ClientConfig},
    tokio_tungstenite::Connector,
};
use hyper_rustls::ConfigBuilderExt;
use std::sync::Arc;

fn tls_config() -> Result<Arc<ClientConfig>> {
    let config = ClientConfig::builder_with_provider(Arc::new(aws_lc_rs::default_provider()))
        .with_safe_default_protocol_versions()?
        .with_webpki_roots()
        .with_no_client_auth();
    Ok(Arc::new(config))
}

pub fn http_connector(proxy: Option<Uri>) -> Result<impl Connect + Clone + Send + Sync + 'static> {
    let mut connector = HttpConnector::new();
    connector.enforce_http(false);
    let transport = match proxy {
        Some(uri) => {
            validate(&uri)?;
            EitherConnector::Tunnel(Tunnel::new(uri, connector).with_headers(HeaderMap::new()))
        }
        None => EitherConnector::Direct(connector),
    };
    Ok(hyper_rustls::HttpsConnectorBuilder::new()
        .with_tls_config((*tls_config()?).clone())
        .https_or_http()
        .enable_http1()
        .enable_http2()
        .wrap_connector(transport))
}

pub fn websocket_connector() -> Result<Connector> {
    Ok(Connector::Rustls(tls_config()?))
}

#[derive(Clone)]
enum EitherConnector {
    Direct(HttpConnector),
    Tunnel(Tunnel<HttpConnector>),
}

impl tower_service::Service<Uri> for EitherConnector {
    type Response = hudsucker::hyper_util::rt::TokioIo<tokio::net::TcpStream>;
    type Error = Box<dyn std::error::Error + Send + Sync>;
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>,
    >;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        match self {
            Self::Direct(value) => tower_service::Service::poll_ready(value, cx)
                .map_err(|error| Box::new(error) as Self::Error),
            Self::Tunnel(value) => tower_service::Service::poll_ready(value, cx)
                .map_err(|error| Box::new(error) as Self::Error),
        }
    }

    fn call(&mut self, dst: Uri) -> Self::Future {
        match self {
            Self::Direct(value) => {
                let future = tower_service::Service::call(value, dst);
                Box::pin(
                    async move { future.await.map_err(|error| Box::new(error) as Self::Error) },
                )
            }
            Self::Tunnel(value) => {
                let future = tower_service::Service::call(value, dst);
                Box::pin(
                    async move { future.await.map_err(|error| Box::new(error) as Self::Error) },
                )
            }
        }
    }
}

fn validate(uri: &Uri) -> Result<()> {
    if uri.scheme_str() != Some("http") {
        anyhow::bail!("upstream proxy must use http://host:port")
    }
    uri.host()
        .filter(|host| !host.is_empty())
        .context("upstream proxy must include a host")?;
    Ok(())
}
