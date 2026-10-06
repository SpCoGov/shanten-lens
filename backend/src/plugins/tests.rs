use super::marketplace::{marketplace_install_block, DEFAULT_MARKETPLACE_URL};
use super::updates::extract_update;
use super::*;
use crate::pipeline::ModuleConfig;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{BufRead, Cursor, Read, Write},
    sync::{atomic::AtomicU64, mpsc},
    thread,
    time::Duration,
};

struct Fixture {
    manager: Arc<PluginManager>,
    directory: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "shanten-plugin-test-{}-{nonce}",
            std::process::id()
        ));
        let root = directory.join("plugins");
        fs::create_dir_all(&root).unwrap();
        let (events, _) = broadcast::channel(64);
        let (update_requests, _) = mpsc::sync_channel(4);
        let manager = Arc::new(PluginManager {
            root,
            config_path: directory.join("plugins.json"),
            marketplace_config_path: directory.join("plugin-marketplaces.json"),
            registry: Arc::new(ModuleRegistry::default()),
            pipeline: Arc::new(RwLock::new(PipelineConfig {
                schema: 1,
                modules: vec![],
            })),
            pipeline_path: directory.join("pipeline.json"),
            events,
            update_requests,
            update_lock: Mutex::new(()),
            plugins: Mutex::new(HashMap::new()),
            scan_errors: Mutex::new(vec![]),
            frontend_health: Mutex::new(None),
            health_changed: Condvar::new(),
            frontend_generation: AtomicU64::new(0),
        });
        Self { manager, directory }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.manager.plugins.lock().unwrap().clear();
        let target = self.directory.canonicalize().unwrap();
        assert!(target.starts_with(std::env::temp_dir().canonicalize().unwrap()));
        fs::remove_dir_all(target).unwrap();
    }
}

fn archive(version: &str) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    zip.start_file("plugin.json", options).unwrap();
    write!(zip, "{}", json!({"id":"test.plugin","name":"Test","version":version,"apiVersion":1,"frontend":{"entry":"index.js"}})).unwrap();
    zip.start_file("index.js", options).unwrap();
    zip.write_all(b"export function activate() {}").unwrap();
    zip.finish().unwrap().into_inner()
}

#[test]
fn marketplace_installs_verified_archive_disabled() {
    let fixture = Fixture::new();
    let archive = archive("1.0.0");
    let sha256 = format!("{:x}", Sha256::digest(&archive));
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let plugin = json!({
        "id":"test.plugin",
        "name":"Test",
        "version":"1.0.0",
        "apiVersion":1,
        "repository":"https://example.com/test.plugin",
        "package":{"downloadUrl":format!("{base}/plugin.zip"),"sha256":sha256,"size":archive.len()},
        "publishedAt":"2026-09-12T00:00:00Z",
        "status":"active"
    });
    let registry = serde_json::to_vec(&json!({
        "schemaVersion":1,
        "marketplace":{"id":"test-market","name":"Test Market"},
        "revision":"1",
        "generatedAt":"2026-09-12T00:00:00Z",
        "plugins":[plugin.clone()]
    }))
    .unwrap();
    let served_archive = archive.clone();
    let server = thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 2048];
            let size = stream.read(&mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..size]);
            let body = if request.starts_with("GET /registry.json ") {
                &registry
            } else {
                &served_archive
            };
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .unwrap();
            stream.write_all(body).unwrap();
        }
    });
    let source_url = format!("{base}/registry.json");
    fs::write(
        &fixture.manager.marketplace_config_path,
        serde_json::to_vec(&json!({"schema":1,"sources":[source_url]})).unwrap(),
    )
    .unwrap();

    let installed = fixture
        .manager
        .install_marketplace_plugin(&source_url, "test.plugin")
        .unwrap();
    server.join().unwrap();
    assert_eq!(installed[0].version, "1.0.0");
    assert!(!installed[0].enabled);
    assert!(fixture
        .manager
        .remove_marketplace_source(DEFAULT_MARKETPLACE_URL)
        .is_err());

    let mut candidate: MarketplaceRegistryPlugin = serde_json::from_value(plugin).unwrap();
    candidate.status = MarketplacePluginStatus::Blocked;
    assert_eq!(
        marketplace_install_block(&candidate).unwrap(),
        Some(MarketplaceInstallBlock::Blocked)
    );
    candidate.status = MarketplacePluginStatus::Active;
    candidate.api_version = 2;
    assert_eq!(
        marketplace_install_block(&candidate).unwrap(),
        Some(MarketplaceInstallBlock::UnsupportedApi)
    );
    candidate.api_version = 1;
    candidate.minimum_app_version = Some("999.0.0".into());
    assert_eq!(
        marketplace_install_block(&candidate).unwrap(),
        Some(MarketplaceInstallBlock::AppTooOld)
    );
}

#[test]
fn install_uninstall_and_scan_errors() {
    let fixture = Fixture::new();
    let manager = &fixture.manager;
    assert!(!manager.install(&archive("1.0.0")).unwrap()[0].enabled);
    assert!(manager.install(&archive("1.0.0")).is_err());
    manager
        .set_config("test.plugin", json!({"keep":true}))
        .unwrap();
    manager.uninstall("test.plugin", false).unwrap();
    manager.install(&archive("1.0.0")).unwrap();
    assert_eq!(
        manager.get_config("test.plugin").unwrap(),
        json!({"keep":true})
    );
    let duplicate = manager.root.join("duplicate");
    fs::create_dir(&duplicate).unwrap();
    extract_update(&archive("1.0.0"), &duplicate).unwrap();
    let broken = manager.root.join("broken");
    fs::create_dir(&broken).unwrap();
    fs::write(broken.join("plugin.json"), "invalid").unwrap();
    manager.rescan().unwrap();
    assert_eq!(manager.scan_errors().len(), 2);
    // The lexically first duplicate is the discovered plugin.
    manager.uninstall("test.plugin", true).unwrap();
    assert!(!manager.plugin_config_path("test.plugin").exists());
}

#[test]
fn rescan_cleans_modules_left_by_previously_uninstalled_plugins() {
    let fixture = Fixture::new();
    let manager = &fixture.manager;
    let id = "plugin:removed.plugin:main:stale";
    let mut module = PacketModuleInfo::builtin(id, vec![]);
    module.builtin = false;
    manager.registry.register(module).unwrap();
    manager
        .pipeline
        .blocking_write()
        .modules
        .push(ModuleConfig {
            id: id.into(),
            enabled: true,
            options: Value::Null,
        });
    manager.rescan().unwrap();
    assert!(manager.registry.get(id).is_none());
    assert!(manager.pipeline.blocking_read().modules.is_empty());
}

#[test]
fn uninstall_removes_only_owned_modules_from_registry_and_saved_pipeline() {
    let fixture = Fixture::new();
    let manager = &fixture.manager;
    manager.install(&archive("1.0.0")).unwrap();
    let other = manager.root.join("other");
    fs::create_dir(&other).unwrap();
    extract_update(&archive("1.0.0"), &other).unwrap();
    let path = other.join("plugin.json");
    let text = fs::read_to_string(&path)
        .unwrap()
        .replace("test.plugin", "test.plugin.other");
    fs::write(path, text).unwrap();
    for id in [
        "plugin:test.plugin:main:first",
        "plugin:test.plugin:other:second",
        "plugin:test.plugin.other:main:keep",
        "method_filter",
    ] {
        let mut module = PacketModuleInfo::builtin(id, vec![]);
        module.builtin = id == "method_filter";
        manager.registry.register(module).unwrap();
        manager
            .pipeline
            .blocking_write()
            .modules
            .push(ModuleConfig {
                id: id.into(),
                enabled: true,
                options: json!({"keep":true}),
            });
    }
    manager.uninstall("test.plugin", false).unwrap();
    let config = manager.pipeline.blocking_read().clone();
    let ids: Vec<_> = config.modules.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["plugin:test.plugin.other:main:keep", "method_filter"]
    );
    assert!(manager
        .registry
        .get("plugin:test.plugin:main:first")
        .is_none());
    assert!(manager
        .registry
        .get("plugin:test.plugin:other:second")
        .is_none());
    assert!(manager
        .registry
        .get("plugin:test.plugin.other:main:keep")
        .is_some());
    let saved: PipelineConfig =
        serde_json::from_slice(&fs::read(&manager.pipeline_path).unwrap()).unwrap();
    assert_eq!(
        serde_json::to_value(saved).unwrap(),
        serde_json::to_value(config).unwrap()
    );
}

#[test]
fn frontend_health_failure_restores_version_and_config() {
    let fixture = Fixture::new();
    let manager = &fixture.manager;
    manager.install(&archive("1.0.0")).unwrap();
    manager.set_enabled("test.plugin", true, vec![]).unwrap();
    manager
        .set_config("test.plugin", json!({"version":1}))
        .unwrap();
    let reporter = manager.clone();
    let task = thread::spawn(move || {
        for _ in 0..300 {
            let token = reporter
                .frontend_bundles()
                .into_iter()
                .find_map(|bundle| bundle.health_token);
            if let Some(token) = token {
                reporter
                    .set_config("test.plugin", json!({"version":2}))
                    .unwrap();
                reporter.report_frontend_health("test.plugin", "stale-token", None);
                assert!(reporter
                    .frontend_health
                    .lock()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .result
                    .is_none());
                reporter.report_frontend_health(
                    "test.plugin",
                    &token,
                    Some("activation failed".into()),
                );
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("update health request was not published");
    });
    let error = manager
        .apply_update_archive(
            "test.plugin",
            &manager.root.join("test.plugin"),
            "2.0.0",
            &archive("2.0.0"),
        )
        .unwrap_err();
    task.join().unwrap();
    assert!(error.to_string().contains("rolled back"));
    assert_eq!(manager.list()[0].version, "1.0.0");
    assert_eq!(
        manager.get_config("test.plugin").unwrap(),
        json!({"version":1})
    );
    assert!(manager.list()[0]
        .last_error
        .as_ref()
        .unwrap()
        .contains("activation failed"));
}

#[test]
fn backend_start_failure_rolls_back() {
    let fixture = Fixture::new();
    let manager = &fixture.manager;
    manager.install(&archive("1.0.0")).unwrap();
    manager.set_enabled("test.plugin", true, vec![]).unwrap();
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    zip.start_file("plugin.json", options).unwrap();
    write!(
        zip,
        "{}",
        json!({"id":"test.plugin","name":"Test","version":"2.0.0","apiVersion":1,
        "backend":{"type":"process","entry":"broken.exe","args":["--exact","plugins::tests::provider_process","--nocapture"]}})
    )
    .unwrap();
    zip.start_file("broken.exe", options).unwrap();
    // This fixture exits without a handshake when its ID is test.plugin.
    zip.write_all(&fs::read(std::env::current_exe().unwrap()).unwrap())
        .unwrap();
    let bytes = zip.finish().unwrap().into_inner();
    assert!(manager
        .apply_update_archive(
            "test.plugin",
            &manager.root.join("test.plugin"),
            "2.0.0",
            &bytes
        )
        .is_err());
    assert_eq!(manager.list()[0].version, "1.0.0");
    assert!(manager.list()[0].running);
}

#[test]
fn healthy_frontend_update_commits() {
    let fixture = Fixture::new();
    let manager = &fixture.manager;
    manager.install(&archive("1.0.0")).unwrap();
    manager.set_enabled("test.plugin", true, vec![]).unwrap();
    let reporter = manager.clone();
    let task = thread::spawn(move || {
        for _ in 0..300 {
            if let Some(token) = reporter
                .frontend_bundles()
                .into_iter()
                .find_map(|bundle| bundle.health_token)
            {
                reporter.report_frontend_health("test.plugin", &token, None);
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("update health request was not published");
    });
    manager
        .apply_update_archive(
            "test.plugin",
            &manager.root.join("test.plugin"),
            "2.0.0",
            &archive("2.0.0"),
        )
        .unwrap();
    task.join().unwrap();
    assert_eq!(manager.list()[0].version, "2.0.0");
    assert!(manager.list()[0].running);
    assert!(!fs::read_dir(&fixture.directory).unwrap().any(|entry| entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".plugin-")));
}

#[test]
fn disabled_update_does_not_execute_frontend() {
    let fixture = Fixture::new();
    let manager = &fixture.manager;
    manager.install(&archive("1.0.0")).unwrap();
    manager
        .apply_update_archive(
            "test.plugin",
            &manager.root.join("test.plugin"),
            "2.0.0",
            &archive("2.0.0"),
        )
        .unwrap();
    assert_eq!(manager.list()[0].version, "2.0.0");
    assert!(!manager.list()[0].enabled);
    assert!(manager.frontend_health.lock().unwrap().is_none());
}

#[test]
fn provider_process() {
    if std::env::var("SHANTEN_LENS_PLUGIN_ID").as_deref() != Ok("test.provider") {
        return;
    }
    for line in std::io::stdin().lock().lines().map_while(Result::ok) {
        let message: Value = serde_json::from_str(&line).unwrap();
        let response = match message["method"].as_str() {
            Some("host.hello") => {
                json!({"jsonrpc":"2.0","id":message["id"],"result":{"providerId":"main"}})
            }
            Some("host.ready") => {
                json!({"jsonrpc":"2.0","method":"module.register","params":{"moduleId":"observer","subscriptionsRevision":1,"subscriptions":[]}})
            }
            Some("host.shutdown") => return,
            _ => continue,
        };
        let mut out = std::io::stdout().lock();
        writeln!(out, "{response}").unwrap();
        out.flush().unwrap();
    }
}

#[test]
fn rescan_keeps_replacement_online_after_old_disconnect() {
    let fixture = Fixture::new();
    let manager = &fixture.manager;
    let directory = manager.root.join("test.provider");
    fs::create_dir(&directory).unwrap();
    fs::copy(
        std::env::current_exe().unwrap(),
        directory.join("provider.exe"),
    )
    .unwrap();
    fs::write(directory.join("plugin.json"), json!({"id":"test.provider","name":"Test","version":"1.0.0","apiVersion":1,
        "backend":{"type":"process","entry":"provider.exe","args":["--exact","plugins::tests::provider_process","--nocapture"]}}).to_string()).unwrap();
    manager.rescan().unwrap();
    manager.set_enabled("test.provider", true, vec![]).unwrap();
    let old = manager
        .plugins
        .lock()
        .unwrap()
        .get("test.provider")
        .unwrap()
        .provider
        .as_ref()
        .unwrap()
        .core
        .clone();
    for _ in 0..3 {
        manager.rescan().unwrap();
        for _ in 0..100 {
            if manager
                .registry
                .snapshot()
                .iter()
                .any(|module| module.online)
            {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        old.disconnect();
        assert!(manager.list()[0].running);
        assert!(manager
            .registry
            .snapshot()
            .iter()
            .any(|module| module.online));
        assert!(manager
            .registry
            .external("plugin:test.provider:main:observer")
            .is_some());
    }
}
