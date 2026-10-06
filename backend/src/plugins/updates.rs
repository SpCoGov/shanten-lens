use super::manifest::{load_manifest, validate_plugin_entries};
use super::process::ProcessProvider;
use super::{FrontendHealth, PluginInfo, PluginManager, PluginUpdateInfo, RemotePluginUpdate};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs,
    io::{Cursor, Read},
    path::Path,
    thread,
    time::Duration,
};
use tracing::warn;

pub(super) const MAX_UPDATE_ARCHIVE_SIZE: u64 = 128 * 1024 * 1024;
const MAX_UPDATE_FILES: usize = 4_096;

impl PluginManager {
    pub fn set_auto_update(&self, id: &str, enabled: bool) -> anyhow::Result<Vec<PluginInfo>> {
        let _guard = self.update_lock.lock().unwrap();
        let mut plugins = self.plugins.lock().unwrap();
        let plugin = plugins
            .get_mut(id)
            .ok_or_else(|| anyhow::anyhow!("unknown plugin: {id}"))?;
        anyhow::ensure!(
            plugin.manifest.update.is_some() || plugin.manifest.backend.is_some(),
            "plugin has no update channel"
        );
        plugin.choice.auto_update = enabled;
        self.save_config(&plugins)?;
        drop(plugins);
        self.emit_status();
        Ok(self.list())
    }

    pub fn check_updates(&self) -> Vec<PluginUpdateInfo> {
        let sources: Vec<_> = self
            .plugins
            .lock()
            .unwrap()
            .values()
            .filter_map(|plugin| {
                let provider = plugin.provider.as_ref();
                let manifest_url = plugin
                    .manifest
                    .update
                    .as_ref()
                    .map(|source| source.manifest_url.clone());
                if manifest_url.is_none() && plugin.manifest.backend.is_none() {
                    return None;
                }
                if let Some(provider) = provider {
                    provider.check_update();
                }
                Some((
                    plugin.manifest.id.clone(),
                    plugin.manifest.version.clone(),
                    manifest_url,
                    provider.and_then(ProcessProvider::update_offer),
                ))
            })
            .collect();
        let client = update_client();
        sources
            .into_iter()
            .map(|(plugin_id, current_version, manifest_url, offer)| {
                let fetched = match (offer, manifest_url, &client) {
                    (Some(update), _, _) => Ok(Some(update)),
                    (None, Some(manifest_url), Ok(client)) => {
                        fetch_update(client, &manifest_url, &plugin_id)
                            .map(Some)
                            .map_err(|error| error.to_string())
                    }
                    (None, Some(_), Err(error)) => Err(error.to_string()),
                    (None, None, _) => Ok(None),
                };
                match fetched {
                    Ok(Some(update)) => match is_newer(&current_version, &update.version) {
                        Ok(available) => PluginUpdateInfo {
                            plugin_id,
                            current_version,
                            latest_version: Some(update.version),
                            available,
                            release_notes: update.release_notes,
                            error: None,
                        },
                        Err(error) => PluginUpdateInfo {
                            plugin_id,
                            current_version,
                            latest_version: None,
                            available: false,
                            release_notes: None,
                            error: Some(error.to_string()),
                        },
                    },
                    Ok(None) => PluginUpdateInfo {
                        plugin_id,
                        current_version,
                        latest_version: None,
                        available: false,
                        release_notes: None,
                        error: None,
                    },
                    Err(error) => PluginUpdateInfo {
                        plugin_id,
                        current_version,
                        latest_version: None,
                        available: false,
                        release_notes: None,
                        error: Some(error),
                    },
                }
            })
            .collect()
    }

    pub fn update(&self, id: &str) -> anyhow::Result<Vec<PluginInfo>> {
        let _update_guard = self.update_lock.lock().unwrap();
        let result = self.update_locked(id);
        if let Err(error) = &result {
            let mut plugins = self.plugins.lock().unwrap();
            if let Some(plugin) = plugins.get_mut(id) {
                plugin.last_error = Some(format!("plugin update failed: {error}"));
                plugin.choice.auto_update = false;
            }
            if let Err(save_error) = self.save_config(&plugins) {
                warn!(target: "shanten_backend::plugin", %save_error, "failed to save plugin update failure state");
            }
            drop(plugins);
            self.emit_status();
        }
        result
    }

    fn update_locked(&self, id: &str) -> anyhow::Result<Vec<PluginInfo>> {
        let (directory, current_version, manifest_url, offer) = {
            let plugins = self.plugins.lock().unwrap();
            let plugin = plugins
                .get(id)
                .ok_or_else(|| anyhow::anyhow!("unknown plugin: {id}"))?;
            (
                plugin.directory.clone(),
                plugin.manifest.version.clone(),
                plugin
                    .manifest
                    .update
                    .as_ref()
                    .map(|source| source.manifest_url.clone()),
                plugin
                    .provider
                    .as_ref()
                    .and_then(ProcessProvider::update_offer),
            )
        };
        let client = update_client()?;
        let update = match (offer, manifest_url) {
            (Some(update), _) => update,
            (None, Some(manifest_url)) => fetch_update(&client, &manifest_url, id)?,
            (None, None) => anyhow::bail!("plugin has not reported an update"),
        };
        anyhow::ensure!(
            is_newer(&current_version, &update.version)?,
            "plugin is already up to date"
        );
        let archive = download_update(&client, &update)?;
        self.apply_update_archive(id, &directory, &update.version, &archive)
    }

    pub(super) fn apply_update_archive(
        &self,
        id: &str,
        directory: &Path,
        version: &str,
        archive: &[u8],
    ) -> anyhow::Result<Vec<PluginInfo>> {
        let parent = self
            .root
            .parent()
            .ok_or_else(|| anyhow::anyhow!("plugin root has no parent"))?;
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let enabled = self
            .load_config()?
            .plugins
            .get(id)
            .is_some_and(|choice| choice.enabled);
        let staging = parent.join(format!(".plugin-update-{id}-{nonce}"));
        let backup = parent.join(format!(".plugin-backup-{id}-{nonce}"));
        fs::create_dir(&staging)?;
        if let Err(error) = extract_update(archive, &staging) {
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }
        let installed_manifest = match load_manifest(&staging) {
            Ok(manifest) => manifest,
            Err(error) => {
                let _ = fs::remove_dir_all(&staging);
                return Err(error);
            }
        };
        if installed_manifest.id != id || installed_manifest.version != version {
            let _ = fs::remove_dir_all(&staging);
            anyhow::bail!("updated plugin identity or version does not match update manifest");
        }
        if let Err(error) = validate_plugin_entries(&staging, &installed_manifest) {
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }

        let config_path = self.plugin_config_path(id);
        self.plugins.lock().unwrap().remove(id);
        self.emit_status();
        let old_config = match fs::read(&config_path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                let _ = fs::remove_dir_all(&staging);
                let _ = self.rescan_locked();
                return Err(error.into());
            }
        };
        if let Err(error) = fs::rename(directory, &backup) {
            let _ = fs::remove_dir_all(&staging);
            let _ = self.rescan_locked();
            return Err(error.into());
        }
        if let Err(error) = fs::rename(&staging, directory) {
            fs::rename(&backup, directory)?;
            let _ = fs::remove_dir_all(&staging);
            let _ = self.rescan_locked();
            return Err(error.into());
        }
        let needs_frontend = enabled && installed_manifest.frontend.is_some();
        if needs_frontend {
            *self.frontend_health.lock().unwrap() = Some(FrontendHealth {
                id: id.to_owned(),
                token: nonce.to_string(),
                result: None,
            });
        }
        let checked = self
            .rescan_locked()
            .and_then(|_| self.check_health(id, needs_frontend));
        *self.frontend_health.lock().unwrap() = None;
        match checked {
            Ok(()) => {
                let _ = fs::remove_dir_all(&backup);
                Ok(self.list())
            }
            Err(error) => {
                self.plugins.lock().unwrap().remove(id);
                self.emit_status();
                // Keep the failed version aside until the old directory has been restored.
                fs::rename(directory, &staging)?;
                fs::rename(&backup, directory)?;
                if let Some(bytes) = old_config {
                    crate::storage::write_bytes(&config_path, &bytes)?;
                } else if config_path.exists() {
                    fs::remove_file(&config_path)?;
                }
                let _ = fs::remove_dir_all(&staging);
                self.rescan_locked()?;
                let message = format!("plugin update rolled back: {error}");
                let mut plugins = self.plugins.lock().unwrap();
                if let Some(plugin) = plugins.get_mut(id) {
                    plugin.last_error = Some(message.clone());
                    // A restored provider may report the same bad update on startup.
                    plugin.choice.auto_update = false;
                }
                self.save_config(&plugins)?;
                drop(plugins);
                self.emit_status();
                anyhow::bail!(message)
            }
        }
    }

    fn check_health(&self, id: &str, needs_frontend: bool) -> anyhow::Result<()> {
        // Catch providers that acknowledge hello but exit immediately on host.ready.
        thread::sleep(Duration::from_millis(500));
        self.check_backend_health(id)?;
        if needs_frontend {
            let health = self.frontend_health.lock().unwrap();
            let (health, _) = self
                .health_changed
                .wait_timeout_while(health, Duration::from_secs(12), |health| {
                    health
                        .as_ref()
                        .is_some_and(|health| health.result.is_none())
                })
                .unwrap();
            health
                .as_ref()
                .and_then(|health| health.result.clone())
                .unwrap_or_else(|| Err("plugin frontend health check timed out".into()))
                .map_err(anyhow::Error::msg)?;
        }
        self.check_backend_health(id)
    }

    fn check_backend_health(&self, id: &str) -> anyhow::Result<()> {
        let plugins = self.plugins.lock().unwrap();
        let plugin = plugins
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("updated plugin was not discovered"))?;
        if let Some(error) = &plugin.last_error {
            anyhow::bail!("{error}");
        }
        if plugin.choice.enabled && plugin.manifest.backend.is_some() {
            anyhow::ensure!(
                plugin
                    .provider
                    .as_ref()
                    .is_some_and(ProcessProvider::is_running),
                "updated plugin backend is offline"
            );
        }
        Ok(())
    }

    pub(super) fn update_automatic(&self) {
        let enabled: HashSet<_> = self
            .plugins
            .lock()
            .unwrap()
            .values()
            .filter(|plugin| plugin.choice.auto_update)
            .map(|plugin| plugin.manifest.id.clone())
            .collect();
        for update in self
            .check_updates()
            .into_iter()
            .filter(|update| update.available && enabled.contains(&update.plugin_id))
        {
            if let Err(error) = self.update(&update.plugin_id) {
                warn!(target: "shanten_backend::plugin", plugin_id = %update.plugin_id, %error, "automatic plugin update failed");
            }
        }
    }
}

pub(super) fn update_client() -> anyhow::Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .user_agent("Shanten-Lens-Plugin-Updater")
        .timeout(Duration::from_secs(20))
        .build()?)
}

pub(super) fn update_url(value: &str) -> anyhow::Result<reqwest::Url> {
    let url = reqwest::Url::parse(value)?;
    let local_http = url.scheme() == "http"
        && url.host_str().is_some_and(|host| {
            host == "localhost"
                || host
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        });
    anyhow::ensure!(
        url.scheme() == "https" || local_http,
        "plugin update URL must use HTTPS"
    );
    Ok(url)
}

fn fetch_update(
    client: &reqwest::blocking::Client,
    manifest_url: &str,
    plugin_id: &str,
) -> anyhow::Result<RemotePluginUpdate> {
    let url = update_url(manifest_url)?;
    let mut update: RemotePluginUpdate =
        client.get(url.clone()).send()?.error_for_status()?.json()?;
    anyhow::ensure!(
        update.id == plugin_id,
        "update manifest plugin id does not match"
    );
    update.download_url = url.join(&update.download_url)?.to_string();
    validate_update_offer(&update.version, &update.download_url, &update.sha256)?;
    Ok(update)
}

pub(super) fn validate_update_offer(
    version: &str,
    download_url: &str,
    sha256: &str,
) -> anyhow::Result<()> {
    semver::Version::parse(version)?;
    update_url(download_url)?;
    anyhow::ensure!(
        sha256.len() == 64 && sha256.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "update SHA-256 is invalid"
    );
    Ok(())
}

pub(super) fn is_newer(current: &str, latest: &str) -> anyhow::Result<bool> {
    Ok(semver::Version::parse(latest)? > semver::Version::parse(current)?)
}

fn download_update(
    client: &reqwest::blocking::Client,
    update: &RemotePluginUpdate,
) -> anyhow::Result<Vec<u8>> {
    let bytes = download_bytes(client, &update.download_url, MAX_UPDATE_ARCHIVE_SIZE)?;
    let actual = format!("{:x}", Sha256::digest(&bytes));
    anyhow::ensure!(
        actual.eq_ignore_ascii_case(&update.sha256),
        "plugin update SHA-256 mismatch"
    );
    Ok(bytes)
}

pub(super) fn download_bytes(
    client: &reqwest::blocking::Client,
    url: &str,
    limit: u64,
) -> anyhow::Result<Vec<u8>> {
    let url = update_url(url)?;
    let mut response = client.get(url).send()?.error_for_status()?;
    update_url(response.url().as_str())?;
    anyhow::ensure!(
        response.content_length().is_none_or(|size| size <= limit),
        "download is too large"
    );
    let mut bytes = Vec::new();
    response.by_ref().take(limit + 1).read_to_end(&mut bytes)?;
    anyhow::ensure!(bytes.len() as u64 <= limit, "download is too large");
    Ok(bytes)
}

pub(super) fn extract_update(archive: &[u8], destination: &Path) -> anyhow::Result<()> {
    let mut zip = zip::ZipArchive::new(Cursor::new(archive))?;
    anyhow::ensure!(
        zip.len() <= MAX_UPDATE_FILES,
        "plugin update contains too many files"
    );
    let mut total_size = 0_u64;
    for index in 0..zip.len() {
        let mut file = zip.by_index(index)?;
        total_size = total_size.saturating_add(file.size());
        anyhow::ensure!(
            total_size <= MAX_UPDATE_ARCHIVE_SIZE,
            "plugin update expands beyond size limit"
        );
        let relative = file
            .enclosed_name()
            .ok_or_else(|| anyhow::anyhow!("plugin update contains an unsafe path"))?;
        let target = destination.join(relative);
        if file.is_dir() {
            fs::create_dir_all(&target)?;
            continue;
        }
        fs::create_dir_all(
            target
                .parent()
                .ok_or_else(|| anyhow::anyhow!("invalid update path"))?,
        )?;
        let mut output = fs::File::create(&target)?;
        std::io::copy(&mut file, &mut output)?;
        #[cfg(unix)]
        if let Some(mode) = file.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&target, fs::Permissions::from_mode(mode))?;
        }
    }
    anyhow::ensure!(
        destination.join("plugin.json").is_file(),
        "plugin update ZIP must contain plugin.json at its root"
    );
    Ok(())
}
