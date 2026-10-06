use super::{ManagedPlugin, PluginFrontendBundle, PluginManager};
use serde_json::Value;
use std::{collections::HashMap, fs, path::Path};

impl PluginManager {
    pub fn report_frontend_health(&self, id: &str, token: &str, error: Option<String>) {
        let mut health = self.frontend_health.lock().unwrap();
        if let Some(health) = health
            .as_mut()
            .filter(|health| health.id == id && health.token == token && health.result.is_none())
        {
            health.result = Some(error.map_or(Ok(()), Err));
            self.health_changed.notify_all();
        }
    }

    pub fn frontend_bundles(&self) -> Vec<PluginFrontendBundle> {
        self.plugins
            .lock()
            .unwrap()
            .values()
            .filter(|plugin| plugin.choice.enabled && plugin.manifest.frontend.is_some())
            .map(|plugin| {
                let mut bundle = load_frontend_bundle(plugin);
                bundle.generation = plugin.frontend_generation;
                bundle.health_token = self
                    .frontend_health
                    .lock()
                    .unwrap()
                    .as_ref()
                    .filter(|health| health.id == plugin.manifest.id)
                    .map(|health| health.token.clone());
                bundle
            })
            .collect()
    }
}

fn load_frontend_bundle(plugin: &ManagedPlugin) -> PluginFrontendBundle {
    let result = (|| -> anyhow::Result<(String, String, HashMap<String, Value>)> {
        let frontend = plugin.manifest.frontend.as_ref().expect("frontend checked");
        let directory = plugin.directory.canonicalize()?;
        let entry = directory.join(&frontend.entry).canonicalize()?;
        anyhow::ensure!(
            entry.starts_with(&directory),
            "plugin frontend entry escapes plugin directory"
        );
        anyhow::ensure!(entry.is_file(), "plugin frontend entry is not a file");
        let source = fs::read_to_string(&entry)?;
        let entry_path = webview_path(&entry);
        let mut locales = HashMap::new();
        let locale_dir = directory.join("locales");
        if locale_dir.is_dir() {
            for entry in fs::read_dir(locale_dir)?.filter_map(Result::ok) {
                let path = entry.path();
                if path.extension().and_then(|value| value.to_str()) != Some("json") {
                    continue;
                }
                let Some(locale) = path.file_stem().and_then(|value| value.to_str()) else {
                    continue;
                };
                let value: Value = serde_json::from_slice(&fs::read(&path)?)?;
                anyhow::ensure!(value.is_object(), "plugin locale must be a JSON object");
                locales.insert(locale.to_owned(), value);
            }
        }
        Ok((source, entry_path, locales))
    })();
    match result {
        Ok((source, entry_path, locales)) => PluginFrontendBundle {
            plugin_id: plugin.manifest.id.clone(),
            health_token: None,
            generation: 0,
            source: Some(source),
            entry_path: Some(entry_path),
            default_locale: plugin
                .manifest
                .default_locale
                .clone()
                .unwrap_or_else(|| "zh-CN".into()),
            locales,
            error: None,
        },
        Err(error) => PluginFrontendBundle {
            plugin_id: plugin.manifest.id.clone(),
            health_token: None,
            generation: 0,
            source: None,
            entry_path: None,
            default_locale: plugin
                .manifest
                .default_locale
                .clone()
                .unwrap_or_else(|| "zh-CN".into()),
            locales: HashMap::new(),
            error: Some(error.to_string()),
        },
    }
}

fn webview_path(path: &Path) -> String {
    let value = path.to_string_lossy();
    #[cfg(windows)]
    {
        if let Some(path) = value.strip_prefix(r"\\?\UNC\") {
            return format!(r"//{path}").replace('\\', "/");
        }
        value
            .strip_prefix(r"\\?\")
            .unwrap_or(&value)
            .replace('\\', "/")
    }
    #[cfg(not(windows))]
    value.into_owned()
}
