use super::{log_dir, open_directory, qyzz, BackendRuntime};
use regex::Regex;
use serde_json::{json, Value};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::OnceLock,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use zip::{write::SimpleFileOptions, ZipWriter};

const FILE_LIMIT: usize = 2 * 1024 * 1024;
const JSON_LIMIT: usize = 4 * 1024 * 1024;
const TOTAL_LIMIT: usize = 20 * 1024 * 1024;
const HIDDEN: &str = "[redacted]";

impl BackendRuntime {
    pub fn record_frontend_error(&self, mut error: Value) -> Result<(), String> {
        if error.to_string().len() > 64 * 1024 {
            return Err("frontend error too large".into());
        }
        Redactor::new(&self.diagnostic_config()).sanitize(&mut error);
        tracing::error!(target: "shanten_lens::frontend", report = %error, "Frontend error");
        Ok(())
    }

    fn diagnostic_config(&self) -> Value {
        let mut config = self.state.services.config_payload();
        config["fuse"] = self.state.services.table("fuse");
        config["autorun"] = self.state.services.table("autorun");
        config
    }

    pub async fn export_diagnostics(
        &self,
        include_game_data: bool,
        frontend: Value,
    ) -> Result<String, String> {
        if frontend.to_string().len() > JSON_LIMIT {
            return Err("frontend diagnostics too large".into());
        }
        let config = self.diagnostic_config();
        let mut redactor = Redactor::new(&config);
        // Also remove saved credentials if an error happens to repeat their literal values.
        for account in self.state.services.direct_accounts() {
            if let Ok(account) = self.state.services.direct_account(&account.id) {
                redactor
                    .collect(&serde_json::to_value(account).map_err(|error| error.to_string())?);
            }
        }
        let game = self.state.services.game_state();
        let status = self.state.automation.autorun_status();
        let packets: Vec<_> = self
            .state
            .services
            .packet_log_snapshot()
            .packets
            .into_iter()
            .map(|packet| packet_diagnostic(packet, include_game_data))
            .collect();
        let plugins: Vec<_> = self.plugins().into_iter().map(|plugin| json!({
            "id":plugin.id,"name":plugin.name,"version":plugin.version,"api_version":plugin.api_version,
            "enabled":plugin.enabled,"running":plugin.running,"last_error":plugin.last_error,
            "requested_permissions":plugin.requested_packet_permissions,"approved_permissions":plugin.approved_packet_permissions,
        })).collect();
        let direct =
            serde_json::to_value(self.direct_status()).map_err(|error| error.to_string())?;
        let mut data = json!({
            "environment": {"backend_version":crate::VERSION,"os":std::env::consts::OS,
                "arch":std::env::consts::ARCH,"debug_build":cfg!(debug_assertions)},
            "config":config,"pipeline":self.state.pipeline.read().await.clone(),"plugins":plugins,
            "connections":{"proxy":self.state.services.proxy_status(),"mitm_port":self.mitm_port,
                "data_sources":self.data_sources(),"qyzz":qyzz::diagnostic_status(),
                "direct":pick(&direct, &["state","route","last_heartbeat_ms","error"]),
                "flows":self.state.proxy.flows(),"packet_recording":self.packet_recording_status()},
            "automation":pick(&status, &["active_profile","running","runs","elapsed_ms","current_step",
                "last_error","started_at","email_status","email_error","game_ready","has_live_game"]),
            "game_summary":pick(&game, &["source","stage","level","node","activity","revision","session_id",
                "qyzz_connected","qyzz_version","qyzz_run_id","qyzz_deal_id"]),
            "frontend":frontend,"packets":packets,"backend_logs":self.state.logs.snapshot(),
        });
        if include_game_data {
            data["game_state"] = game;
            data["automation_detail"] = status;
        }
        redactor.collect(&data);
        let logs = log_dir(&self.runtime_root);
        let directory = self
            .runtime_root
            .parent()
            .unwrap_or(&self.runtime_root)
            .join("diagnostics");
        let port = self.mitm_port;
        let path = tokio::task::spawn_blocking(move || {
            data["connections"]["system"] = system_diagnostics(port);
            write_archive(&directory, &logs, data, redactor, include_game_data)
        })
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())?;
        if let Some(parent) = path.parent() {
            let _ = open_directory(parent);
        }
        Ok(path.to_string_lossy().into_owned())
    }
}

fn packet_diagnostic(packet: crate::ipc::PacketLogItem, include_game_data: bool) -> Value {
    let error = packet.data.get("error").cloned();
    let mut value = json!({"ts_ms":packet.ts_ms,"direction":packet.direction,"type":packet.packet_type,
        "method":packet.method,"id":packet.id,"bytes":packet.data.to_string().len(),"error":error});
    if include_game_data {
        value["data"] = packet.data;
    }
    value
}

fn pick(value: &Value, keys: &[&str]) -> Value {
    Value::Object(
        keys.iter()
            .filter_map(|key| value.get(*key).map(|value| ((*key).into(), value.clone())))
            .collect(),
    )
}

fn sensitive_key(key: &str) -> bool {
    let key: String = key
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    [
        "password",
        "passwd",
        "token",
        "secret",
        "credential",
        "cookie",
        "authorization",
        "privatekey",
        "apikey",
        "signingkey",
    ]
    .iter()
    .any(|part| key.contains(part))
        || matches!(
            key.as_str(),
            "pass"
                | "code"
                | "email"
                | "from"
                | "to"
                | "username"
                | "accountid"
                | "userid"
                | "deviceid"
                | "uid"
                | "nickname"
        )
}

struct Redactor {
    secrets: Vec<String>,
}

impl Redactor {
    fn new(config: &Value) -> Self {
        let mut value = Self {
            secrets: Vec::new(),
        };
        value.collect(config);
        value
    }

    fn collect(&mut self, value: &Value) {
        match value {
            Value::Object(values) => {
                for (key, value) in values {
                    // Numeric gameplay UIDs are needed to reproduce inventory operations.
                    if sensitive_key(key) {
                        if let Some(secret) = value
                            .as_str()
                            .filter(|secret| !secret.is_empty() && !secret.starts_with('['))
                        {
                            if !self.secrets.iter().any(|existing| existing == secret) {
                                self.secrets.push(secret.into());
                            }
                        } else {
                            self.collect(value);
                        }
                    } else {
                        self.collect(value);
                    }
                }
            }
            Value::Array(values) => {
                for value in values {
                    self.collect(value);
                }
            }
            _ => (),
        }
        self.secrets
            .sort_by_key(|secret| std::cmp::Reverse(secret.len()));
    }

    fn text(&self, text: &str) -> String {
        static RULES: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
        let rules = RULES.get_or_init(|| vec![
            (Regex::new(r"(?s)-----BEGIN [^-]*PRIVATE KEY-----.*?-----END [^-]*PRIVATE KEY-----").unwrap(), "[redacted private key]"),
            (Regex::new(r"(?i)\b(?:Bearer|Basic)\s+[a-z0-9._~+/-]+=*").unwrap(), "[redacted authorization]"),
            (Regex::new(r#"(?i)((?:[a-z]:[\\/]+Users[\\/]+|/Users/|/home/))[^\\/\s"']+"#).unwrap(), "${1}[user]"),
            (Regex::new(r"(?i)[a-z0-9.!#$%&'*+/=?^_`{|}~-]+@[a-z0-9.-]+\.[a-z]{2,}").unwrap(), "[redacted email]"),
            (Regex::new(r#"(?i)(\b(?:[a-z_-]*(?:password|passwd|token|secret|credential|cookie|authorization|private_?key|api_?key)[a-z_-]*|pass|device_id|email|username|account_id|user_id|code)\b[\\"'\s]*[:=]\s*)(?:"(?:\\.|[^"\\])*"|'[^']*'|[^\s,;&}\]\r\n]+)"#).unwrap(), "${1}[redacted]"),
        ]);
        let mut text = text.to_owned();
        for secret in &self.secrets {
            text = text.replace(secret, HIDDEN);
        }
        static URL: OnceLock<Regex> = OnceLock::new();
        text = URL
            .get_or_init(|| {
                Regex::new(r#"(?i)\b(?:https?|wss?|socks5h?)://[^\s"'<>\\)]+"#).unwrap()
            })
            .replace_all(&text, |capture: &regex::Captures<'_>| {
                reqwest::Url::parse(&capture[0])
                    .map(|mut url| {
                        let _ = url.set_username("");
                        let _ = url.set_password(None);
                        url.set_query(None);
                        url.set_fragment(None);
                        url.set_path("");
                        url.to_string()
                    })
                    .unwrap_or_else(|_| "[redacted url]".into())
            })
            .into_owned();
        for (pattern, replacement) in rules {
            text = pattern.replace_all(&text, *replacement).into_owned();
        }
        text
    }

    fn sanitize(&self, value: &mut Value) {
        if let Some(method) = value.get("method").and_then(Value::as_str) {
            let method = method.to_ascii_lowercase();
            if ["login", "auth", "signup", "emailcode"]
                .iter()
                .any(|part| method.contains(part))
            {
                if let Some(data) = value.get_mut("data") {
                    *data = json!(HIDDEN);
                }
            }
        }
        match value {
            Value::Object(values) => {
                let item_uid = values.contains_key("id")
                    && (values.contains_key("store") || values.contains_key("volume"));
                for (key, value) in values {
                    if sensitive_key(key)
                        && !(value.is_number() && (key == "code" || (key == "uid" && item_uid)))
                    {
                        *value = json!(HIDDEN);
                    } else if matches!(
                        key.to_ascii_lowercase().as_str(),
                        "content" | "headers" | "trailers" | "body" | "raw" | "binary" | "payload"
                    ) {
                        *value = json!("[omitted raw content]");
                    } else {
                        self.sanitize(value);
                    }
                }
            }
            Value::Array(values) => {
                for value in values {
                    self.sanitize(value);
                }
            }
            Value::String(text) => {
                if let Ok(mut nested @ (Value::Object(_) | Value::Array(_))) =
                    serde_json::from_str(text)
                {
                    self.sanitize(&mut nested);
                    *text = nested.to_string();
                } else {
                    *text = self.text(text);
                }
            }
            _ => (),
        }
    }
}

struct Bundle {
    zip: ZipWriter<File>,
    redactor: Redactor,
    entries: Vec<Value>,
    total_bytes: usize,
}

impl Bundle {
    fn json(&mut self, name: &str, mut value: Value) -> anyhow::Result<()> {
        self.redactor.sanitize(&mut value);
        let bytes = serde_json::to_vec_pretty(&value)?;
        self.entry(
            name,
            &bytes,
            bytes.len() > JSON_LIMIT,
            "omitted: JSON size limit",
        )
    }

    fn jsonl(&mut self, name: &str, values: Vec<Value>, mut omitted: usize) -> anyhow::Result<()> {
        let mut bytes = Vec::new();
        let mut total = 0;
        // Keep the newest entries; a large packet must not hide the following errors.
        for mut value in values.into_iter().rev() {
            self.redactor.sanitize(&mut value);
            let line = serde_json::to_vec(&value)?;
            if total + line.len() + 1 > FILE_LIMIT {
                omitted += 1;
                continue;
            }
            total += line.len() + 1;
            bytes.push(line);
        }
        let bytes: Vec<_> = bytes
            .into_iter()
            .rev()
            .flat_map(|mut line| {
                line.push(b'\n');
                line
            })
            .collect();
        self.entry(name, &bytes, false, "")?;
        self.entries.last_mut().unwrap()["omitted_entries"] = json!(omitted);
        Ok(())
    }

    fn entry(
        &mut self,
        name: &str,
        bytes: &[u8],
        omitted: bool,
        reason: &str,
    ) -> anyhow::Result<()> {
        let total_limit =
            name != "manifest.json" && self.total_bytes + bytes.len() > TOTAL_LIMIT - 64 * 1024;
        if omitted || total_limit {
            let reason = if total_limit {
                "omitted: total size limit"
            } else {
                reason
            };
            self.entries.push(json!({"file":name,"status":reason}));
            return Ok(());
        }
        self.zip.start_file(
            name,
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
        )?;
        self.zip.write_all(bytes)?;
        self.total_bytes += bytes.len();
        self.entries
            .push(json!({"file":name,"bytes":bytes.len(),"status":"included"}));
        Ok(())
    }

    fn log(&mut self, path: &Path, name: &str, json_lines: bool) -> anyhow::Result<()> {
        let (bytes, truncated) = match read_tail(path) {
            Ok(value) => value,
            Err(error) => {
                self.entries.push(json!({"file":name,"status":"unavailable","error":self.redactor.text(&error.to_string())}));
                return Ok(());
            }
        };
        if json_lines {
            let mut skipped = 0;
            let values = bytes
                .split(|byte| *byte == b'\n')
                .filter(|line| !line.is_empty())
                .filter_map(|line| match serde_json::from_slice::<Value>(line) {
                    Ok(value) => Some(value),
                    Err(_) => {
                        skipped += 1;
                        None
                    }
                })
                .collect();
            self.jsonl(name, values, skipped)?;
        } else {
            let text = self.redactor.text(&String::from_utf8_lossy(&bytes));
            self.entry(name, text.as_bytes(), false, "")?;
        }
        self.entries.last_mut().unwrap()["tail_truncated"] = json!(truncated);
        Ok(())
    }
}

fn read_tail(path: &Path) -> std::io::Result<(Vec<u8>, bool)> {
    if !fs::symlink_metadata(path)?.file_type().is_file() {
        return Err(std::io::Error::other(
            "diagnostic log must be a regular file",
        ));
    }
    let mut file = File::open(path)?;
    let size = file.metadata()?.len();
    let offset = size.saturating_sub(FILE_LIMIT as u64);
    file.seek(SeekFrom::Start(offset))?;
    let mut bytes = Vec::new();
    file.take(FILE_LIMIT as u64).read_to_end(&mut bytes)?;
    if offset > 0 {
        // The first partial JSON line or stack frame cannot be reconstructed.
        let start = bytes
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(bytes.len(), |index| index + 1);
        bytes.drain(..start);
    }
    Ok((bytes, offset > 0))
}

fn write_archive(
    directory: &Path,
    logs: &Path,
    mut data: Value,
    redactor: Redactor,
    include_game_data: bool,
) -> anyhow::Result<PathBuf> {
    fs::create_dir_all(directory)?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?;
    let path = directory.join(format!("shanten-lens-diagnostics-{}.zip", now.as_nanos()));
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    let result = (|| -> anyhow::Result<()> {
        let mut bundle = Bundle {
            zip: ZipWriter::new(file),
            redactor,
            entries: Vec::new(),
            total_bytes: 0,
        };
        for key in [
            "environment",
            "config",
            "pipeline",
            "plugins",
            "connections",
            "automation",
            "game_summary",
            "frontend",
            "game_state",
            "automation_detail",
        ] {
            if let Some(value) = data.as_object_mut().unwrap().remove(key) {
                bundle.json(&format!("{key}.json"), value)?;
            }
        }
        for (key, name) in [
            ("backend_logs", "logs/backend-recent.jsonl"),
            ("packets", "packets/recent.jsonl"),
        ] {
            let values = data[key]
                .as_array_mut()
                .map(std::mem::take)
                .unwrap_or_default();
            bundle.jsonl(name, values, 0)?;
        }
        let mut files = Vec::new();
        match fs::read_dir(logs) {
            Ok(entries) => for entry in entries {
                let entry = entry?;
                let name = entry.file_name().to_string_lossy().into_owned();
                if let Some(timestamp) = name.strip_prefix("shanten-lens-").and_then(|name| name.strip_suffix(".log")).and_then(|name| name.parse::<u64>().ok()) {
                    if entry.file_type()?.is_file() { files.push((timestamp, entry.path(), name)); }
                }
            },
            Err(error) => bundle.entries.push(json!({"file":"logs/","status":"unavailable","error":bundle.redactor.text(&error.to_string())})),
        }
        files.sort_by_key(|(timestamp, _, _)| std::cmp::Reverse(*timestamp));
        for (_, path, name) in files.into_iter().take(2) {
            bundle.log(&path, &format!("logs/{name}"), true)?;
        }
        bundle.log(&logs.join("panic.log"), "logs/panic.log", false)?;
        let manifest = json!({"format":"shanten-lens.diagnostics","version":1,"exported_at_unix_ms":now.as_millis(),
            "include_game_data":include_game_data,"entries":bundle.entries,
            "limits":{"log_tail_bytes":FILE_LIMIT,"json_bytes":JSON_LIMIT,"total_bytes":TOTAL_LIMIT,"launch_logs":2,"packet_buffer":1000},
            "excluded":["credentials","cookies","private keys","account files","IPC discovery file","raw HTTP bodies and headers","historical packet recordings","plugin code and private configuration"],
            "notes":["Logs and snapshots are sanitized. This archive is for diagnosis, not a restorable configuration backup.",
                "Only recorded frontend errors and available recent packets can be exported; earlier errors cannot be reconstructed."]});
        // The manifest has no user input; preserve its field names and status labels.
        bundle.entry(
            "manifest.json",
            &serde_json::to_vec_pretty(&manifest)?,
            false,
            "",
        )?;
        bundle.zip.finish()?.sync_all()?;
        Ok(())
    })();
    if let Err(error) = result {
        let _ = fs::remove_file(&path);
        return Err(error.context("failed to export diagnostics"));
    }
    Ok(path)
}

fn command_output(program: &str, args: &[&str]) -> Value {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let result = (|| -> anyhow::Result<Value> {
        let mut child = command.spawn()?;
        fn capture(
            mut pipe: impl Read + Send + 'static,
        ) -> std::thread::JoinHandle<std::io::Result<Vec<u8>>> {
            std::thread::spawn(move || {
                let mut bytes = Vec::new();
                pipe.by_ref().take(64 * 1024).read_to_end(&mut bytes)?;
                std::io::copy(&mut pipe, &mut std::io::sink())?;
                Ok(bytes)
            })
        }
        let stdout = capture(child.stdout.take().unwrap());
        let stderr = capture(child.stderr.take().unwrap());
        let start = Instant::now();
        while child.try_wait()?.is_none() {
            if start.elapsed() > Duration::from_secs(8) {
                let _ = child.kill();
                let _ = child.wait();
                anyhow::bail!("diagnostic command timed out");
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let status = child.wait()?;
        let stdout = stdout
            .join()
            .map_err(|_| anyhow::anyhow!("diagnostic stdout reader failed"))??;
        let stderr = stderr
            .join()
            .map_err(|_| anyhow::anyhow!("diagnostic stderr reader failed"))??;
        let stdout = String::from_utf8_lossy(&stdout).trim().to_owned();
        let stderr = String::from_utf8_lossy(&stderr).trim().to_owned();
        Ok(
            json!({"ok":status.success(),"exit_code":status.code(),"output":serde_json::from_str::<Value>(&stdout).unwrap_or(json!(stdout)),"error":stderr}),
        )
    })();
    result.unwrap_or_else(|error| json!({"ok":false,"error":error.to_string()}))
}

fn system_diagnostics(port: u16) -> Value {
    #[cfg(target_os = "windows")]
    {
        // The only interpolated input is a typed port; never execute frontend-provided command text.
        let script = format!(
            r#"[Console]::OutputEncoding=[System.Text.UTF8Encoding]::new(); $ErrorActionPreference='Stop'; $connections=@(); $queryError=$null; try {{ Get-Command Get-NetTCPConnection -ErrorAction Stop | Out-Null; $connections=@(Get-NetTCPConnection -LocalPort {port} -ErrorAction SilentlyContinue | ForEach-Object {{ $processName=$null; try {{ $processName=(Get-Process -Id $_.OwningProcess -ErrorAction Stop).ProcessName }} catch {{}}; [pscustomobject]@{{address=$_.LocalAddress;port=$_.LocalPort;state=[string]$_.State;pid=$_.OwningProcess;process=$processName}} }}) }} catch {{ $queryError=$_.Exception.Message }}; [pscustomobject]@{{os_version=[Environment]::OSVersion.VersionString;connections=$connections;query_error=$queryError;excluded_ipv4=@(netsh interface ipv4 show excludedportrange protocol=tcp);excluded_ipv6=@(netsh interface ipv6 show excludedportrange protocol=tcp)}} | ConvertTo-Json -Depth 5 -Compress"#
        );
        command_output(
            "powershell.exe",
            &["-NoProfile", "-NonInteractive", "-Command", &script],
        )
    }
    #[cfg(target_os = "macos")]
    {
        json!({"os_version":command_output("/usr/bin/sw_vers", &["-productVersion"]),"port":command_output("/usr/sbin/lsof", &["-nP", &format!("-iTCP:{port}")])})
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        json!({"os_version":command_output("uname", &["-sr"]),"port":command_output("ss", &["-H", "-tanp", "sport", "=", &format!(":{port}")])})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_nested_secrets_free_text_urls_and_raw_traffic() {
        let config =
            json!({"autorun":{"email_notify":{"pass":"smtp-secret","from":"me@example.com"}}});
        let mut redactor = Redactor::new(&config);
        let mut value = json!({"config":config,"note":"failed smtp-secret at C:\\Users\\Alice\\logs; user@example.com https://u:p@example.com/game?token=xyz",
            "error":"token = abc123; Authorization: Bearer opaque-secret; api_key=unknown-api-value; client_secret=unknown-client-value", "nested":[{"accessToken":"tok-secret","credential_hash":"hash-secret"}],
            "fields":{"content":"base64-secret","headers":"raw-secret"},"packet":{"method":".lq.Lobby.login","data":{"arbitrary":"login-secret"}},
            "amulet":{"id":2250,"uid":42,"store":[]},"account_id":123,"json":"{\"token\":\"encoded-secret\",\"reason\":\"failure\"}"});
        redactor.collect(&value);
        redactor.sanitize(&mut value);
        let text = value.to_string();
        for secret in [
            "smtp-secret",
            "me@example.com",
            "Alice",
            "user@example.com",
            "u:p",
            "xyz",
            "abc123",
            "opaque-secret",
            "tok-secret",
            "hash-secret",
            "base64-secret",
            "raw-secret",
            "login-secret",
            "encoded-secret",
            "unknown-api-value",
            "unknown-client-value",
        ] {
            assert!(!text.contains(secret), "leaked {secret}: {text}");
        }
        assert_eq!(value["amulet"]["uid"], 42);
        assert_eq!(value["account_id"], HIDDEN);
        assert!(text.contains("failure"));
    }

    #[test]
    fn packet_payload_is_opt_in_and_error_codes_survive_redaction() {
        let packet = crate::ipc::PacketLogItem {
            direction: crate::pipeline::Direction::Inbound,
            packet_type: "response".into(),
            method: "game.operation".into(),
            id: Some(7),
            ts_ms: 10,
            data: json!({"hand_tiles":[1,2,3],"error":{"code":10013,"message":"failed"}}),
        };
        let mut summary = packet_diagnostic(packet.clone(), false);
        let mut detail = packet_diagnostic(packet, true);
        let redactor = Redactor::new(&Value::Null);
        redactor.sanitize(&mut summary);
        redactor.sanitize(&mut detail);
        assert!(summary.get("data").is_none());
        assert!(!summary.to_string().contains("hand_tiles"));
        assert_eq!(summary["error"]["code"], 10013);
        assert_eq!(detail["data"]["hand_tiles"], json!([1, 2, 3]));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn port_diagnostics_find_the_current_listener_without_exposing_credentials() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let bound = tokio::net::TcpSocket::new_v4().unwrap();
        bound.bind("127.0.0.1:0".parse().unwrap()).unwrap();
        for (port, expected) in [
            (listener.local_addr().unwrap().port(), "Listen"),
            (bound.local_addr().unwrap().port(), "Bound"),
        ] {
            let result = system_diagnostics(port);
            assert_eq!(result["ok"], true, "{result}");
            assert!(result["output"]["query_error"].is_null(), "{result}");
            let connections = result["output"]["connections"].as_array().unwrap();
            assert!(
                connections
                    .iter()
                    .any(|connection| connection["pid"] == std::process::id()
                        && connection["state"] == expected),
                "{result}"
            );
            assert!(result["output"].get("excluded_ipv4").is_some());
        }
    }

    #[test]
    fn archive_is_bounded_sanitized_and_does_not_copy_other_files() {
        let root = std::env::temp_dir().join(format!(
            "lens-diagnostics-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let logs = root.join("logs");
        fs::create_dir_all(&logs).unwrap();
        let websocket_error = json!({"level":"WARN","target":"shanten_backend::websocket",
            "message":"Direct WebSocket receive failed","fields":{"flow_id":7,
                "error":"WebSocket protocol error: Connection reset without closing handshake",
                "error_debug":"Protocol(ResetWithoutClosingHandshake)"}});
        for timestamp in [1, 2, 3] {
            fs::write(logs.join(format!("shanten-lens-{timestamp}.log")), format!("{{\"message\":\"password=hidden-value\",\"fields\":{{\"content\":\"private-body\"}}}}\ninvalid-line\n{websocket_error}\n")).unwrap();
        }
        fs::write(
            logs.join("shanten-lens-packets-99.log"),
            "never-copy-packet-file",
        )
        .unwrap();
        fs::write(logs.join("panic.log"), "panic at /Users/Alice/main.rs").unwrap();
        fs::write(root.join("private-key.pem"), "never-copy-private-key").unwrap();
        let packets = vec![
            json!({"method":"game.operation","data":{"secret":"packet-secret"}}),
            json!({"method":"large","data":"x".repeat(FILE_LIMIT + 1)}),
            json!({"method":"last-error","data":{"error":1}}),
        ];
        let path = write_archive(
            &root.join("export"),
            &logs,
            json!({"config":{"pass":"hidden-value"},"backend_logs":[websocket_error.clone()],"packets":packets}),
            Redactor::new(&json!({"pass":"hidden-value"})),
            true,
        )
        .unwrap();
        let mut zip = zip::ZipArchive::new(File::open(path).unwrap()).unwrap();
        let mut names = Vec::new();
        let mut all = String::new();
        for index in 0..zip.len() {
            let mut file = zip.by_index(index).unwrap();
            names.push(file.name().to_owned());
            file.read_to_string(&mut all).unwrap();
        }
        assert!(names.contains(&"logs/shanten-lens-3.log".into()));
        assert!(names.contains(&"logs/shanten-lens-2.log".into()));
        assert!(!names.contains(&"logs/shanten-lens-1.log".into()));
        for name in ["logs/backend-recent.jsonl", "logs/shanten-lens-3.log"] {
            let mut contents = String::new();
            zip.by_name(name)
                .unwrap()
                .read_to_string(&mut contents)
                .unwrap();
            let entries: Vec<Value> = contents
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            assert!(
                entries.contains(&websocket_error),
                "missing original WebSocket error in {name}"
            );
        }
        for secret in [
            "hidden-value",
            "private-body",
            "packet-secret",
            "Alice",
            "never-copy",
        ] {
            assert!(!all.contains(secret), "leaked {secret}");
        }
        let mut recent = String::new();
        zip.by_name("packets/recent.jsonl")
            .unwrap()
            .read_to_string(&mut recent)
            .unwrap();
        assert!(recent.contains("last-error"));
        assert!(!recent.contains("large"));
        let mut manifest = String::new();
        zip.by_name("manifest.json")
            .unwrap()
            .read_to_string(&mut manifest)
            .unwrap();
        assert!(manifest.contains("omitted_entries"));
        drop(zip);
        fs::remove_dir_all(root).unwrap();
    }
}
