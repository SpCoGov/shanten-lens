use super::manifest::validate_identifier;
use super::updates::{download_bytes, update_client, update_url, MAX_UPDATE_ARCHIVE_SIZE};
use super::{
    MarketplaceConfig, MarketplaceInstallBlock, MarketplacePackageInfo, MarketplacePluginInfo,
    MarketplacePluginStatus, MarketplaceRegistry, MarketplaceRegistryPlugin, MarketplaceSnapshot,
    MarketplaceSourceInfo, PluginInfo, PluginManager,
};
use sha2::{Digest, Sha256};
use std::{collections::HashSet, fs};

const MAX_MARKETPLACE_REGISTRY_SIZE: u64 = 4 * 1024 * 1024;
const MAX_MARKETPLACE_PLUGINS: usize = 10_000;
pub(super) const DEFAULT_MARKETPLACE_URL: &str =
    "https://raw.githubusercontent.com/SpCoGov/shanten-lens-marketplace/main/public/registry.json";

impl PluginManager {
    pub fn marketplace(&self) -> anyhow::Result<MarketplaceSnapshot> {
        let sources = self.marketplace_sources()?;
        let client = update_client();
        let mut source_info = Vec::with_capacity(sources.len());
        let mut plugins = Vec::new();
        for (url, default) in sources {
            let fetched = client
                .as_ref()
                .map_err(|error| anyhow::anyhow!(error.to_string()))
                .and_then(|client| fetch_marketplace_registry(client, &url));
            match fetched {
                Ok(registry) => {
                    let marketplace_id = registry.marketplace.id.clone();
                    let marketplace_name = registry.marketplace.name.clone();
                    plugins.extend(registry.plugins.into_iter().map(|plugin| {
                        marketplace_plugin_info(&url, &marketplace_id, &marketplace_name, plugin)
                    }));
                    source_info.push(MarketplaceSourceInfo {
                        url,
                        marketplace_id: Some(marketplace_id),
                        name: Some(marketplace_name),
                        default,
                        error: None,
                    });
                }
                Err(error) => source_info.push(MarketplaceSourceInfo {
                    url,
                    marketplace_id: None,
                    name: None,
                    default,
                    error: Some(error.to_string()),
                }),
            }
        }
        plugins.sort_by(|left, right| {
            left.name
                .to_lowercase()
                .cmp(&right.name.to_lowercase())
                .then_with(|| left.id.cmp(&right.id))
                .then_with(|| left.source_url.cmp(&right.source_url))
        });
        Ok(MarketplaceSnapshot {
            sources: source_info,
            plugins,
        })
    }

    pub fn add_marketplace_source(&self, url: &str) -> anyhow::Result<MarketplaceSnapshot> {
        let url = update_url(url)?.to_string();
        anyhow::ensure!(
            url != update_url(DEFAULT_MARKETPLACE_URL)?.to_string(),
            "marketplace source is already the default"
        );
        fetch_marketplace_registry(&update_client()?, &url)?;
        let guard = self.update_lock.lock().unwrap();
        let mut config = self.load_marketplace_config()?;
        anyhow::ensure!(
            !config.sources.iter().any(|source| source == &url),
            "marketplace source already exists"
        );
        config.sources.push(url);
        self.save_marketplace_config(&config)?;
        drop(guard);
        self.marketplace()
    }

    pub fn remove_marketplace_source(&self, url: &str) -> anyhow::Result<MarketplaceSnapshot> {
        let _guard = self.update_lock.lock().unwrap();
        let url = update_url(url)?.to_string();
        anyhow::ensure!(
            url != update_url(DEFAULT_MARKETPLACE_URL)?.to_string(),
            "default marketplace source cannot be removed"
        );
        let mut config = self.load_marketplace_config()?;
        let previous = config.sources.len();
        config.sources.retain(|source| source != &url);
        anyhow::ensure!(
            config.sources.len() != previous,
            "marketplace source does not exist"
        );
        self.save_marketplace_config(&config)?;
        drop(_guard);
        self.marketplace()
    }

    pub fn install_marketplace_plugin(
        &self,
        source_url: &str,
        plugin_id: &str,
    ) -> anyhow::Result<Vec<PluginInfo>> {
        let source_url = update_url(source_url)?.to_string();
        anyhow::ensure!(
            self.marketplace_sources()?
                .iter()
                .any(|(source, _)| source == &source_url),
            "unknown marketplace source"
        );
        let client = update_client()?;
        let registry = fetch_marketplace_registry(&client, &source_url)?;
        let plugin = registry
            .plugins
            .into_iter()
            .find(|plugin| plugin.id == plugin_id)
            .ok_or_else(|| anyhow::anyhow!("plugin is not present in marketplace source"))?;
        anyhow::ensure!(
            marketplace_install_block(&plugin)?.is_none(),
            "plugin cannot be installed on this app version"
        );
        let archive = download_bytes(
            &client,
            &plugin.package.download_url,
            MAX_UPDATE_ARCHIVE_SIZE,
        )?;
        anyhow::ensure!(
            archive.len() as u64 == plugin.package.size,
            "marketplace package size mismatch"
        );
        let actual = format!("{:x}", Sha256::digest(&archive));
        anyhow::ensure!(
            actual.eq_ignore_ascii_case(&plugin.package.sha256),
            "marketplace package SHA-256 mismatch"
        );
        self.install_expected(&archive, Some((&plugin.id, &plugin.version)))
    }

    fn marketplace_sources(&self) -> anyhow::Result<Vec<(String, bool)>> {
        let default = update_url(DEFAULT_MARKETPLACE_URL)
            .expect("default marketplace URL is valid")
            .to_string();
        let mut sources = vec![(default.clone(), true)];
        for source in self.load_marketplace_config()?.sources {
            if source != default && !sources.iter().any(|(url, _)| url == &source) {
                sources.push((source, false));
            }
        }
        Ok(sources)
    }

    fn load_marketplace_config(&self) -> anyhow::Result<MarketplaceConfig> {
        match fs::read(&self.marketplace_config_path) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(MarketplaceConfig {
                schema: 1,
                sources: Vec::new(),
            }),
            Err(error) => Err(error.into()),
        }
    }

    fn save_marketplace_config(&self, config: &MarketplaceConfig) -> anyhow::Result<()> {
        crate::storage::write_json(&self.marketplace_config_path, config)?;
        Ok(())
    }
}

fn fetch_marketplace_registry(
    client: &reqwest::blocking::Client,
    registry_url: &str,
) -> anyhow::Result<MarketplaceRegistry> {
    let bytes = download_bytes(client, registry_url, MAX_MARKETPLACE_REGISTRY_SIZE)?;
    let registry: MarketplaceRegistry = serde_json::from_slice(&bytes)?;
    anyhow::ensure!(
        registry.schema_version == 1,
        "unsupported marketplace schema version"
    );
    validate_identifier(&registry.marketplace.id, "marketplace id")?;
    anyhow::ensure!(
        !registry.marketplace.name.trim().is_empty(),
        "marketplace name cannot be empty"
    );
    anyhow::ensure!(
        !registry.revision.trim().is_empty() && !registry.generated_at.trim().is_empty(),
        "marketplace revision or generation time is missing"
    );
    anyhow::ensure!(
        registry.plugins.len() <= MAX_MARKETPLACE_PLUGINS,
        "marketplace contains too many plugins"
    );
    let mut ids = HashSet::new();
    for plugin in &registry.plugins {
        validate_identifier(&plugin.id, "marketplace plugin id")?;
        anyhow::ensure!(
            ids.insert(plugin.id.as_str()),
            "marketplace contains duplicate plugin ids"
        );
        anyhow::ensure!(
            !plugin.name.trim().is_empty(),
            "marketplace plugin name cannot be empty"
        );
        semver::Version::parse(&plugin.version)?;
        anyhow::ensure!(plugin.api_version > 0, "marketplace plugin API is invalid");
        if let Some(version) = &plugin.minimum_app_version {
            semver::Version::parse(version)?;
        }
        for (label, value) in [
            ("homepage", plugin.homepage.as_deref()),
            ("source", plugin.source.as_deref()),
            ("repository", Some(plugin.repository.as_str())),
            ("issues", plugin.issues.as_deref()),
            ("release notes", plugin.release_notes_url.as_deref()),
        ] {
            if let Some(value) = value {
                update_url(value)
                    .map_err(|error| anyhow::anyhow!("invalid {label} URL: {error}"))?;
            }
        }
        anyhow::ensure!(
            plugin.categories.iter().collect::<HashSet<_>>().len() == plugin.categories.len(),
            "marketplace plugin categories must be unique"
        );
        update_url(&plugin.package.download_url)?;
        anyhow::ensure!(
            plugin.package.size > 0 && plugin.package.size <= MAX_UPDATE_ARCHIVE_SIZE,
            "marketplace package size is invalid"
        );
        anyhow::ensure!(
            plugin.package.sha256.len() == 64
                && plugin
                    .package
                    .sha256
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit()),
            "marketplace package SHA-256 is invalid"
        );
        anyhow::ensure!(
            !plugin.published_at.trim().is_empty(),
            "marketplace plugin publish time is missing"
        );
    }
    Ok(registry)
}

pub(super) fn marketplace_install_block(
    plugin: &MarketplaceRegistryPlugin,
) -> anyhow::Result<Option<MarketplaceInstallBlock>> {
    if plugin.status == MarketplacePluginStatus::Blocked {
        return Ok(Some(MarketplaceInstallBlock::Blocked));
    }
    if plugin.api_version != 1 {
        return Ok(Some(MarketplaceInstallBlock::UnsupportedApi));
    }
    if let Some(minimum) = &plugin.minimum_app_version {
        if semver::Version::parse(minimum)? > semver::Version::parse(crate::VERSION)? {
            return Ok(Some(MarketplaceInstallBlock::AppTooOld));
        }
    }
    Ok(None)
}

fn marketplace_plugin_info(
    source_url: &str,
    marketplace_id: &str,
    marketplace_name: &str,
    plugin: MarketplaceRegistryPlugin,
) -> MarketplacePluginInfo {
    let install_block = marketplace_install_block(&plugin).expect("marketplace plugin validated");
    MarketplacePluginInfo {
        source_url: source_url.to_owned(),
        marketplace_id: marketplace_id.to_owned(),
        marketplace_name: marketplace_name.to_owned(),
        id: plugin.id,
        name: plugin.name,
        description: plugin.description,
        version: plugin.version,
        api_version: plugin.api_version,
        minimum_app_version: plugin.minimum_app_version,
        author: plugin.author,
        homepage: plugin.homepage,
        source: plugin.source,
        repository: plugin.repository,
        issues: plugin.issues,
        license: plugin.license,
        categories: plugin.categories,
        package: MarketplacePackageInfo {
            download_url: plugin.package.download_url,
            sha256: plugin.package.sha256,
            size: plugin.package.size,
        },
        release_notes: plugin.release_notes,
        release_notes_url: plugin.release_notes_url,
        published_at: plugin.published_at,
        status: plugin.status,
        install_block,
    }
}
