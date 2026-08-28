use lettre::{transport::smtp::authentication::Credentials, Message, SmtpTransport, Transport};
use serde_json::Value;

pub fn send(config: &Value, subject: &str, body: &str) -> Result<(), String> {
    let host = config
        .get("host")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let from = config
        .get("from")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let to = config
        .get("to")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if host.is_empty() || from.is_empty() || to.is_empty() {
        return Err("email-not-configured".into());
    }
    let message = Message::builder()
        .from(from.parse().map_err(|e| format!("invalid-from: {e}"))?)
        .to(to.parse().map_err(|e| format!("invalid-to: {e}"))?)
        .subject(subject)
        .body(body.to_owned())
        .map_err(|e| e.to_string())?;
    let ssl = config.get("ssl").and_then(Value::as_bool).unwrap_or(false);
    let port = config
        .get("port")
        .and_then(Value::as_u64)
        .unwrap_or(if ssl { 465 } else { 587 }) as u16;
    let mut builder = if ssl {
        SmtpTransport::relay(host)
    } else {
        SmtpTransport::starttls_relay(host)
    }
    .map_err(|e| e.to_string())?
    .port(port);
    let password = config.get("pass").and_then(Value::as_str).unwrap_or("");
    if !password.is_empty() {
        builder = builder.credentials(Credentials::new(from.into(), password.into()));
    }
    builder
        .build()
        .send(&message)
        .map(|_| ())
        .map_err(|e| e.to_string())
}
