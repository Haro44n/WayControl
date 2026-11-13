//! Niri JSON-over-Unix-socket compositor adapter.

use async_trait::async_trait;
use serde_json::{Value, json};
use std::{env, path::PathBuf};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
};
use waycontrol_compositor::{
    CompositorBackend, CompositorCapabilities, CompositorInfo, CompositorKind,
};
use waycontrol_core::{Monitor, Window, Workspace, WorkspaceId};

const MAX_REQUEST_BYTES: usize = 64 * 1024;
const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
const MAX_EVENT_BYTES: usize = 7 * 1024;

#[derive(Clone, Debug)]
pub struct NiriAdapter {
    socket: Option<PathBuf>,
}

impl NiriAdapter {
    pub fn from_env() -> Result<Self, String> {
        let socket = env::var_os("NIRI_SOCKET")
            .map(PathBuf::from)
            .ok_or_else(|| "NIRI_SOCKET is not set".to_owned())?;
        Ok(Self {
            socket: Some(socket),
        })
    }

    pub fn from_env_or_unavailable() -> Self {
        Self::from_env().unwrap_or(Self { socket: None })
    }

    async fn connection(&self) -> Result<UnixStream, String> {
        let path = self
            .socket
            .as_ref()
            .ok_or_else(|| "NIRI_SOCKET is unavailable".to_owned())?;
        UnixStream::connect(path)
            .await
            .map_err(|error| format!("could not connect to Niri IPC socket: {error}"))
    }

    async fn send_request(stream: &mut UnixStream, request: &Value) -> Result<(), String> {
        let mut bytes = serde_json::to_vec(request).map_err(|error| error.to_string())?;
        if bytes.len() > MAX_REQUEST_BYTES {
            return Err("Niri IPC request exceeds size limit".into());
        }
        bytes.push(b'\n');
        stream
            .write_all(&bytes)
            .await
            .map_err(|error| error.to_string())
    }

    async fn request(&self, request: Value) -> Result<Value, String> {
        let mut stream = self.connection().await?;
        Self::send_request(&mut stream, &request).await?;
        stream.shutdown().await.map_err(|error| error.to_string())?;
        read_reply(&mut stream).await
    }

    async fn action(&self, action: Value) -> Result<(), String> {
        let response = self.request(json!({"Action": action})).await?;
        if response == "Handled" {
            Ok(())
        } else {
            Err("Niri did not confirm the compositor action".into())
        }
    }

    fn window_id(&self, id: &str) -> Result<u64, String> {
        id.strip_prefix("niri:")
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| *value > 0)
            .ok_or_else(|| "invalid Niri window ID".to_owned())
    }

    fn workspace_id(id: &WorkspaceId) -> Result<u64, String> {
        id.as_str()
            .parse::<u64>()
            .ok()
            .filter(|value| *value > 0)
            .ok_or_else(|| "invalid Niri workspace ID".to_owned())
    }

    async fn workspace_values(&self) -> Result<Vec<Workspace>, String> {
        let response = self.request(json!("Workspaces")).await?;
        let entries = response
            .get("Workspaces")
            .and_then(Value::as_array)
            .ok_or_else(|| "invalid Niri workspace response".to_owned())?;
        let mut workspaces = entries
            .iter()
            .map(|entry| {
                let id = entry
                    .get("id")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| "Niri workspace response has no valid ID".to_owned())?;
                let idx = entry.get("idx").and_then(Value::as_u64).unwrap_or_default();
                Ok(Workspace {
                    id: WorkspaceId::new(id.to_string()),
                    name: entry
                        .get("name")
                        .and_then(Value::as_str)
                        .filter(|name| !name.is_empty())
                        .map(str::to_owned)
                        .unwrap_or_else(|| format!("Workspace {idx}")),
                    monitor: entry
                        .get("output")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    window_count: 0,
                    active: entry
                        .get("is_focused")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        for window in self.window_values().await? {
            if let Some(id) = window.workspace_id.as_ref() {
                if let Some(workspace) = workspaces.iter_mut().find(|item| &item.id == id) {
                    workspace.window_count += 1;
                }
            }
        }
        Ok(workspaces)
    }

    async fn window_values(&self) -> Result<Vec<Window>, String> {
        let response = self.request(json!("Windows")).await?;
        response
            .get("Windows")
            .and_then(Value::as_array)
            .ok_or_else(|| "invalid Niri window response".to_owned())?
            .iter()
            .map(|entry| {
                let id = entry
                    .get("id")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| "Niri window response has no valid ID".to_owned())?;
                Ok(Window {
                    address: format!("niri:{id}"),
                    title: entry
                        .get("title")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    app_id: entry
                        .get("app_id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    workspace_id: entry
                        .get("workspace_id")
                        .and_then(Value::as_u64)
                        .map(|id| WorkspaceId::new(id.to_string())),
                    monitor: None,
                    floating: Some(
                        entry
                            .get("is_floating")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                    ),
                    pinned: None,
                    icon_png_base64: None,
                    fullscreen: entry
                        .get("is_fullscreen")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    focused: entry
                        .get("is_focused")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                })
            })
            .collect()
    }

    async fn monitor_values(&self) -> Result<Vec<Monitor>, String> {
        let response = self.request(json!("Outputs")).await?;
        let outputs = response
            .get("Outputs")
            .and_then(Value::as_object)
            .ok_or_else(|| "invalid Niri output response".to_owned())?;
        Ok(outputs
            .keys()
            .map(|name| Monitor {
                name: name.clone(),
                dpms_enabled: None,
            })
            .collect())
    }
}

async fn read_line_bounded(stream: &mut UnixStream, max: usize) -> Result<Vec<u8>, String> {
    let mut line = Vec::new();
    loop {
        let byte = stream.read_u8().await.map_err(|error| error.to_string())?;
        if byte == b'\n' {
            return Ok(line);
        }
        if line.len() >= max {
            return Err("Niri IPC line exceeds size limit".into());
        }
        line.push(byte);
    }
}

async fn read_reply(stream: &mut UnixStream) -> Result<Value, String> {
    let line = read_line_bounded(stream, MAX_RESPONSE_BYTES).await?;
    let response: Value = serde_json::from_slice(&line)
        .map_err(|error| format!("invalid Niri IPC JSON response: {error}"))?;
    if let Some(error) = response.get("Err") {
        return Err(format!("Niri IPC returned an error: {error}"));
    }
    response
        .get("Ok")
        .cloned()
        .ok_or_else(|| "Niri IPC response is missing Ok/Err".into())
}

async fn forward_events(mut source: UnixStream, mut destination: UnixStream) {
    loop {
        let Ok(line) = read_line_bounded(&mut source, MAX_EVENT_BYTES).await else {
            break;
        };
        if serde_json::from_slice::<Value>(&line).is_err() {
            break;
        }
        if destination.write_all(&line).await.is_err()
            || destination.write_all(b"\n").await.is_err()
        {
            break;
        }
    }
}

#[async_trait]
impl CompositorBackend for NiriAdapter {
    fn info(&self) -> CompositorInfo {
        CompositorInfo::detected(CompositorKind::Niri)
    }

    fn capabilities(&self) -> CompositorCapabilities {
        if self.socket.is_none() {
            return CompositorCapabilities::none();
        }
        CompositorCapabilities {
            workspaces: true,
            switch_workspace: true,
            windows: true,
            focus_window: true,
            move_window: true,
            move_window_to_monitor: false,
            toggle_floating: true,
            // Niri's current IPC Window model does not expose fullscreen state,
            // while WayControl's UI needs that state to label a toggle safely.
            toggle_fullscreen: false,
            toggle_pinned: false,
            center_floating_window: false,
            close_window: true,
            monitors: true,
            monitor_power: false,
            events: true,
            lock_screen: false,
        }
    }

    async fn workspaces(&self) -> Result<Vec<Workspace>, String> {
        self.workspace_values().await
    }

    async fn windows(&self) -> Result<Vec<Window>, String> {
        self.window_values().await
    }

    async fn switch_workspace(&self, id: &WorkspaceId) -> Result<(), String> {
        self.action(json!({"FocusWorkspace":{"reference":{"Id":Self::workspace_id(id)?}}}))
            .await
    }

    async fn focus_window(&self, id: &str) -> Result<(), String> {
        self.action(json!({"FocusWindow":{"id":self.window_id(id)?}}))
            .await
    }

    async fn move_window(&self, id: &str, workspace_id: &WorkspaceId) -> Result<(), String> {
        self.action(json!({"MoveWindowToWorkspace":{
            "window_id":self.window_id(id)?,
            "reference":{"Id":Self::workspace_id(workspace_id)?},
            "focus":false
        }}))
        .await
    }

    async fn toggle_floating(&self, id: &str) -> Result<(), String> {
        self.action(json!({"ToggleWindowFloating":{"id":self.window_id(id)?}}))
            .await
    }

    async fn toggle_fullscreen(&self, id: &str) -> Result<(), String> {
        let _ = self.window_id(id)?;
        Err("Niri fullscreen state is not exposed by this adapter".into())
    }

    async fn close_window(&self, id: &str) -> Result<(), String> {
        self.action(json!({"CloseWindow":{"id":self.window_id(id)?}}))
            .await
    }

    async fn monitors(&self) -> Result<Vec<Monitor>, String> {
        self.monitor_values().await
    }

    async fn set_monitor_power(&self, _name: &str, _enabled: bool) -> Result<(), String> {
        Err("Niri per-monitor power control is not supported by this adapter".into())
    }

    async fn lock_screen(&self) -> Result<(), String> {
        Err("Niri lock screen is not supported by this adapter".into())
    }

    async fn event_stream(&self) -> Result<UnixStream, String> {
        let mut stream = self.connection().await?;
        stream
            .write_all(b"\"EventStream\"\n")
            .await
            .map_err(|error| error.to_string())?;
        let response = read_reply(&mut stream).await?;
        if response != "Handled" {
            return Err("Niri did not confirm the event stream".into());
        }
        let (client, producer) = UnixStream::pair().map_err(|error| error.to_string())?;
        tokio::spawn(forward_events(stream, producer));
        Ok(client)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use tokio::{
        io::{AsyncBufReadExt, BufReader},
        net::UnixListener,
    };

    static NEXT_SOCKET: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn niri_without_a_socket_reports_no_compositor_capabilities() {
        let capabilities = NiriAdapter { socket: None }.capabilities();
        assert_eq!(capabilities, CompositorCapabilities::none());
    }

    #[test]
    fn configured_niri_adapter_advertises_implemented_workspace_and_window_actions() {
        let capabilities = NiriAdapter {
            socket: Some(PathBuf::from("/run/user/1000/niri.sock")),
        }
        .capabilities();
        assert!(capabilities.workspaces);
        assert!(capabilities.switch_workspace);
        assert!(capabilities.windows);
        assert!(capabilities.move_window);
        assert!(!capabilities.toggle_fullscreen);
        assert!(!capabilities.monitor_power);
    }

    #[tokio::test]
    #[ignore = "requires a live Niri compositor and WAYCONTROL_TEST_LIVE_NIRI=1"]
    async fn live_niri_supports_workspace_window_output_event_and_window_actions() {
        assert_eq!(
            std::env::var("WAYCONTROL_TEST_LIVE_NIRI").as_deref(),
            Ok("1"),
            "this live test must run only in its isolated CI session"
        );
        let adapter = NiriAdapter::from_env().expect("CI should provide NIRI_SOCKET");
        let monitors = adapter.monitors().await.expect("query Niri outputs");
        assert!(!monitors.is_empty(), "nested Niri should expose an output");
        let workspaces = adapter.workspaces().await.expect("query Niri workspaces");
        assert!(
            workspaces.len() >= 2,
            "test config should define named workspaces"
        );
        let original = workspaces
            .iter()
            .find(|workspace| workspace.active)
            .expect("Niri should expose a focused workspace")
            .id
            .clone();
        let target = workspaces
            .iter()
            .find(|workspace| !workspace.active)
            .expect("test config should provide another workspace")
            .id
            .clone();

        let window = tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                if let Ok(windows) = adapter.windows().await {
                    if let Some(window) = windows.into_iter().find(|window| {
                        window.title == "WayControlLiveTest"
                            || window.app_id == "WayControlLiveTest"
                    }) {
                        break window;
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        })
        .await
        .expect("CI foot window should appear in Niri's window list");
        assert_eq!(
            window.floating,
            Some(false),
            "test window should start tiled"
        );

        let mut events = BufReader::new(
            adapter
                .event_stream()
                .await
                .expect("subscribe to Niri events"),
        );
        adapter
            .switch_workspace(&target)
            .await
            .expect("focus the second named workspace");
        let target_id = target.as_str().parse::<u64>().unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let mut line = String::new();
            loop {
                line.clear();
                assert_ne!(
                    events.read_line(&mut line).await.unwrap(),
                    0,
                    "Niri event stream closed"
                );
                let Ok(event) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                if event["WorkspacesChanged"]["workspaces"]
                    .as_array()
                    .is_some_and(|items| {
                        items.iter().any(|item| {
                            item["id"].as_u64() == Some(target_id)
                                && item["is_focused"].as_bool() == Some(true)
                        })
                    })
                    || (event["WorkspaceActivated"]["id"].as_u64() == Some(target_id)
                        && event["WorkspaceActivated"]["focused"].as_bool() == Some(true))
                {
                    break;
                }
            }
        })
        .await
        .expect("Niri should stream a state update for the focused workspace");
        assert!(
            adapter
                .workspaces()
                .await
                .unwrap()
                .iter()
                .any(|workspace| workspace.id == target && workspace.active)
        );

        adapter
            .move_window(&window.address, &target)
            .await
            .expect("move the test window to the focused workspace");
        adapter
            .focus_window(&window.address)
            .await
            .expect("focus the test window");
        adapter
            .toggle_floating(&window.address)
            .await
            .expect("toggle the test window floating state");
        assert!(
            adapter
                .windows()
                .await
                .unwrap()
                .iter()
                .any(|item| item.address == window.address && item.floating == Some(true))
        );
        adapter
            .close_window(&window.address)
            .await
            .expect("close only the temporary CI window");
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if !adapter
                    .windows()
                    .await
                    .unwrap()
                    .iter()
                    .any(|item| item.address == window.address)
                {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        })
        .await
        .expect("closed foot window should disappear from Niri's window list");
        adapter
            .switch_workspace(&original)
            .await
            .expect("restore the initial focused workspace");
    }

    async fn mock_socket() -> (PathBuf, UnixListener) {
        let path = std::env::temp_dir().join(format!(
            "waycontrol-niri-{}-{}.sock",
            std::process::id(),
            NEXT_SOCKET.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).unwrap();
        (path, listener)
    }

    async fn read_request(stream: &mut UnixStream) -> Value {
        let line = read_line_bounded(stream, MAX_REQUEST_BYTES).await.unwrap();
        serde_json::from_slice(&line).unwrap()
    }

    async fn reply(stream: &mut UnixStream, value: &Value) {
        stream
            .write_all(&serde_json::to_vec(value).unwrap())
            .await
            .unwrap();
        stream.write_all(b"\n").await.unwrap();
    }

    #[test]
    fn niri_ids_are_scoped_and_strict() {
        let adapter = NiriAdapter { socket: None };
        assert_eq!(adapter.window_id("niri:42").unwrap(), 42);
        assert!(adapter.window_id("niri:42;Quit").is_err());
        assert!(NiriAdapter::workspace_id(&WorkspaceId::from("named-workspace")).is_err());
    }

    #[tokio::test]
    async fn workspace_query_uses_official_json_requests_and_maps_state() {
        let (path, listener) = mock_socket().await;
        let peer = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            assert_eq!(read_request(&mut stream).await, json!("Workspaces"));
            reply(
                &mut stream,
                &json!({"Ok":{"Workspaces":[{
                    "id":42,"idx":2,"name":"dev","output":"DP-1","is_focused":true
                }]}}),
            )
            .await;
            let (mut stream, _) = listener.accept().await.unwrap();
            assert_eq!(read_request(&mut stream).await, json!("Windows"));
            reply(
                &mut stream,
                &json!({"Ok":{"Windows":[{
                    "id":7,"title":"Terminal","app_id":"foot","workspace_id":42,
                    "is_focused":true,"is_floating":false,"is_fullscreen":false
                }]}}),
            )
            .await;
        });
        let adapter = NiriAdapter {
            socket: Some(path.clone()),
        };
        let workspaces = adapter.workspaces().await.unwrap();
        assert_eq!(workspaces[0].id.as_str(), "42");
        assert_eq!(workspaces[0].name, "dev");
        assert_eq!(workspaces[0].monitor.as_deref(), Some("DP-1"));
        assert_eq!(workspaces[0].window_count, 1);
        assert!(workspaces[0].active);
        peer.await.unwrap();
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn workspace_and_window_actions_use_typed_niri_ids() {
        let (path, listener) = mock_socket().await;
        let peer = tokio::spawn(async move {
            let expected = [
                json!({"Action":{"FocusWorkspace":{"reference":{"Id":42}}}}),
                json!({"Action":{"FocusWindow":{"id":7}}}),
                json!({"Action":{"MoveWindowToWorkspace":{"window_id":7,"reference":{"Id":42},"focus":false}}}),
                json!({"Action":{"ToggleWindowFloating":{"id":7}}}),
                json!({"Action":{"CloseWindow":{"id":7}}}),
            ];
            for request in expected {
                let (mut stream, _) = listener.accept().await.unwrap();
                assert_eq!(read_request(&mut stream).await, request);
                reply(&mut stream, &json!({"Ok":"Handled"})).await;
            }
        });
        let adapter = NiriAdapter {
            socket: Some(path.clone()),
        };
        adapter
            .switch_workspace(&WorkspaceId::from(42))
            .await
            .unwrap();
        adapter.focus_window("niri:7").await.unwrap();
        adapter
            .move_window("niri:7", &WorkspaceId::from(42))
            .await
            .unwrap();
        adapter.toggle_floating("niri:7").await.unwrap();
        adapter.close_window("niri:7").await.unwrap();
        peer.await.unwrap();
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn event_stream_consumes_ack_and_forwards_json_lines() {
        let (path, listener) = mock_socket().await;
        let peer = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            assert_eq!(
                read_line_bounded(&mut stream, MAX_REQUEST_BYTES)
                    .await
                    .unwrap(),
                b"\"EventStream\""
            );
            reply(&mut stream, &json!({"Ok":"Handled"})).await;
            stream
                .write_all(b"{\"WorkspacesChanged\":{\"workspaces\":[]}}\n")
                .await
                .unwrap();
        });
        let adapter = NiriAdapter {
            socket: Some(path.clone()),
        };
        let client = adapter.event_stream().await.unwrap();
        let mut reader = BufReader::new(client);
        let mut line = String::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            reader.read_line(&mut line),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&line).unwrap()["WorkspacesChanged"]["workspaces"],
            json!([])
        );
        peer.await.unwrap();
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn outputs_are_read_without_advertising_dpms_control() {
        let (path, listener) = mock_socket().await;
        let peer = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            assert_eq!(read_request(&mut stream).await, json!("Outputs"));
            reply(
                &mut stream,
                &json!({"Ok":{"Outputs":{"DP-1":{"name":"DP-1"},"eDP-1":{"name":"eDP-1"}}}}),
            )
            .await;
        });
        let adapter = NiriAdapter {
            socket: Some(path.clone()),
        };
        let monitors = adapter.monitors().await.unwrap();
        assert_eq!(monitors.len(), 2);
        assert!(
            monitors
                .iter()
                .all(|monitor| monitor.dpms_enabled.is_none())
        );
        assert!(!adapter.capabilities().monitor_power);
        assert!(!adapter.capabilities().toggle_fullscreen);
        peer.await.unwrap();
        std::fs::remove_file(path).unwrap();
    }
}
