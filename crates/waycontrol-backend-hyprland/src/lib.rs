//! Hyprland adapter for WayControl's compositor API.

use async_trait::async_trait;
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Duration;
use tokio::net::UnixStream;
use waycontrol_compositor::{
    CompositorBackend, CompositorCapabilities, CompositorInfo, CompositorKind,
};
use waycontrol_core::{Monitor, Window, Workspace, WorkspaceId};
use waycontrol_hyprland::{HyprlandClient, HyprlandError};

fn executable_file_exists(path: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;

    std::fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[derive(Clone)]
pub struct HyprlandAdapter {
    client: Option<HyprlandClient>,
    version: Arc<OnceLock<String>>,
    capabilities: Arc<RwLock<CompositorCapabilities>>,
}

impl HyprlandAdapter {
    pub fn can_discover_active_session() -> bool {
        HyprlandClient::from_env_or_discover().is_ok()
    }

    pub fn from_env() -> Result<Self, HyprlandError> {
        Ok(Self {
            client: Some(HyprlandClient::from_env()?),
            version: Arc::new(OnceLock::new()),
            capabilities: Arc::new(RwLock::new(CompositorCapabilities::none())),
        })
    }

    /// Keep the daemon available during login ordering races; each call retries discovery.
    pub fn from_env_or_unavailable() -> Self {
        match Self::from_env() {
            Ok(backend) => backend,
            Err(_) => Self {
                client: None,
                version: Arc::new(OnceLock::new()),
                capabilities: Arc::new(RwLock::new(CompositorCapabilities::none())),
            },
        }
    }

    /// Verify the selected instance's read-only IPC endpoints before exposing controls.
    pub async fn negotiate_capabilities(&self) -> bool {
        let probe = async {
            let client = self.client()?;
            client.version().await.map_err(|error| error.to_string())?;
            client
                .workspaces()
                .await
                .map_err(|error| error.to_string())?;
            client.clients().await.map_err(|error| error.to_string())?;
            client.monitors().await.map_err(|error| error.to_string())?;
            let events = client.connect_events().await.is_ok();
            Ok::<_, String>(events)
        };
        if let Ok(Ok(events_available)) = tokio::time::timeout(Duration::from_secs(2), probe).await
        {
            let mut capabilities = CompositorCapabilities::HYPRLAND;
            capabilities.events = events_available;
            capabilities.lock_screen = executable_file_exists("/usr/bin/hyprlock");
            *self
                .capabilities
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = capabilities;
            true
        } else {
            *self
                .capabilities
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner) =
                CompositorCapabilities::none();
            false
        }
    }

    /// Keep the daemon available if the compositor starts after the user service.
    pub fn retry_negotiation_until_ready(&self) {
        let adapter = self.clone();
        tokio::spawn(async move {
            loop {
                if adapter.capabilities().workspaces || adapter.negotiate_capabilities().await {
                    break;
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        });
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
        *self
            .capabilities
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
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
    async fn move_window_to_monitor(&self, id: &str, monitor: &str) -> Result<(), String> {
        self.client()?
            .move_window_to_monitor(id, monitor)
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
    async fn toggle_pinned(&self, id: &str) -> Result<(), String> {
        self.client()?
            .toggle_pinned(id)
            .await
            .map_err(|e| e.to_string())
    }
    async fn center_floating_window(&self, id: &str) -> Result<(), String> {
        self.client()?
            .center_floating_window(id)
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
    use std::{os::unix::net::UnixListener as StdUnixListener, time::SystemTime};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::UnixListener,
    };

    #[test]
    fn unnegotiated_adapter_does_not_advertise_compositor_operations() {
        let backend = HyprlandAdapter::from_env_or_unavailable();
        assert_eq!(backend.info().id, "hyprland");
        assert_eq!(backend.info().name, "Hyprland");
        assert_eq!(backend.capabilities(), CompositorCapabilities::none());
    }

    #[tokio::test]
    async fn stale_hyprland_socket_files_do_not_negotiate_capabilities() {
        let unique = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let socket_dir = std::env::temp_dir().join(format!(
            "waycontrol-hyprland-stale-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir(&socket_dir).unwrap();
        for name in [".socket.sock", ".socket2.sock"] {
            let listener = StdUnixListener::bind(socket_dir.join(name)).unwrap();
            drop(listener);
        }
        let backend = HyprlandAdapter {
            client: Some(HyprlandClient::at_socket_dir(socket_dir.clone())),
            version: Arc::new(OnceLock::new()),
            capabilities: Arc::new(RwLock::new(CompositorCapabilities::none())),
        };

        assert!(!backend.negotiate_capabilities().await);
        assert_eq!(backend.capabilities(), CompositorCapabilities::none());

        std::fs::remove_dir_all(socket_dir).unwrap();
    }

    #[tokio::test]
    async fn live_hyprland_ipc_negotiates_supported_operations() {
        let unique = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let socket_dir = std::env::temp_dir().join(format!(
            "waycontrol-hyprland-live-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir(&socket_dir).unwrap();
        let ipc = UnixListener::bind(socket_dir.join(".socket.sock")).unwrap();
        let events = UnixListener::bind(socket_dir.join(".socket2.sock")).unwrap();
        let ipc_task = tokio::spawn(async move {
            for _ in 0..10 {
                let (mut stream, _) = ipc.accept().await.unwrap();
                let mut request = [0_u8; 64];
                let size = stream.read(&mut request).await.unwrap();
                let response = match &request[..size] {
                    b"j/version" => r#"{"tag":"0.50.0"}"#,
                    b"j/workspaces" => "[]",
                    b"j/activeworkspace" => r#"{"id":1}"#,
                    b"j/clients" => "[]",
                    b"j/monitors" => "[]",
                    b"j/activewindow" => "{}",
                    other => panic!("unexpected Hyprland IPC request: {other:?}"),
                };
                stream.write_all(response.as_bytes()).await.unwrap();
                stream.shutdown().await.unwrap();
            }
        });
        let backend = HyprlandAdapter {
            client: Some(HyprlandClient::at_socket_dir(socket_dir.clone())),
            version: Arc::new(OnceLock::new()),
            capabilities: Arc::new(RwLock::new(CompositorCapabilities::none())),
        };

        assert!(backend.negotiate_capabilities().await);
        let capabilities = backend.capabilities();
        assert!(capabilities.workspaces);
        assert!(capabilities.windows);
        assert!(capabilities.monitor_power);
        assert!(capabilities.events);
        ipc_task.await.unwrap();
        drop(events);
        std::fs::remove_dir_all(socket_dir).unwrap();
    }
}
