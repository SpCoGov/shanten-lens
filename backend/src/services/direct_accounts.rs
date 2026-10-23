use super::{event, Services};
use crate::{
    pipeline::{Direction, Packet},
    storage::write_json,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;
use std::{
    path::Path,
    time::{Duration, Instant},
};

#[derive(Clone, Deserialize, Serialize, Type)]
pub struct DirectAccountSummary {
    pub id: String,
    pub label: String,
    pub region: String,
    pub login_method: String,
    pub username: String,
    pub uid: String,
    pub oauth_type: u32,
    pub client_version_string: String,
    pub route: Option<String>,
    pub has_password: bool,
    pub has_code: bool,
    pub has_session: bool,
}

#[derive(Clone, Deserialize, Serialize, Type)]
pub struct DirectAccountInput {
    pub id: Option<String>,
    pub label: String,
    pub region: String,
    pub login_method: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub uid: String,
    #[serde(default)]
    pub oauth_type: u32,
    #[serde(default)]
    pub client_version_string: String,
    pub route: Option<String>,
    pub password: Option<String>,
    pub code: Option<String>,
    pub verification_code: Option<String>,
}

// Credentials are deliberately excluded from the IPC summary and Debug output.
#[derive(Clone, Deserialize, Serialize)]
pub struct StoredDirectAccount {
    pub id: String,
    pub label: String,
    pub region: String,
    pub login_method: String,
    pub username: String,
    pub uid: String,
    pub oauth_type: u32,
    pub client_version_string: String,
    pub route: Option<String>,
    pub password: String,
    pub code: String,
    pub device_id: String,
    #[serde(default)]
    pub yostar_token: String,
}

impl StoredDirectAccount {
    fn summary(&self) -> DirectAccountSummary {
        DirectAccountSummary {
            id: self.id.clone(),
            label: self.label.clone(),
            region: self.region.clone(),
            login_method: self.login_method.clone(),
            username: self.username.clone(),
            uid: self.uid.clone(),
            oauth_type: self.oauth_type,
            client_version_string: self.client_version_string.clone(),
            route: self.route.clone(),
            has_password: !self.password.is_empty(),
            has_code: !self.code.is_empty(),
            has_session: !self.yostar_token.is_empty(),
        }
    }

    fn validate(&self) -> Result<(), String> {
        let text =
            |value: &str, max: usize| value.len() <= max && !value.chars().any(char::is_control);
        if self.id.is_empty()
            || !text(&self.id, 128)
            || self.label.trim().is_empty()
            || !text(&self.label, 128)
            || !matches!(self.region.as_str(), "cn" | "jp" | "en" | "kr")
            || !matches!(self.login_method.as_str(), "password" | "oauth" | "email")
            || !text(&self.username, 256)
            || !text(&self.uid, 256)
            || !text(&self.password, 4096)
            || !text(&self.code, 4096)
            || !text(&self.yostar_token, 4096)
            || !text(&self.client_version_string, 256)
            || self.device_id.is_empty()
            || !text(&self.device_id, 128)
            || self.oauth_type == 0
            || self
                .route
                .as_ref()
                .is_some_and(|route| route.is_empty() || !text(route, 128))
        {
            return Err("direct.invalid_account".into());
        }
        if self.login_method == "password" {
            if self.region != "cn" {
                return Err("direct.password_cn_only".into());
            }
            if self.username.is_empty() || self.password.is_empty() {
                return Err("direct.credentials_required".into());
            }
        } else if self.login_method == "email" {
            if self.region == "cn" {
                return Err("direct.email_region_required".into());
            }
            crate::direct_connection::yostar::validate_email(&self.username)
                .map_err(|error| error.to_string())?;
            if !self.yostar_token.is_empty()
                && (self.uid.is_empty() || !self.uid.bytes().all(|byte| byte.is_ascii_digit()))
            {
                return Err("direct.invalid_uid".into());
            }
        } else if !self.uid.is_empty() && !self.uid.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err("direct.invalid_uid".into());
        }
        Ok(())
    }
}

struct Capture {
    account_id: String,
    expires: Instant,
}

#[derive(Default)]
pub(super) struct DirectAccounts {
    accounts: Vec<StoredDirectAccount>,
    capture: Option<Capture>,
}

impl DirectAccounts {
    pub(super) fn load(path: &Path) -> anyhow::Result<Self> {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default())
            }
            Err(error) => return Err(error.into()),
        };
        let accounts: Vec<StoredDirectAccount> = serde_json::from_slice(&bytes)?;
        let mut ids = std::collections::HashSet::new();
        for account in &accounts {
            account.validate().map_err(anyhow::Error::msg)?;
            anyhow::ensure!(ids.insert(&account.id), "duplicate direct account ID");
        }
        Ok(Self {
            accounts,
            capture: None,
        })
    }
}

pub(crate) fn new_id() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    hudsucker::rustls::crypto::aws_lc_rs::default_provider()
        .secure_random
        .fill(&mut bytes)
        .map_err(|_| "direct.random_failed")?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        u32::from_be_bytes(bytes[..4].try_into().unwrap()),
        u16::from_be_bytes(bytes[4..6].try_into().unwrap()),
        u16::from_be_bytes(bytes[6..8].try_into().unwrap()),
        u16::from_be_bytes(bytes[8..10].try_into().unwrap()),
        u64::from_be_bytes([
            0, 0, bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
        ])
    ))
}

impl Services {
    fn direct_accounts_path(&self) -> std::path::PathBuf {
        self.root
            .parent()
            .unwrap_or(&self.root)
            .join("data/direct-accounts.json")
    }

    pub fn direct_accounts(&self) -> Vec<DirectAccountSummary> {
        self.direct_accounts
            .lock()
            .unwrap()
            .accounts
            .iter()
            .map(StoredDirectAccount::summary)
            .collect()
    }

    pub fn direct_account(&self, id: &str) -> Result<StoredDirectAccount, String> {
        self.direct_accounts
            .lock()
            .unwrap()
            .accounts
            .iter()
            .find(|account| account.id == id)
            .cloned()
            .ok_or_else(|| "direct.account_not_found".into())
    }

    pub fn save_direct_account(
        &self,
        input: DirectAccountInput,
    ) -> Result<Vec<DirectAccountSummary>, String> {
        self.save_direct_account_with_session(input, None)
    }

    pub(crate) fn save_direct_account_with_session(
        &self,
        input: DirectAccountInput,
        session: Option<crate::direct_connection::yostar::Session>,
    ) -> Result<Vec<DirectAccountSummary>, String> {
        let mut store = self.direct_accounts.lock().unwrap();
        let previous = match input.id.as_deref() {
            Some(id) => Some(
                store
                    .accounts
                    .iter()
                    .find(|account| account.id == id)
                    .ok_or("direct.account_not_found")?,
            ),
            None => None,
        };
        let oauth_type = if input.oauth_type != 0 {
            input.oauth_type
        } else {
            previous
                .filter(|account| {
                    account.region == input.region && account.login_method == input.login_method
                })
                .map(|account| account.oauth_type)
                .unwrap_or(if matches!(input.region.as_str(), "cn" | "jp") {
                    21
                } else {
                    22
                })
        };
        let same_identity = previous.filter(|account| {
            account.region == input.region
                && account.login_method == input.login_method
                && if matches!(input.login_method.as_str(), "password" | "email") {
                    account.username == input.username.trim()
                } else {
                    account.uid == input.uid.trim() && account.oauth_type == oauth_type
                }
        });
        let password = input
            .password
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| {
                same_identity
                    .map(|account| account.password.clone())
                    .unwrap_or_default()
            });
        let code = input
            .code
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| {
                same_identity
                    .map(|account| account.code.clone())
                    .unwrap_or_default()
            });
        let mut account = StoredDirectAccount {
            id: previous
                .map(|account| Ok(account.id.clone()))
                .unwrap_or_else(new_id)?,
            device_id: previous
                .map(|account| Ok(account.device_id.clone()))
                .unwrap_or_else(new_id)?,
            label: input.label.trim().into(),
            region: input.region,
            login_method: input.login_method,
            username: input.username.trim().into(),
            uid: input.uid.trim().into(),
            oauth_type,
            client_version_string: input.client_version_string.trim().into(),
            route: input.route,
            password,
            code,
            yostar_token: same_identity
                .filter(|account| account.login_method == "email")
                .map(|account| account.yostar_token.clone())
                .unwrap_or_default(),
        };
        if let Some(session) = session {
            if account.login_method != "email" {
                return Err("direct.invalid_account".into());
            }
            account.uid = session.uid;
            account.yostar_token = session.token;
            account.device_id = session.device_id;
            account.oauth_type = if account.region == "jp" { 21 } else { 22 };
            account.client_version_string.clear();
        }
        if account.login_method == "password" {
            account.code.clear();
        } else {
            account.password.clear();
            if account.login_method == "email" {
                account.code.clear();
            }
        }
        account.validate()?;
        let mut accounts = store.accounts.clone();
        if let Some(existing) = accounts.iter_mut().find(|entry| entry.id == account.id) {
            *existing = account;
        } else {
            accounts.push(account);
        }
        write_json(&self.direct_accounts_path(), &accounts).map_err(|error| error.to_string())?;
        store.accounts = accounts;
        let summaries = store
            .accounts
            .iter()
            .map(StoredDirectAccount::summary)
            .collect();
        drop(store);
        let _ = self
            .events
            .send(event("direct_accounts_changed", Value::Null));
        Ok(summaries)
    }

    pub(crate) fn update_direct_email_session(
        &self,
        id: &str,
        session: &crate::direct_connection::yostar::Session,
    ) -> Result<(), String> {
        let mut store = self.direct_accounts.lock().unwrap();
        let mut accounts = store.accounts.clone();
        let account = accounts
            .iter_mut()
            .find(|account| account.id == id)
            .ok_or("direct.account_not_found")?;
        if account.login_method != "email"
            || account.uid != session.uid
            || account.device_id != session.device_id
        {
            return Err("direct.email_account_mismatch".into());
        }
        account.yostar_token = session.token.clone();
        account.validate()?;
        write_json(&self.direct_accounts_path(), &accounts).map_err(|error| error.to_string())?;
        store.accounts = accounts;
        Ok(())
    }

    pub fn delete_direct_account(&self, id: &str) -> Result<Vec<DirectAccountSummary>, String> {
        let mut store = self.direct_accounts.lock().unwrap();
        let mut accounts = store.accounts.clone();
        let length = accounts.len();
        accounts.retain(|account| account.id != id);
        if accounts.len() == length {
            return Err("direct.account_not_found".into());
        }
        write_json(&self.direct_accounts_path(), &accounts).map_err(|error| error.to_string())?;
        store.accounts = accounts;
        if store
            .capture
            .as_ref()
            .is_some_and(|capture| capture.account_id == id)
        {
            store.capture = None;
        }
        let summaries = store
            .accounts
            .iter()
            .map(StoredDirectAccount::summary)
            .collect();
        drop(store);
        let _ = self
            .events
            .send(event("direct_accounts_changed", Value::Null));
        Ok(summaries)
    }

    pub fn capture_direct_account(&self, id: Option<&str>) -> Result<Option<String>, String> {
        let mut store = self.direct_accounts.lock().unwrap();
        if let Some(id) = id {
            let account = store
                .accounts
                .iter()
                .find(|account| account.id == id)
                .ok_or("direct.account_not_found")?;
            if account.login_method != "oauth" {
                return Err("direct.capture_oauth_only".into());
            }
        }
        store.capture = id.map(|id| Capture {
            account_id: id.into(),
            expires: Instant::now() + Duration::from_secs(300),
        });
        drop(store);
        let _ = self
            .events
            .send(event("direct_accounts_changed", Value::Null));
        Ok(id.map(str::to_owned))
    }

    pub fn capture_account_id(&self) -> Option<String> {
        let mut store = self.direct_accounts.lock().unwrap();
        if store
            .capture
            .as_ref()
            .is_some_and(|capture| Instant::now() >= capture.expires)
        {
            store.capture = None;
        }
        store
            .capture
            .as_ref()
            .map(|capture| capture.account_id.clone())
    }

    pub fn capture_oauth_packet(
        &self,
        packet: &Packet,
        official_region: Option<&str>,
    ) -> Result<bool, String> {
        if packet.direction != Direction::Outbound
            || packet.packet_type != "Req"
            || packet.method != ".lq.Lobby.oauth2Auth"
        {
            return Ok(false);
        }
        let Some(region) = official_region else {
            return Ok(false);
        };
        let mut store = self.direct_accounts.lock().unwrap();
        let Some(capture) = store.capture.as_ref() else {
            return Ok(false);
        };
        if Instant::now() >= capture.expires {
            store.capture = None;
            return Ok(false);
        }
        let Some(index) = store.accounts.iter().position(|account| {
            account.id == capture.account_id
                && account.login_method == "oauth"
                && (account.region == region
                    || (region == "global" && matches!(account.region.as_str(), "en" | "kr")))
        }) else {
            return Ok(false);
        };
        let (Some(uid), Some(code), Some(kind), Some(version)) = (
            packet.data["uid"].as_str(),
            packet.data["code"].as_str(),
            packet.data["type"]
                .as_u64()
                .and_then(|kind| u32::try_from(kind).ok()),
            packet.data["clientVersionString"].as_str(),
        ) else {
            return Ok(false);
        };
        if uid.is_empty() || code.is_empty() || version.is_empty() {
            return Ok(false);
        }
        let mut accounts = store.accounts.clone();
        let account = &mut accounts[index];
        account.uid = uid.into();
        account.code = code.into();
        account.oauth_type = kind;
        account.client_version_string = version.into();
        account.validate()?;
        let account_id = account.id.clone();
        write_json(&self.direct_accounts_path(), &accounts).map_err(|error| error.to_string())?;
        store.accounts = accounts;
        store.capture = None;
        drop(store);
        let _ = self
            .events
            .send(event("direct_accounts_changed", Value::Null));
        let _ = self.events.send(event(
            "direct_account_captured",
            serde_json::json!({"account_id": account_id, "uid": uid}),
        ));
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn input() -> DirectAccountInput {
        DirectAccountInput {
            id: None,
            label: "Account".into(),
            region: "jp".into(),
            login_method: "oauth".into(),
            username: String::new(),
            uid: String::new(),
            oauth_type: 21,
            client_version_string: String::new(),
            route: None,
            password: None,
            code: None,
            verification_code: None,
        }
    }

    #[test]
    fn accounts_capture_only_when_armed_for_matching_region_and_never_expose_secrets() {
        let root = std::env::temp_dir().join(format!("lens-direct-{}", new_id().unwrap()));
        let (events, mut received) = tokio::sync::broadcast::channel(64);
        let services = Services::load(root.join("configs"), events.clone()).unwrap();
        let id = services.save_direct_account(input()).unwrap()[0].id.clone();
        let mut packet = Packet {
            direction: Direction::Outbound,
            packet_type: "Req".into(),
            method: ".lq.Lobby.oauth2Auth".into(),
            id: Some(1),
            data: json!({
                "uid":"12345", "code":"secret-code", "type":21, "clientVersionString":"StandaloneWindows64_2022-0.16.272"
            }),
        };
        assert!(!services.capture_oauth_packet(&packet, Some("jp")).unwrap());
        services.capture_direct_account(Some(&id)).unwrap();
        assert!(!services.capture_oauth_packet(&packet, None).unwrap());
        assert!(!services.capture_oauth_packet(&packet, Some("cn")).unwrap());
        packet.direction = Direction::Inbound;
        assert!(!services.capture_oauth_packet(&packet, Some("jp")).unwrap());
        packet.direction = Direction::Outbound;
        assert!(services.capture_oauth_packet(&packet, Some("jp")).unwrap());
        assert_eq!(services.capture_account_id(), None);
        assert_eq!(services.direct_account(&id).unwrap().code, "secret-code");
        services.capture_direct_account(Some(&id)).unwrap();
        packet.data["uid"] = json!("99999");
        packet.data["code"] = json!("replacement-secret-code");
        assert!(services.capture_oauth_packet(&packet, Some("jp")).unwrap());
        assert_eq!(services.direct_account(&id).unwrap().uid, "99999");
        assert_eq!(
            services.direct_account(&id).unwrap().code,
            "replacement-secret-code"
        );
        assert_eq!(services.capture_account_id(), None);
        services.capture_direct_account(None).unwrap();
        let summary = serde_json::to_value(services.direct_accounts()).unwrap();
        assert_eq!(summary[0]["has_code"], true);
        assert!(summary[0].get("code").is_none() && summary[0].get("password").is_none());
        while let Ok(event) = received.try_recv() {
            assert!(!event.to_string().contains("secret-code"));
        }
        let mut edit = input();
        edit.id = Some(id.clone());
        edit.uid = "99999".into();
        edit.code = Some(String::new());
        services.save_direct_account(edit.clone()).unwrap();
        assert_eq!(
            services.direct_account(&id).unwrap().code,
            "replacement-secret-code"
        );
        drop(services);
        let services = Services::load(root.join("configs"), events).unwrap();
        assert_eq!(
            services.direct_account(&id).unwrap().code,
            "replacement-secret-code"
        );
        edit.region = "en".into();
        services.save_direct_account(edit).unwrap();
        assert!(services.direct_account(&id).unwrap().code.is_empty());
        services.capture_direct_account(Some(&id)).unwrap();
        services
            .direct_accounts
            .lock()
            .unwrap()
            .capture
            .as_mut()
            .unwrap()
            .expires = Instant::now();
        assert!(!services.capture_oauth_packet(&packet, Some("en")).unwrap());
        assert_eq!(services.capture_account_id(), None);
        services.capture_direct_account(Some(&id)).unwrap();
        assert!(services
            .capture_oauth_packet(&packet, Some("global"))
            .unwrap());
        services.capture_direct_account(Some(&id)).unwrap();
        assert!(services.delete_direct_account(&id).unwrap().is_empty());
        assert_eq!(services.capture_account_id(), None);
        drop(services);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn capturing_replaces_only_the_armed_account_in_a_multi_account_store() {
        let root = std::env::temp_dir().join(format!("lens-multi-{}", new_id().unwrap()));
        let (events, _) = tokio::sync::broadcast::channel(64);
        let services = Services::load(root.join("configs"), events.clone()).unwrap();
        let first = services.save_direct_account(input()).unwrap()[0].id.clone();
        let mut second = input();
        second.label = "Second account".into();
        second.uid = "111".into();
        second.code = Some("old-secret".into());
        let rows = services.save_direct_account(second).unwrap();
        assert_eq!(rows.len(), 2);
        let second = rows.iter().find(|row| row.id != first).unwrap().id.clone();
        services.capture_direct_account(Some(&second)).unwrap();
        let packet = Packet {
            direction: Direction::Outbound,
            packet_type: "Req".into(),
            method: ".lq.Lobby.oauth2Auth".into(),
            id: Some(1),
            data: json!({"uid":"222", "code":"new-secret", "type":21, "clientVersionString":"WebGL_2022-1.2.3"}),
        };
        assert!(services.capture_oauth_packet(&packet, Some("jp")).unwrap());
        assert!(services.direct_account(&first).unwrap().uid.is_empty());
        assert_eq!(services.direct_account(&second).unwrap().code, "new-secret");
        drop(services);
        let services = Services::load(root.join("configs"), events).unwrap();
        assert_eq!(services.direct_accounts().len(), 2);
        assert_eq!(services.direct_account(&second).unwrap().uid, "222");
        assert_eq!(services.direct_account(&second).unwrap().code, "new-secret");
        assert_eq!(
            services.delete_direct_account(&second).unwrap()[0].id,
            first
        );
        drop(services);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn password_login_is_cn_only_and_requires_credentials() {
        let mut account = StoredDirectAccount {
            id: "id".into(),
            label: "name".into(),
            region: "jp".into(),
            login_method: "password".into(),
            username: "name@example.com".into(),
            uid: String::new(),
            oauth_type: 21,
            client_version_string: String::new(),
            route: None,
            password: "password".into(),
            code: String::new(),
            device_id: "device".into(),
            yostar_token: String::new(),
        };
        assert!(account.validate().is_err());
        account.region = "cn".into();
        assert!(account.validate().is_ok());
        account.password.clear();
        assert!(account.validate().is_err());
    }

    #[test]
    fn email_sessions_are_private_persist_and_clear_when_the_identity_changes() {
        use crate::direct_connection::yostar::Session;
        let root = std::env::temp_dir().join(format!("lens-email-{}", new_id().unwrap()));
        let (events, mut received) = tokio::sync::broadcast::channel(64);
        let services = Services::load(root.join("configs"), events.clone()).unwrap();
        let mut account = input();
        account.login_method = "email".into();
        account.username = "test@example.com".into();
        account.verification_code = Some("123456".into());
        let summaries = services
            .save_direct_account_with_session(
                account.clone(),
                Some(Session {
                    uid: "123".into(),
                    token: "private-yostar-token".into(),
                    device_id: "device".into(),
                }),
            )
            .unwrap();
        let id = &summaries[0].id;
        assert!(summaries[0].has_session);
        let public = serde_json::to_string(&summaries).unwrap();
        assert!(!public.contains("private-yostar-token") && !public.contains("123456"));
        while let Ok(event) = received.try_recv() {
            assert!(!event.to_string().contains("private-yostar-token"));
        }
        account.id = Some(id.clone());
        account.uid = "123".into();
        account.verification_code = None;
        services.save_direct_account(account.clone()).unwrap();
        services
            .update_direct_email_session(
                id,
                &Session {
                    uid: "123".into(),
                    token: "rotated-token".into(),
                    device_id: "device".into(),
                },
            )
            .unwrap();
        assert!(services
            .update_direct_email_session(
                id,
                &Session {
                    uid: "999".into(),
                    token: "wrong-token".into(),
                    device_id: "device".into()
                }
            )
            .is_err());
        drop(services);
        let services = Services::load(root.join("configs"), events).unwrap();
        assert_eq!(
            services.direct_account(id).unwrap().yostar_token,
            "rotated-token"
        );
        account.username = "other@example.com".into();
        services.save_direct_account(account).unwrap();
        assert!(services.direct_account(id).unwrap().yostar_token.is_empty());
        drop(services);
        std::fs::remove_dir_all(root).unwrap();
    }
}
