//! Typed Hyprland IPC client. Socket paths are resolved explicitly from the caller's environment.

use serde_json::Value;
use std::{
    env,
    os::unix::fs::FileTypeExt,
    path::{Path as FsPath, PathBuf},
};
use thiserror::Error;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
};
use waycontrol_core::{Monitor, Window, Workspace, WorkspaceId};

#[derive(Debug, Error)]
pub enum HyprlandError {
    #[error("Hyprland instance is unavailable: {0}")]
    MissingInstance(String),
    #[error("Hyprland IPC I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Hyprland returned invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Hyprland response had an unexpected shape: {0}")]
    InvalidResponse(&'static str),
    #[error("Hyprland rejected the operation: {0}")]
    CommandRejected(String),
}

pub fn socket_dir(
    runtime_dir: Option<&str>,
    instance_signature: Option<&str>,
) -> Result<PathBuf, HyprlandError> {
    let runtime_dir = runtime_dir
        .map(str::to_owned)
        .or_else(|| env::var("XDG_RUNTIME_DIR").ok())
        .ok_or_else(|| HyprlandError::MissingInstance("XDG_RUNTIME_DIR is unset".into()))?;
    let hypr_dir = PathBuf::from(runtime_dir).join("hypr");
    let signature = instance_signature
        .map(str::to_owned)
        .or_else(|| env::var("HYPRLAND_INSTANCE_SIGNATURE").ok());
    if let Some(signature) = signature {
        if signature.is_empty()
            || signature.len() > 128
            || !signature
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        {
            return Err(HyprlandError::MissingInstance(
                "HYPRLAND_INSTANCE_SIGNATURE has an invalid format".into(),
            ));
        }
        return Ok(hypr_dir.join(signature));
    }
    discover_socket_dir(&hypr_dir)
}

fn discover_socket_dir(hypr_dir: &FsPath) -> Result<PathBuf, HyprlandError> {
    let entries = std::fs::read_dir(hypr_dir).map_err(|error| {
        HyprlandError::MissingInstance(format!(
            "cannot enumerate Hyprland instances in {}: {error}",
            hypr_dir.display()
        ))
    })?;
    let mut active = Vec::new();
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let candidate = entry.path();
        let control = std::fs::symlink_metadata(candidate.join(".socket.sock"));
        let events = std::fs::symlink_metadata(candidate.join(".socket2.sock"));
        if matches!(control, Ok(metadata) if metadata.file_type().is_socket())
            && matches!(events, Ok(metadata) if metadata.file_type().is_socket())
        {
            active.push(candidate);
        }
    }
    match active.len() {
        0 => Err(HyprlandError::MissingInstance(
            "no active Hyprland IPC sockets were found".into(),
        )),
        1 => Ok(active.remove(0)),
        _ => Err(HyprlandError::MissingInstance(
            "multiple active Hyprland instances were found; set HYPRLAND_INSTANCE_SIGNATURE to select the intended session".into(),
        )),
    }
}

#[derive(Clone, Debug)]
pub struct HyprlandClient {
    socket_dir: PathBuf,
}

impl HyprlandClient {
    pub fn from_env() -> Result<Self, HyprlandError> {
        Ok(Self {
            socket_dir: socket_dir(None, None)?,
        })
    }

    pub fn from_env_or_discover() -> Result<Self, HyprlandError> {
        let runtime_dir = env::var("XDG_RUNTIME_DIR")
            .map_err(|_| HyprlandError::MissingInstance("XDG_RUNTIME_DIR is unset".into()))?;
        if let Ok(signature) = env::var("HYPRLAND_INSTANCE_SIGNATURE") {
            let selected = socket_dir(Some(&runtime_dir), Some(&signature))?;
            if has_ipc_sockets(&selected) {
                return Ok(Self {
                    socket_dir: selected,
                });
            }
        }
        Ok(Self {
            socket_dir: discover_socket_dir(&PathBuf::from(runtime_dir).join("hypr"))?,
        })
    }

    pub fn has_ipc_sockets(&self) -> bool {
        has_ipc_sockets(&self.socket_dir)
    }
    pub fn at_socket_dir(socket_dir: PathBuf) -> Self {
        Self { socket_dir }
    }

    pub async fn connect_events(&self) -> Result<UnixStream, HyprlandError> {
        Ok(UnixStream::connect(self.socket_dir.join(".socket2.sock")).await?)
    }

    async fn request(&self, endpoint: &str) -> Result<String, HyprlandError> {
        let socket = self.socket_dir.join(".socket.sock");
        let mut stream = UnixStream::connect(socket).await?;
        stream.write_all(endpoint.as_bytes()).await?;
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await?;
        Ok(String::from_utf8_lossy(&response).into_owned())
    }

    pub async fn version(&self) -> Result<String, HyprlandError> {
        parse_hyprland_version(&self.request("j/version").await?)
    }

    pub async fn workspaces(&self) -> Result<Vec<Workspace>, HyprlandError> {
        let workspaces: Vec<Value> = serde_json::from_str(&self.request("j/workspaces").await?)?;
        let active = serde_json::from_str::<Value>(&self.request("j/activeworkspace").await?)?
            .get("id")
            .and_then(Value::as_i64);
        let clients = self.clients().await?;
        workspaces
            .into_iter()
            .map(|w| {
                let id = w
                    .get("id")
                    .and_then(Value::as_i64)
                    .ok_or(HyprlandError::InvalidResponse("workspace id"))?;
                Ok(Workspace {
                    id: WorkspaceId::from(id),
                    name: w
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned(),
                    monitor: w.get("monitor").and_then(Value::as_str).map(str::to_owned),
                    window_count: clients
                        .iter()
                        .filter(|c| {
                            c.workspace_id.as_ref().and_then(WorkspaceId::as_i64) == Some(id)
                        })
                        .count() as u64,
                    active: Some(id) == active,
                })
            })
            .collect()
    }

    pub async fn clients(&self) -> Result<Vec<Window>, HyprlandError> {
        let clients: Vec<Value> = serde_json::from_str(&self.request("j/clients").await?)?;
        let monitors: Vec<Value> = serde_json::from_str(&self.request("j/monitors").await?)?;
        let monitor_names = monitors
            .iter()
            .filter_map(|monitor| {
                Some((
                    monitor.get("id")?.as_i64()?,
                    monitor.get("name")?.as_str()?.to_owned(),
                ))
            })
            .collect::<std::collections::HashMap<_, _>>();
        let active_address = serde_json::from_str::<Value>(&self.request("j/activewindow").await?)?
            .get("address")
            .and_then(Value::as_str)
            .map(str::to_owned);
        clients
            .into_iter()
            .map(|c| {
                let address = c
                    .get("address")
                    .and_then(Value::as_str)
                    .ok_or(HyprlandError::InvalidResponse("window address"))?
                    .to_owned();
                Ok(Window {
                    focused: active_address.as_deref() == Some(&address),
                    address,
                    title: c
                        .get("title")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned(),
                    app_id: c
                        .get("class")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned(),
                    workspace_id: c
                        .get("workspace")
                        .and_then(|v| v.get("id"))
                        .and_then(Value::as_i64)
                        .map(WorkspaceId::from),
                    monitor: c
                        .get("monitor")
                        .and_then(Value::as_i64)
                        .and_then(|id| monitor_names.get(&id).cloned()),
                    floating: Some(c.get("floating").and_then(Value::as_bool).unwrap_or(false)),
                    pinned: c.get("pinned").and_then(Value::as_bool),
                    fullscreen: c.get("fullscreen").and_then(Value::as_i64).unwrap_or(0) != 0,
                })
            })
            .collect()
    }

    pub async fn monitors(&self) -> Result<Vec<Monitor>, HyprlandError> {
        let monitors: Vec<Value> = serde_json::from_str(&self.request("j/monitors").await?)?;
        monitors
            .into_iter()
            .map(|monitor| {
                let name = monitor
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or(HyprlandError::InvalidResponse("monitor name"))?
                    .to_owned();
                Ok(Monitor {
                    name,
                    dpms_enabled: monitor.get("dpmsStatus").and_then(Value::as_bool),
                })
            })
            .collect()
    }

    pub async fn set_monitor_power(&self, name: &str, enabled: bool) -> Result<(), HyprlandError> {
        if !valid_monitor_name(name) {
            return Err(HyprlandError::InvalidResponse("invalid monitor name"));
        }
        if !self
            .monitors()
            .await?
            .iter()
            .any(|monitor| monitor.name == name)
        {
            return Err(HyprlandError::CommandRejected(
                "monitor no longer exists".into(),
            ));
        }
        let action = if enabled { "enable" } else { "disable" };
        self.dispatch_compatible(
            &format!("hl.dispatch(hl.dsp.dpms({{ action = \"{action}\", monitor = \"{name}\" }}))"),
            &format!(
                "dispatch dpms {},{name}",
                if enabled { "on" } else { "off" }
            ),
        )
        .await
    }

    pub async fn switch_workspace(&self, workspace_id: i64) -> Result<(), HyprlandError> {
        if !(1..=i64::from(i32::MAX)).contains(&workspace_id) {
            return Err(HyprlandError::InvalidResponse(
                "workspace id outside supported range",
            ));
        }
        self.dispatch_compatible(
            &format!("hl.dispatch(hl.dsp.focus({{ workspace = \"{workspace_id}\" }}))"),
            &format!("dispatch workspace {workspace_id}"),
        )
        .await
    }

    pub async fn focus_window(&self, address: &str) -> Result<(), HyprlandError> {
        if !valid_window_address(address) {
            return Err(HyprlandError::InvalidResponse("invalid window address"));
        }
        if !self
            .clients()
            .await?
            .iter()
            .any(|window| window.address == address)
        {
            return Err(HyprlandError::CommandRejected(
                "window no longer exists".into(),
            ));
        }
        self.dispatch_compatible(
            &format!("hl.dispatch(hl.dsp.focus({{ window = \"address:{address}\" }}))"),
            &format!("dispatch focuswindow address:{address}"),
        )
        .await
    }

    pub async fn move_window(&self, address: &str, workspace_id: i64) -> Result<(), HyprlandError> {
        self.require_window(address).await?;
        if !(1..=i64::from(i32::MAX)).contains(&workspace_id) {
            return Err(HyprlandError::InvalidResponse(
                "workspace id outside supported range",
            ));
        }
        self.dispatch_compatible(
            &format!("hl.dispatch(hl.dsp.window.move({{ workspace = \"{workspace_id}\", window = \"address:{address}\", follow = false }}))"),
            &format!("dispatch movetoworkspacesilent {workspace_id},address:{address}"),
        ).await
    }

    pub async fn move_window_to_monitor(
        &self,
        address: &str,
        monitor: &str,
    ) -> Result<(), HyprlandError> {
        if !valid_monitor_name(monitor) {
            return Err(HyprlandError::InvalidResponse("invalid monitor name"));
        }
        self.require_window(address).await?;
        if !self
            .monitors()
            .await?
            .iter()
            .any(|item| item.name == monitor)
        {
            return Err(HyprlandError::CommandRejected(
                "monitor no longer exists".into(),
            ));
        }

        let lua = format!(
            "hl.dispatch(hl.dsp.window.move({{ monitor = \"{monitor}\", window = \"address:{address}\", follow = false }}))"
        );
        let response = self.request(&format!("eval {lua}")).await?;
        if response.trim() == "ok" {
            return Ok(());
        }
        let response_lower = response.to_ascii_lowercase();
        if response_lower.contains("unknown")
            || response_lower.contains("not supported")
            || response_lower.contains("only supported with")
        {
            let focused = self
                .request(&format!("dispatch focuswindow address:{address}"))
                .await?;
            if focused.trim() != "ok" {
                return Err(HyprlandError::CommandRejected(focused.trim().to_owned()));
            }
            let moved = self
                .request(&format!("dispatch movewindow mon:{monitor} silent"))
                .await?;
            if moved.trim() == "ok" {
                return Ok(());
            }
            return Err(HyprlandError::CommandRejected(moved.trim().to_owned()));
        }
        Err(HyprlandError::CommandRejected(response.trim().to_owned()))
    }

    pub async fn toggle_floating(&self, address: &str) -> Result<(), HyprlandError> {
        self.require_window(address).await?;
        self.dispatch_compatible(
            &format!("hl.dispatch(hl.dsp.window.float({{ action = \"toggle\", window = \"address:{address}\" }}))"),
            &format!("dispatch togglefloating address:{address}"),
        ).await
    }

    pub async fn toggle_fullscreen(&self, address: &str) -> Result<(), HyprlandError> {
        self.require_window(address).await?;
        self.dispatch_compatible(
            &format!("hl.dispatch(hl.dsp.window.fullscreen({{ action = \"toggle\", window = \"address:{address}\" }}))"),
            &format!("dispatch fullscreen 0,address:{address}"),
        ).await
    }

    pub async fn close_window(&self, address: &str) -> Result<(), HyprlandError> {
        self.require_window(address).await?;
        self.dispatch_compatible(
            &format!("hl.dispatch(hl.dsp.window.close({{ window = \"address:{address}\" }}))"),
            &format!("dispatch closewindow address:{address}"),
        )
        .await
    }

    pub async fn toggle_pinned(&self, address: &str) -> Result<(), HyprlandError> {
        let window = self
            .clients()
            .await?
            .into_iter()
            .find(|window| window.address == address)
            .ok_or_else(|| HyprlandError::CommandRejected("window no longer exists".into()))?;
        if window.floating != Some(true) {
            return Err(HyprlandError::CommandRejected(
                "Hyprland can pin only floating windows".into(),
            ));
        }
        self.dispatch_compatible(
            &format!("hl.dispatch(hl.dsp.window.pin({{ action = \"toggle\", window = \"address:{address}\" }}))"),
            &format!("dispatch pin address:{address}"),
        )
        .await
    }

    async fn require_window(&self, address: &str) -> Result<(), HyprlandError> {
        if !valid_window_address(address) {
            return Err(HyprlandError::InvalidResponse("invalid window address"));
        }
        if !self
            .clients()
            .await?
            .iter()
            .any(|window| window.address == address)
        {
            return Err(HyprlandError::CommandRejected(
                "window no longer exists".into(),
            ));
        }
        Ok(())
    }

    async fn dispatch_compatible(
        &self,
        lua_expression: &str,
        legacy_dispatch: &str,
    ) -> Result<(), HyprlandError> {
        let lua_response = self.request(&format!("eval {lua_expression}")).await?;
        if lua_response.trim() == "ok" {
            Ok(())
        } else {
            let response_lower = lua_response.to_ascii_lowercase();
            if response_lower.contains("unknown")
                || response_lower.contains("not supported")
                || response_lower.contains("only supported with")
            {
                let legacy_response = self.request(legacy_dispatch).await?;
                if legacy_response.trim() == "ok" {
                    return Ok(());
                }
                return Err(HyprlandError::CommandRejected(
                    legacy_response.trim().to_owned(),
                ));
            }
            Err(HyprlandError::CommandRejected(
                lua_response.trim().to_owned(),
            ))
        }
    }
}

fn parse_hyprland_version(response: &str) -> Result<String, HyprlandError> {
    let value: Value = serde_json::from_str(response)?;
    ["tag", "version"]
        .into_iter()
        .filter_map(|field| value.get(field).and_then(Value::as_str))
        .map(str::trim)
        .find(|version| !version.is_empty())
        .map(str::to_owned)
        .ok_or(HyprlandError::InvalidResponse("version"))
}

fn has_ipc_sockets(path: &FsPath) -> bool {
    [".socket.sock", ".socket2.sock"].iter().all(|name| {
        std::fs::symlink_metadata(path.join(name))
            .is_ok_and(|metadata| metadata.file_type().is_socket())
    })
}

fn valid_window_address(address: &str) -> bool {
    address.starts_with("0x")
        && address.len() > 2
        && address[2..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_monitor_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_dir_requires_explicit_hyprland_instance() {
        assert_eq!(
            socket_dir(Some("/run/user/1000"), Some("abc")).unwrap(),
            PathBuf::from("/run/user/1000/hypr/abc")
        );
        assert!(socket_dir(Some("/run/user/1000"), Some("../other")).is_err());
    }

    #[test]
    fn hyprland_version_parser_prefers_tag_and_falls_back_to_version() {
        assert_eq!(
            parse_hyprland_version(r#"{"tag":"v0.57.0","branch":"main"}"#).unwrap(),
            "v0.57.0",
        );
        assert_eq!(
            parse_hyprland_version(r#"{"version":"0.57.0"}"#).unwrap(),
            "0.57.0",
        );
        assert!(parse_hyprland_version(r#"{"branch":"main"}"#).is_err());
    }

    #[tokio::test]
    async fn client_reads_hyprland_version_from_the_target_instance() {
        use tokio::{io::AsyncReadExt, net::UnixListener};

        let dir = std::env::temp_dir().join(format!("waycontrol-version-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let listener = UnixListener::bind(dir.join(".socket.sock")).unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0; 64];
            let size = stream.read(&mut request).await.unwrap();
            assert_eq!(&request[..size], b"j/version");
            stream
                .write_all(br#"{"tag":"v0.57.0","branch":"main"}"#)
                .await
                .unwrap();
        });

        let version = HyprlandClient::at_socket_dir(dir.clone())
            .version()
            .await
            .unwrap();
        server.await.unwrap();
        std::fs::remove_dir_all(dir).unwrap();
        assert_eq!(version, "v0.57.0");
    }

    #[tokio::test]
    async fn instance_discovery_selects_only_a_unique_active_socket_pair() {
        use tokio::net::UnixListener;

        let dir = std::env::temp_dir().join(format!("waycontrol-discovery-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(discover_socket_dir(&dir).is_err());

        let first = dir.join("instance-one");
        std::fs::create_dir(&first).unwrap();
        let _control_one = UnixListener::bind(first.join(".socket.sock")).unwrap();
        let _events_one = UnixListener::bind(first.join(".socket2.sock")).unwrap();
        assert_eq!(discover_socket_dir(&dir).unwrap(), first);

        let second = dir.join("instance-two");
        std::fs::create_dir(&second).unwrap();
        let _control_two = UnixListener::bind(second.join(".socket.sock")).unwrap();
        let _events_two = UnixListener::bind(second.join(".socket2.sock")).unwrap();
        assert!(discover_socket_dir(&dir).is_err());
        drop((_control_one, _events_one, _control_two, _events_two));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn malformed_ipc_json_is_rejected() {
        assert!(serde_json::from_str::<Vec<Value>>("not json").is_err());
    }

    #[tokio::test]
    async fn workspace_query_counts_windows_and_marks_focused_workspace() {
        use tokio::{io::AsyncReadExt, net::UnixListener};

        let dir = std::env::temp_dir().join(format!("waycontrol-ipc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let listener = UnixListener::bind(dir.join(".socket.sock")).unwrap();
        let server = tokio::spawn(async move {
            for (expected, response) in [
                (
                    "j/workspaces",
                    r#"[{"id":1,"name":"1","monitor":"DP-1"},{"id":3,"name":"3","monitor":"HDMI-A-1"}]"#,
                ),
                (
                    "j/activeworkspace",
                    r#"{"id":3,"name":"3","monitor":"HDMI-A-1"}"#,
                ),
                (
                    "j/clients",
                    r#"[{"address":"0xabc","title":"Terminal","class":"kitty","workspace":{"id":1},"monitor":0,"floating":false,"fullscreen":0}]"#,
                ),
                ("j/monitors", r#"[{"id":0,"name":"DP-1"}]"#),
                ("j/activewindow", r#"{"address":"0xabc"}"#),
            ] {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = [0; 64];
                let size = stream.read(&mut request).await.unwrap();
                assert_eq!(&request[..size], expected.as_bytes());
                stream.write_all(response.as_bytes()).await.unwrap();
            }
        });
        let result = HyprlandClient::at_socket_dir(dir.clone())
            .workspaces()
            .await
            .unwrap();
        server.await.unwrap();
        std::fs::remove_dir_all(dir).unwrap();

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].window_count, 1);
        assert!(!result[0].active);
        assert_eq!(result[1].window_count, 0);
        assert!(result[1].active);
    }

    #[tokio::test]
    async fn client_query_resolves_monitor_and_focus_state() {
        use tokio::{io::AsyncReadExt, net::UnixListener};

        let dir = std::env::temp_dir().join(format!("waycontrol-clients-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let listener = UnixListener::bind(dir.join(".socket.sock")).unwrap();
        let server = tokio::spawn(async move {
            for (expected, response) in [
                (
                    "j/clients",
                    r#"[{"address":"0xabc","title":"Terminal","class":"kitty","workspace":{"id":1},"monitor":0,"floating":true,"fullscreen":1,"pinned":true}]"#,
                ),
                ("j/monitors", r#"[{"id":0,"name":"DP-3"}]"#),
                ("j/activewindow", r#"{"address":"0xabc"}"#),
            ] {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = [0; 64];
                let size = stream.read(&mut request).await.unwrap();
                assert_eq!(&request[..size], expected.as_bytes());
                stream.write_all(response.as_bytes()).await.unwrap();
            }
        });
        let windows = HyprlandClient::at_socket_dir(dir.clone())
            .clients()
            .await
            .unwrap();
        server.await.unwrap();
        std::fs::remove_dir_all(dir).unwrap();

        assert_eq!(windows[0].monitor.as_deref(), Some("DP-3"));
        assert_eq!(windows[0].floating, Some(true));
        assert_eq!(windows[0].pinned, Some(true));
        assert!(windows[0].fullscreen);
        assert!(windows[0].focused);
    }

    #[tokio::test]
    async fn pinning_requires_floating_window_and_uses_hyprland_dispatcher() {
        use tokio::{io::AsyncReadExt, net::UnixListener};

        let dir = std::env::temp_dir().join(format!("waycontrol-pin-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let listener = UnixListener::bind(dir.join(".socket.sock")).unwrap();
        let server = tokio::spawn(async move {
            for (expected, response) in [
                (
                    "j/clients",
                    r#"[{"address":"0xabc","floating":true,"pinned":false}]"#,
                ),
                ("j/monitors", "[]"),
                ("j/activewindow", "{}"),
                (
                    "eval hl.dispatch(hl.dsp.window.pin({ action = \"toggle\", window = \"address:0xabc\" }))",
                    "ok",
                ),
            ] {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = [0; 256];
                let size = stream.read(&mut request).await.unwrap();
                assert_eq!(&request[..size], expected.as_bytes());
                stream.write_all(response.as_bytes()).await.unwrap();
            }
        });

        HyprlandClient::at_socket_dir(dir.clone())
            .toggle_pinned("0xabc")
            .await
            .unwrap();
        server.await.unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn pinning_rejects_tiled_window_without_dispatch() {
        use tokio::{io::AsyncReadExt, net::UnixListener};

        let dir = std::env::temp_dir().join(format!("waycontrol-pin-tiled-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let listener = UnixListener::bind(dir.join(".socket.sock")).unwrap();
        let server = tokio::spawn(async move {
            for (expected, response) in [
                ("j/clients", r#"[{"address":"0xabc","floating":false}]"#),
                ("j/monitors", "[]"),
                ("j/activewindow", "{}"),
            ] {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = [0; 256];
                let size = stream.read(&mut request).await.unwrap();
                assert_eq!(&request[..size], expected.as_bytes());
                stream.write_all(response.as_bytes()).await.unwrap();
            }
        });

        let result = HyprlandClient::at_socket_dir(dir.clone())
            .toggle_pinned("0xabc")
            .await;
        assert!(result.unwrap_err().to_string().contains("floating"));
        server.await.unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn moving_a_window_to_monitor_uses_an_addressed_validated_dispatcher() {
        use tokio::{io::AsyncReadExt, net::UnixListener};

        let dir = std::env::temp_dir().join(format!("waycontrol-monitor-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let listener = UnixListener::bind(dir.join(".socket.sock")).unwrap();
        let server = tokio::spawn(async move {
            for (expected, response) in [
                ("j/clients", r#"[{"address":"0xabc","floating":false}]"#),
                ("j/monitors", "[]"),
                ("j/activewindow", "{}"),
                ("j/monitors", r#"[{"name":"DP-1","id":0}]"#),
                (
                    "eval hl.dispatch(hl.dsp.window.move({ monitor = \"DP-1\", window = \"address:0xabc\", follow = false }))",
                    "ok",
                ),
            ] {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = [0; 256];
                let size = stream.read(&mut request).await.unwrap();
                assert_eq!(&request[..size], expected.as_bytes());
                stream.write_all(response.as_bytes()).await.unwrap();
            }
        });

        HyprlandClient::at_socket_dir(dir.clone())
            .move_window_to_monitor("0xabc", "DP-1")
            .await
            .unwrap();
        server.await.unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn ipc_client_recovers_after_socket_disconnect_and_recreation() {
        use tokio::{io::AsyncReadExt, net::UnixListener};

        async fn serve_workspaces(listener: UnixListener) {
            for (expected, response) in [
                ("j/workspaces", r#"[{"id":2,"name":"2","monitor":"DP-1"}]"#),
                (
                    "j/activeworkspace",
                    r#"{"id":2,"name":"2","monitor":"DP-1"}"#,
                ),
                ("j/clients", "[]"),
                ("j/monitors", r#"[{"id":0,"name":"DP-1"}]"#),
                ("j/activewindow", "{}"),
            ] {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = [0; 64];
                let size = stream.read(&mut request).await.unwrap();
                assert_eq!(&request[..size], expected.as_bytes());
                stream.write_all(response.as_bytes()).await.unwrap();
            }
        }

        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "waycontrol-reconnect-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let socket_path = dir.join(".socket.sock");
        let client = HyprlandClient::at_socket_dir(dir.clone());

        let first_listener = UnixListener::bind(&socket_path).unwrap();
        let first_server = tokio::spawn(serve_workspaces(first_listener));
        assert_eq!(client.workspaces().await.unwrap()[0].id, 2.into());
        first_server.await.unwrap();

        std::fs::remove_file(&socket_path).unwrap();
        assert!(client.workspaces().await.is_err());

        let recovered_listener = UnixListener::bind(&socket_path).unwrap();
        let recovered_server = tokio::spawn(serve_workspaces(recovered_listener));
        assert_eq!(client.workspaces().await.unwrap()[0].id, 2.into());
        recovered_server.await.unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn workspace_switch_uses_validated_numeric_dispatch() {
        use tokio::{io::AsyncReadExt, net::UnixListener};

        let dir = std::env::temp_dir().join(format!("waycontrol-switch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let listener = UnixListener::bind(dir.join(".socket.sock")).unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0; 64];
            let size = stream.read(&mut request).await.unwrap();
            assert_eq!(
                &request[..size],
                b"eval hl.dispatch(hl.dsp.focus({ workspace = \"3\" }))"
            );
            stream.write_all(b"ok").await.unwrap();
        });
        HyprlandClient::at_socket_dir(dir.clone())
            .switch_workspace(3)
            .await
            .unwrap();
        server.await.unwrap();
        std::fs::remove_dir_all(dir).unwrap();
        assert!(
            HyprlandClient::at_socket_dir(std::env::temp_dir())
                .switch_workspace(0)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn moving_a_window_uses_silent_lua_and_legacy_dispatchers() {
        use tokio::{io::AsyncReadExt, net::UnixListener};

        let dir = std::env::temp_dir().join(format!("waycontrol-move-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let listener = UnixListener::bind(dir.join(".socket.sock")).unwrap();
        let server = tokio::spawn(async move {
            for (expected, response) in [
                (
                    "j/clients",
                    r#"[{"address":"0xabc","title":"Terminal","class":"kitty","workspace":{"id":1},"monitor":0,"floating":false,"fullscreen":0}]"#,
                ),
                ("j/monitors", "[]"),
                ("j/activewindow", r#"{"address":"0xabc"}"#),
            ] {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = [0; 256];
                let size = stream.read(&mut request).await.unwrap();
                assert_eq!(&request[..size], expected.as_bytes());
                stream.write_all(response.as_bytes()).await.unwrap();
            }

            for (expected, response) in [
                (
                    "eval hl.dispatch(hl.dsp.window.move({ workspace = \"3\", window = \"address:0xabc\", follow = false }))",
                    "unknown command",
                ),
                ("dispatch movetoworkspacesilent 3,address:0xabc", "ok"),
            ] {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = [0; 256];
                let size = stream.read(&mut request).await.unwrap();
                assert_eq!(&request[..size], expected.as_bytes());
                stream.write_all(response.as_bytes()).await.unwrap();
            }
        });

        HyprlandClient::at_socket_dir(dir.clone())
            .move_window("0xabc", 3)
            .await
            .unwrap();
        server.await.unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn window_addresses_are_restricted_to_hexadecimal_identifiers() {
        assert!(valid_window_address("0xabc123"));
        assert!(!valid_window_address("0xabc;exec reboot"));
        assert!(!valid_window_address("activewindow"));
    }
}
