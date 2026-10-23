use super::{fetch_document, http_client, Region};
use anyhow::{bail, ensure, Context, Result};
use md5::{Digest, Md5};
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};

// Adapted from majsoul-ws-login (Apache-2.0).
pub(crate) struct Session {
    pub uid: String,
    pub token: String,
    pub device_id: String,
}

pub(crate) fn validate_email(email: &str) -> Result<()> {
    ensure!(
        email.len() <= 254
            && !email.chars().any(|c| c.is_whitespace() || c.is_control())
            && email
                .split_once('@')
                .is_some_and(|(user, host)| !user.is_empty()
                    && !host.contains('@')
                    && host.contains('.')
                    && !host.starts_with('.')
                    && !host.ends_with('.')),
        "direct.invalid_email"
    );
    Ok(())
}

fn sign(head: &str, body: &str) -> String {
    let mut digest = Md5::new();
    digest.update(head.as_bytes());
    digest.update(body.as_bytes());
    digest.update(b"347467131a466f6865d7f2662e38841fbe2adb23");
    format!("{:X}", digest.finalize())
}

pub(crate) struct Yostar {
    http: reqwest::Client,
    #[cfg(test)]
    endpoint: Option<String>,
}

impl Yostar {
    pub fn new() -> Result<Self> {
        Ok(Self {
            http: http_client()?,
            #[cfg(test)]
            endpoint: None,
        })
    }

    async fn post(
        &self,
        region: Region,
        device_id: &str,
        path: &str,
        body: Value,
        session: Option<(&str, &str)>,
    ) -> Result<Value> {
        let (area, pid, base) = match region {
            Region::Jp => ("JP", "JP-MJ", "https://jp-sdk-api.yostarplat.com"),
            Region::En => ("US", "US-MAJONGSOUL", "https://en-sdk-api.yostarplat.com"),
            Region::Kr => ("KR", "KR-MAJONGSOUL", "https://jp-sdk-api.yostarplat.com"),
            Region::Cn => bail!("direct.email_region_required"),
        };
        let mut head = json!({"Region":area,"PID":pid,"Channel":"web","Platform":"pc",
            "Version":"4.16.2","Lang":region.tag(),"DeviceID":device_id,
            "Time":SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs()});
        if let Some((uid, token)) = session {
            head["UID"] = json!(uid);
            head["Token"] = json!(token);
        }
        let body = serde_json::to_string(&body)?;
        let signature = sign(&serde_json::to_string(&head)?, &body);
        let authorization = serde_json::to_string(&json!({"Head":head,"Sign":signature}))?;
        #[cfg(test)]
        let base = self.endpoint.as_deref().unwrap_or(base);
        let bytes = fetch_document(
            self.http
                .post(format!("{base}{path}"))
                .header("Authorization", authorization)
                .header("Content-Type", "application/json")
                .header("Accept", "application/json")
                .body(body),
            512 * 1024,
        )
        .await
        .context("direct.email_request_failed")?;
        let response: Value =
            serde_json::from_slice(&bytes).context("direct.email_response_invalid")?;
        let code = response["Code"]
            .as_i64()
            .context("direct.email_response_invalid")?;
        if code != 200 && path == "/user/quick-login" {
            bail!("direct.email_session_expired");
        }
        ensure!(code == 200, "Yostar {path}: {code}");
        Ok(response)
    }

    pub async fn send_code(&self, region: Region, device_id: &str, email: &str) -> Result<()> {
        validate_email(email)?;
        self.post(
            region,
            device_id,
            "/yostar/send-code",
            json!({"Account":email}),
            None,
        )
        .await?;
        Ok(())
    }

    pub async fn email_login(
        &self,
        region: Region,
        device_id: &str,
        email: &str,
        code: &str,
    ) -> Result<Session> {
        validate_email(email)?;
        ensure!(
            code.len() == 6 && code.bytes().all(|b| b.is_ascii_digit()),
            "direct.invalid_email_code"
        );
        let auth = self
            .post(
                region,
                device_id,
                "/yostar/get-auth",
                json!({"Account":email,"Code":code}),
                None,
            )
            .await?;
        let token = auth
            .pointer("/Data/Token")
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty())
            .context("direct.email_response_invalid")?;
        let result = self.post(region, device_id, "/user/login",
            json!({"Type":"yostar","OpenID":email,"Token":token,"Secret":"","CheckAccountPlus":0}), None).await?;
        user_info(&result, device_id)
    }

    pub async fn quick_login(&self, region: Region, session: &Session) -> Result<Session> {
        let result = self
            .post(
                region,
                &session.device_id,
                "/user/quick-login",
                json!({}),
                Some((&session.uid, &session.token)),
            )
            .await?;
        let next = user_info(&result, &session.device_id)?;
        ensure!(next.uid == session.uid, "direct.email_account_mismatch");
        Ok(next)
    }
}

fn user_info(value: &Value, device_id: &str) -> Result<Session> {
    let info = value
        .pointer("/Data/UserInfo")
        .context("direct.email_response_invalid")?;
    let uid = match &info["ID"] {
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        _ => bail!("direct.email_response_invalid"),
    };
    ensure!(
        !uid.is_empty() && uid.bytes().all(|b| b.is_ascii_digit()),
        "direct.email_response_invalid"
    );
    let token = info["Token"]
        .as_str()
        .filter(|v| !v.is_empty() && v.len() <= 4096)
        .context("direct.email_response_invalid")?;
    Ok(Session {
        uid,
        token: token.into(),
        device_id: device_id.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sdk_signature_and_input_validation_match_reference() {
        assert_eq!(
            sign(r#"{"Channel":"web","Time":123}"#, "{}"),
            "F42101B949BE96C40241AA807B3AFB24"
        );
        for email in ["", "no-at", "a@@example.com", "a@.com", "a b@example.com"] {
            assert!(validate_email(email).is_err());
        }
        assert!(validate_email("test@example.com").is_ok());
        assert!(user_info(
            &json!({"Data":{"UserInfo":{"ID":"abc","Token":"secret"}}}),
            "device"
        )
        .is_err());
        let session = user_info(
            &json!({"Data":{"UserInfo":{"ID":123,"Token":"secret"}}}),
            "device",
        )
        .unwrap();
        assert_eq!(session.uid, "123");
    }

    #[tokio::test]
    async fn email_verification_and_saved_sessions_use_the_same_device_in_each_region() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        for (region, expected_pid) in [
            (Region::Jp, "JP-MJ"),
            (Region::En, "US-MAJONGSOUL"),
            (Region::Kr, "KR-MAJONGSOUL"),
        ] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let endpoint = format!("http://{}", listener.local_addr().unwrap());
            let server = tokio::spawn(async move {
                for (index, path) in [
                    "/yostar/send-code",
                    "/yostar/get-auth",
                    "/user/login",
                    "/user/quick-login",
                    "/user/quick-login",
                    "/user/quick-login",
                ]
                .iter()
                .enumerate()
                {
                    let (mut socket, _) = listener.accept().await.unwrap();
                    let mut bytes = Vec::new();
                    let (headers, payload) = loop {
                        let mut chunk = [0; 1024];
                        let n = socket.read(&mut chunk).await.unwrap();
                        assert!(n > 0 && bytes.len() < 64 * 1024);
                        bytes.extend_from_slice(&chunk[..n]);
                        let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n")
                        else {
                            continue;
                        };
                        let headers = std::str::from_utf8(&bytes[..end]).unwrap();
                        let len: usize = headers
                            .lines()
                            .find_map(|line| {
                                line.split_once(':')
                                    .filter(|(key, _)| key.eq_ignore_ascii_case("content-length"))
                            })
                            .unwrap()
                            .1
                            .trim()
                            .parse()
                            .unwrap();
                        if bytes.len() < end + 4 + len {
                            continue;
                        }
                        break (
                            headers.to_owned(),
                            serde_json::from_slice::<Value>(&bytes[end + 4..end + 4 + len])
                                .unwrap(),
                        );
                    };
                    assert!(headers.starts_with(&format!("POST {path} ")));
                    let auth = headers
                        .lines()
                        .find_map(|line| {
                            line.split_once(':')
                                .filter(|(key, _)| key.eq_ignore_ascii_case("authorization"))
                        })
                        .unwrap()
                        .1
                        .trim();
                    let auth: Value = serde_json::from_str(auth).unwrap();
                    assert_eq!(auth["Head"]["DeviceID"], "device");
                    assert_eq!(auth["Head"]["PID"], expected_pid);
                    assert_eq!(
                        auth["Sign"],
                        sign(
                            &serde_json::to_string(&auth["Head"]).unwrap(),
                            &serde_json::to_string(&payload).unwrap()
                        )
                    );
                    if index >= 3 {
                        assert_eq!(auth["Head"]["UID"], "123");
                        assert_eq!(auth["Head"]["Token"], "session-token");
                    } else {
                        assert!(auth["Head"].get("Token").is_none());
                    }
                    let response = match index {
                        0 => { assert_eq!(payload["Account"], "test@example.com"); json!({"Code":200}) },
                        1 => { assert_eq!(payload["Code"], "123456"); json!({"Code":200,"Data":{"Token":"email-token"}}) },
                        2 => { assert_eq!(payload["Token"], "email-token"); json!({"Code":200,"Data":{"UserInfo":{"ID":123,"Token":"session-token"}}}) },
                        3 => json!({"Code":200,"Data":{"UserInfo":{"ID":"123","Token":"rotated-token"}}}),
                        4 => json!({"Code":200,"Data":{"UserInfo":{"ID":"999","Token":"private-mismatched-token"}}}),
                        _ => json!({"Code":403,"Data":{"Token":"private-error-token"}}),
                    }.to_string();
                    socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()).as_bytes()).await.unwrap();
                }
            });
            let sdk = Yostar {
                http: http_client().unwrap(),
                endpoint: Some(endpoint),
            };
            assert!(sdk
                .email_login(region, "device", "test@example.com", "bad")
                .await
                .is_err());
            sdk.send_code(region, "device", "test@example.com")
                .await
                .unwrap();
            let session = sdk
                .email_login(region, "device", "test@example.com", "123456")
                .await
                .unwrap();
            let next = sdk.quick_login(region, &session).await.unwrap();
            assert_eq!(next.token, "rotated-token");
            let mismatch = sdk
                .quick_login(region, &session)
                .await
                .err()
                .unwrap()
                .to_string();
            assert_eq!(mismatch, "direct.email_account_mismatch");
            let refusal = sdk
                .quick_login(region, &session)
                .await
                .err()
                .unwrap()
                .to_string();
            assert_eq!(refusal, "direct.email_session_expired");
            server.await.unwrap();
        }
    }
}
