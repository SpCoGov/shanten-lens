use super::BackendRuntime;
use crate::plugins::{
    MarketplaceSnapshot, PluginFrontendBundle, PluginInfo, PluginScanError, PluginUpdateInfo,
};
use serde_json::Value;

impl BackendRuntime {
    pub fn plugins(&self) -> Vec<PluginInfo> {
        self.state.plugins.list()
    }

    pub async fn plugin_marketplace(&self) -> Result<MarketplaceSnapshot, String> {
        let plugins = self.state.plugins.clone();
        tokio::task::spawn_blocking(move || {
            plugins.marketplace().map_err(|error| error.to_string())
        })
        .await
        .map_err(|error| error.to_string())?
    }

    pub async fn add_plugin_marketplace_source(
        &self,
        url: String,
    ) -> Result<MarketplaceSnapshot, String> {
        let plugins = self.state.plugins.clone();
        tokio::task::spawn_blocking(move || {
            plugins
                .add_marketplace_source(&url)
                .map_err(|error| error.to_string())
        })
        .await
        .map_err(|error| error.to_string())?
    }

    pub async fn remove_plugin_marketplace_source(
        &self,
        url: String,
    ) -> Result<MarketplaceSnapshot, String> {
        let plugins = self.state.plugins.clone();
        tokio::task::spawn_blocking(move || {
            plugins
                .remove_marketplace_source(&url)
                .map_err(|error| error.to_string())
        })
        .await
        .map_err(|error| error.to_string())?
    }

    pub async fn install_marketplace_plugin(
        &self,
        source_url: String,
        plugin_id: String,
    ) -> Result<Vec<PluginInfo>, String> {
        let plugins = self.state.plugins.clone();
        tokio::task::spawn_blocking(move || {
            plugins
                .install_marketplace_plugin(&source_url, &plugin_id)
                .map_err(|error| error.to_string())
        })
        .await
        .map_err(|error| error.to_string())?
    }

    pub fn plugin_frontends(&self) -> Vec<PluginFrontendBundle> {
        self.state.plugins.frontend_bundles()
    }

    pub fn plugin_config(&self, id: String) -> Result<Value, String> {
        self.state
            .plugins
            .get_config(&id)
            .map_err(|error| error.to_string())
    }

    pub fn set_plugin_config(&self, id: String, value: Value) -> Result<(), String> {
        self.state
            .plugins
            .set_config(&id, value)
            .map_err(|error| error.to_string())
    }

    pub async fn invoke_plugin(
        &self,
        id: String,
        method: String,
        params: Value,
    ) -> Result<Value, String> {
        let plugins = self.state.plugins.clone();
        tokio::task::spawn_blocking(move || {
            plugins
                .invoke(&id, &method, params)
                .map_err(|error| error.to_string())
        })
        .await
        .map_err(|error| error.to_string())?
    }

    pub fn plugin_scan_errors(&self) -> Vec<PluginScanError> {
        self.state.plugins.scan_errors()
    }

    pub fn report_plugin_frontend_health(&self, id: String, token: String, error: Option<String>) {
        self.state
            .plugins
            .report_frontend_health(&id, &token, error);
    }

    pub async fn install_plugin(&self, archive: String) -> Result<Vec<PluginInfo>, String> {
        let plugins = self.state.plugins.clone();
        tokio::task::spawn_blocking(move || {
            use base64::Engine;
            if archive.len() > 180 * 1024 * 1024 {
                return Err("plugin archive is too large".into());
            }
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(archive)
                .map_err(|error| error.to_string())?;
            plugins.install(&bytes).map_err(|error| error.to_string())
        })
        .await
        .map_err(|error| error.to_string())?
    }

    pub async fn uninstall_plugin(
        &self,
        id: String,
        remove_config: bool,
    ) -> Result<Vec<PluginInfo>, String> {
        let plugins = self.state.plugins.clone();
        tokio::task::spawn_blocking(move || {
            plugins
                .uninstall(&id, remove_config)
                .map_err(|error| error.to_string())
        })
        .await
        .map_err(|error| error.to_string())?
    }

    pub async fn rescan_plugins(&self) -> Result<Vec<PluginInfo>, String> {
        let plugins = self.state.plugins.clone();
        tokio::task::spawn_blocking(move || plugins.rescan().map_err(|error| error.to_string()))
            .await
            .map_err(|error| error.to_string())?
    }

    pub async fn set_plugin_enabled(
        &self,
        id: String,
        enabled: bool,
        approved_permissions: Vec<shanten_backend::pipeline::PacketOperation>,
    ) -> Result<Vec<PluginInfo>, String> {
        let plugins = self.state.plugins.clone();
        tokio::task::spawn_blocking(move || {
            plugins
                .set_enabled(&id, enabled, approved_permissions)
                .map_err(|error| error.to_string())
        })
        .await
        .map_err(|error| error.to_string())?
    }

    pub async fn restart_plugin(&self, id: String) -> Result<Vec<PluginInfo>, String> {
        let plugins = self.state.plugins.clone();
        tokio::task::spawn_blocking(move || plugins.restart(&id).map_err(|error| error.to_string()))
            .await
            .map_err(|error| error.to_string())?
    }

    pub async fn set_plugin_auto_update(
        &self,
        id: String,
        enabled: bool,
    ) -> Result<Vec<PluginInfo>, String> {
        let plugins = self.state.plugins.clone();
        tokio::task::spawn_blocking(move || {
            plugins
                .set_auto_update(&id, enabled)
                .map_err(|error| error.to_string())
        })
        .await
        .map_err(|error| error.to_string())?
    }

    pub async fn check_plugin_updates(&self) -> Result<Vec<PluginUpdateInfo>, String> {
        let plugins = self.state.plugins.clone();
        tokio::task::spawn_blocking(move || plugins.check_updates())
            .await
            .map_err(|error| error.to_string())
    }

    pub async fn update_plugin(&self, id: String) -> Result<Vec<PluginInfo>, String> {
        let plugins = self.state.plugins.clone();
        tokio::task::spawn_blocking(move || plugins.update(&id).map_err(|error| error.to_string()))
            .await
            .map_err(|error| error.to_string())?
    }
}
