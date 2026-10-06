use super::updates::update_url;
use super::PluginManifest;
use std::{fs, path::Path};

pub(super) fn load_manifest(directory: &Path) -> anyhow::Result<PluginManifest> {
    let path = directory.join("plugin.json");
    let manifest: PluginManifest = serde_json::from_slice(&fs::read(&path)?)?;
    validate_identifier(&manifest.id, "plugin id")?;
    anyhow::ensure!(
        !manifest.name.trim().is_empty(),
        "plugin name cannot be empty"
    );
    anyhow::ensure!(
        !manifest.version.trim().is_empty(),
        "plugin version cannot be empty"
    );
    anyhow::ensure!(manifest.api_version == 1, "unsupported plugin API version");
    for (label, value) in [
        ("homepage", manifest.homepage.as_deref()),
        ("source", manifest.source.as_deref()),
        ("issues", manifest.issues.as_deref()),
    ] {
        if let Some(value) = value {
            let url = reqwest::Url::parse(value)?;
            anyhow::ensure!(
                matches!(url.scheme(), "http" | "https"),
                "plugin {label} URL must use HTTP or HTTPS"
            );
        }
    }
    if let Some(update) = &manifest.update {
        update_url(&update.manifest_url)?;
    }
    anyhow::ensure!(
        manifest.backend.is_some() || manifest.frontend.is_some(),
        "plugin has no backend or frontend entry"
    );
    Ok(manifest)
}

pub(super) fn validate_plugin_entries(
    directory: &Path,
    manifest: &PluginManifest,
) -> anyhow::Result<()> {
    let root = directory.canonicalize()?;
    for (label, entry) in [
        (
            "backend",
            manifest.backend.as_ref().map(|value| value.entry.as_str()),
        ),
        (
            "frontend",
            manifest.frontend.as_ref().map(|value| value.entry.as_str()),
        ),
    ] {
        let Some(entry) = entry else { continue };
        let path = root.join(entry).canonicalize()?;
        anyhow::ensure!(
            path.starts_with(&root) && path.is_file(),
            "plugin {label} entry is invalid"
        );
    }
    Ok(())
}

pub(super) fn validate_identifier(value: &str, label: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        !value.is_empty()
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')),
        "invalid {label}"
    );
    Ok(())
}
