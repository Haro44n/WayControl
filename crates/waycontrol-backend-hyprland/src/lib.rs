//! Hyprland adapter for WayControl's compositor API.

use async_trait::async_trait;
use std::sync::{Arc, OnceLock};
use tokio::net::UnixStream;
use waycontrol_compositor::{
    CompositorBackend, CompositorCapabilities, CompositorInfo, CompositorKind,
};
use waycontrol_core::{Monitor, Window, Workspace, WorkspaceId};
use waycontrol_hyprland::{HyprlandClient, HyprlandError};

#[derive(Clone)]
pub struct HyprlandAdapter {
    client: Option<HyprlandClient>,
    version: Arc<OnceLock<String>>,
}

impl HyprlandAdapter {
    pub fn can_discover_active_session() -> bool {
        HyprlandClient::from_env_or_discover().is_ok()
    }

    pub fn from_env() -> Result<Self, HyprlandError> {
        Ok(Self {
            client: Some(HyprlandClient::from_env()?),
            version: Arc::new(OnceLock::new()),
        })
    }

    /// Keep the daemon available during login ordering races; each call retries discovery.
    pub fn from_env_or_unavailable() -> Self {
        match Self::from_env() {
            Ok(backend) => backend,
            Err(_) => Self {
                client: None,
                version: Arc::new(OnceLock::new()),
            },
        }
    }

    fn client(&self) -> Result<HyprlandClient, String> {
        if let Some(client) = self
            .client
            .as_ref()
            .filter(|client| client.has_ipc_sockets())
        {
            return Ok(client.clone());
        }
        HyprlandClient::from_env_or_discover().map_err(|error| error.to_string())
    }

    async fn version(&self) -> Option<String> {
        if let Some(version) = self.version.get() {
            return Some(version.clone());
        }
        let version = self.client().ok()?.version().await.ok()?;
        let _ = self.version.set(version.clone());
        Some(version)
    }
}

#[async_trait]
impl CompositorBackend for HyprlandAdapter {
    fn info(&self) -> CompositorInfo {
        CompositorInfo::detected(CompositorKind::Hyprland)
    }

    fn capabilities(&self) -> CompositorCapabilities {
        CompositorCapabilities::HYPRLAND
    }

    async fn version(&self) -> Option<String> {
        HyprlandAdapter::version(self).await
    }

    async fn workspaces(&self) -> Result<Vec<Workspace>, String> {
        self.client()?.workspaces().await.map_err(|e| e.to_string())
    }
    async fn windows(&self) -> Result<Vec<Window>, String> {
        self.client()?.clients().await.map_err(|e| e.to_string())
    }
    async fn switch_workspace(&self, id: &WorkspaceId) -> Result<(), String> {
        let id = id
            .as_i64()
            .ok_or_else(|| "Hyprland workspace IDs must be numeric".to_owned())?;
        self.client()?
            .switch_workspace(id)
            .await
            .map_err(|e| e.to_string())
    }
    async fn focus_window(&self, id: &str) -> Result<(), String> {
        self.client()?
            .focus_window(id)
            .await
            .map_err(|e| e.to_string())
    }
    async fn move_window(&self, id: &str, workspace_id: &WorkspaceId) -> Result<(), String> {
        let workspace_id = workspace_id
            .as_i64()
            .ok_or_else(|| "Hyprland workspace IDs must be numeric".to_owned())?;
        self.client()?
            .move_window(id, workspace_id)
            .await
            .map_err(|e| e.to_string())
    }
    async fn toggle_floating(&self, id: &str) -> Result<(), String> {
        self.client()?
            .toggle_floating(id)
            .await
            .map_err(|e| e.to_string())
    }
    async fn toggle_fullscreen(&self, id: &str) -> Result<(), String> {
        self.client()?
            .toggle_fullscreen(id)
            .await
            .map_err(|e| e.to_string())
    }
    async fn close_window(&self, id: &str) -> Result<(), String> {
        self.client()?
            .close_window(id)
            .await
            .map_err(|e| e.to_string())
    }
    async fn monitors(&self) -> Result<Vec<Monitor>, String> {
        self.client()?.monitors().await.map_err(|e| e.to_string())
    }
    async fn set_monitor_power(&self, name: &str, enabled: bool) -> Result<(), String> {
        self.client()?
            .set_monitor_power(name, enabled)
            .await
            .map_err(|e| e.to_string())
    }
    async fn lock_screen(&self) -> Result<(), String> {
        tokio::process::Command::new("/usr/bin/hyprlock")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("could not start /usr/bin/hyprlock: {error}"))
    }
    async fn event_stream(&self) -> Result<UnixStream, String> {
        self.client()?
            .connect_events()
            .await
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapter_reports_hyprland_identity_and_supported_operations() {
        let backend = HyprlandAdapter::from_env_or_unavailable();
        assert_eq!(backend.info().id, "hyprland");
        assert_eq!(backend.info().name, "Hyprland");
        let capabilities = backend.capabilities();
        assert!(capabilities.workspaces);
        assert!(capabilities.windows);
        assert!(capabilities.events);
    }
}
