//! Sway IPC adapter. It represents Sway container IDs as opaque `sway:<id>` IDs.

use async_trait::async_trait;
use serde_json::{Value, json};
use std::{env, path::PathBuf};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixStream, unix::OwnedReadHalf},
};
use waycontrol_compositor::{
    CompositorBackend, CompositorCapabilities, CompositorInfo, CompositorKind,
};
use waycontrol_core::{Monitor, Window, Workspace, WorkspaceId};

const MAGIC: &[u8; 6] = b"i3-ipc";
const MAX_MESSAGE_BYTES: usize = 16 * 1024 * 1024;
const MAX_EVENT_PAYLOAD_BYTES: usize = 7 * 1024;
const GET_WORKSPACES: u32 = 1;
const SUBSCRIBE: u32 = 2;
const GET_OUTPUTS: u32 = 3;
const GET_TREE: u32 = 4;
const RUN_COMMAND: u32 = 0;

#[derive(Clone, Debug)]
pub struct SwayAdapter {
    socket: Option<PathBuf>,
}

impl SwayAdapter {
    pub fn from_env() -> Result<Self, String> {
        let socket = env::var_os("SWAYSOCK")
            .map(PathBuf::from)
            .ok_or_else(|| "SWAYSOCK is not set".to_owned())?;
        Ok(Self {
            socket: Some(socket),
        })
    }

    pub fn from_env_or_unavailable() -> Self {
        Self::from_env().unwrap_or(Self { socket: None })
    }

    pub async fn connect_to(socket: impl Into<PathBuf>) -> Result<Self, String> {
        let adapter = Self {
            socket: Some(socket.into()),
        };
        adapter.request(GET_WORKSPACES, &[]).await?;
        Ok(adapter)
    }

    async fn connection(&self) -> Result<UnixStream, String> {
        let path = self
            .socket
            .as_ref()
            .ok_or_else(|| "SWAYSOCK is unavailable".to_owned())?;
        UnixStream::connect(path)
            .await
            .map_err(|error| format!("could not connect to Sway IPC socket: {error}"))
    }

    async fn request(&self, kind: u32, payload: &[u8]) -> Result<Value, String> {
        let mut stream = self.connection().await?;
        send_frame(&mut stream, kind, payload).await?;
        let (response_type, bytes) = read_frame(&mut stream).await?;
        if response_type != kind {
            return Err(format!(
                "unexpected Sway IPC reply type {response_type} for request {kind}"
            ));
        }
        serde_json::from_slice(&bytes).map_err(|error| format!("invalid Sway IPC JSON: {error}"))
    }

    async fn command(&self, command: String) -> Result<(), String> {
        let result = self.request(RUN_COMMAND, command.as_bytes()).await?;
        let entries = result
            .as_array()
            .ok_or_else(|| "invalid Sway command response".to_owned())?;
        if !entries.is_empty()
            && entries
                .iter()
                .all(|entry| entry.get("success").and_then(Value::as_bool) == Some(true))
        {
            Ok(())
        } else {
            Err(entries
                .iter()
                .filter_map(|entry| entry.get("error").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("; "))
        }
    }

    async fn workspace_values(&self) -> Result<Vec<Workspace>, String> {
        let values = self.request(GET_WORKSPACES, &[]).await?;
        let values = values
            .as_array()
            .ok_or_else(|| "invalid Sway workspace response".to_owned())?;
        let mut workspaces = values
            .iter()
            .filter_map(|item| {
                let name = item.get("name")?.as_str()?.to_owned();
                Some(Workspace {
                    id: WorkspaceId::from(name.as_str()),
                    name,
                    monitor: item
                        .get("output")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    window_count: 0,
                    active: item
                        .get("focused")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                })
            })
            .collect::<Vec<_>>();
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
        let tree = self.request(GET_TREE, &[]).await?;
        if !tree.is_object() {
            return Err("invalid Sway tree response".into());
        }
        let mut windows = Vec::new();
        collect_windows(&tree, None, None, false, &mut windows);
        Ok(windows)
    }

    fn id(&self, id: &str) -> Result<u64, String> {
        id.strip_prefix("sway:")
            .and_then(|rest| rest.parse::<u64>().ok())
            .filter(|id| *id > 0)
            .ok_or_else(|| "invalid Sway container ID".to_owned())
    }

    fn quote(value: &str) -> Result<String, String> {
        if value.is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
            return Err("invalid Sway name".into());
        }
        Ok(format!(
            "\"{}\"",
            value.replace('\\', "\\\\").replace('"', "\\\"")
        ))
    }
}

fn collect_windows(
    node: &Value,
    workspace: Option<&str>,
    monitor: Option<&str>,
    floating: bool,
    out: &mut Vec<Window>,
) {
    let kind = node.get("type").and_then(Value::as_str).unwrap_or_default();
    let floating = floating || kind == "floating_con";
    let workspace = if kind == "workspace" {
        node.get("name").and_then(Value::as_str).or(workspace)
    } else {
        workspace
    };
    let monitor = if kind == "workspace" {
        node.get("output").and_then(Value::as_str).or(monitor)
    } else {
        monitor
    };
    let app_id = node.get("app_id").and_then(Value::as_str).or_else(|| {
        node.get("window_properties")
            .and_then(|value| value.get("class"))
            .and_then(Value::as_str)
    });
    if matches!(kind, "con" | "floating_con") && app_id.is_some() {
        if let (Some(id), Some(app_id)) = (node.get("id").and_then(Value::as_u64), app_id) {
            out.push(Window {
                address: format!("sway:{id}"),
                title: node
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                app_id: app_id.to_owned(),
                workspace_id: workspace.map(WorkspaceId::from),
                monitor: monitor.map(str::to_owned),
                floating: Some(floating),
                pinned: None,
                icon_png_base64: None,
                fullscreen: node
                    .get("fullscreen_mode")
                    .and_then(Value::as_i64)
                    .unwrap_or(0)
                    != 0,
                focused: node
                    .get("focused")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            });
        }
    }
    if let Some(children) = node.get("nodes").and_then(Value::as_array) {
        for child in children {
            collect_windows(child, workspace, monitor, floating, out);
        }
    }
    if let Some(children) = node.get("floating_nodes").and_then(Value::as_array) {
        for child in children {
            collect_windows(child, workspace, monitor, true, out);
        }
    }
}

async fn send_frame(stream: &mut UnixStream, kind: u32, payload: &[u8]) -> Result<(), String> {
    if payload.len() > MAX_MESSAGE_BYTES {
        return Err("Sway IPC request exceeds size limit".into());
    }
    stream.write_all(MAGIC).await.map_err(|e| e.to_string())?;
    stream
        .write_all(&(payload.len() as u32).to_le_bytes())
        .await
        .map_err(|e| e.to_string())?;
    stream
        .write_all(&kind.to_le_bytes())
        .await
        .map_err(|e| e.to_string())?;
    stream.write_all(payload).await.map_err(|e| e.to_string())
}

async fn read_frame(stream: &mut UnixStream) -> Result<(u32, Vec<u8>), String> {
    let mut header = [0_u8; 14];
    stream
        .read_exact(&mut header)
        .await
        .map_err(|e| e.to_string())?;
    if &header[..6] != MAGIC {
        return Err("invalid Sway IPC magic".into());
    }
    let len = u32::from_le_bytes(header[6..10].try_into().expect("fixed header")) as usize;
    if len > MAX_MESSAGE_BYTES {
        return Err("Sway IPC response exceeds size limit".into());
    }
    let kind = u32::from_le_bytes(header[10..14].try_into().expect("fixed header"));
    let mut payload = vec![0; len];
    stream
        .read_exact(&mut payload)
        .await
        .map_err(|e| e.to_string())?;
    Ok((kind, payload))
}

async fn forward_events(mut reader: OwnedReadHalf, mut writer: UnixStream) {
    loop {
        let Ok((kind, payload)) = read_frame_half(&mut reader).await else {
            break;
        };
        let event = json!({"type":kind, "payload":serde_json::from_slice::<Value>(&payload).unwrap_or(Value::Null)});
        let Ok(mut line) = serde_json::to_vec(&event) else {
            break;
        };
        if line.len() >= 8192 {
            break;
        }
        line.push(b'\n');
        if writer.write_all(&line).await.is_err() {
            break;
        }
    }
}

async fn read_frame_half(reader: &mut OwnedReadHalf) -> Result<(u32, Vec<u8>), String> {
    let mut header = [0_u8; 14];
    reader
        .read_exact(&mut header)
        .await
        .map_err(|e| e.to_string())?;
    if &header[..6] != MAGIC {
        return Err("invalid Sway IPC event magic".into());
    }
    let len = u32::from_le_bytes(header[6..10].try_into().expect("fixed header")) as usize;
    if len > MAX_EVENT_PAYLOAD_BYTES {
        return Err("Sway IPC event exceeds the bounded forwarding limit".into());
    }
    let kind = u32::from_le_bytes(header[10..14].try_into().expect("fixed header"));
    let mut payload = vec![0; len];
    reader
        .read_exact(&mut payload)
        .await
        .map_err(|e| e.to_string())?;
    Ok((kind, payload))
}

#[async_trait]
impl CompositorBackend for SwayAdapter {
    fn info(&self) -> CompositorInfo {
        CompositorInfo::detected(CompositorKind::Sway)
    }
    fn capabilities(&self) -> CompositorCapabilities {
        CompositorCapabilities {
            workspaces: true,
            switch_workspace: true,
            windows: true,
            focus_window: true,
            move_window: true,
            move_window_to_monitor: false,
            toggle_floating: true,
            toggle_fullscreen: true,
            toggle_pinned: false,
            center_floating_window: false,
            close_window: true,
            monitors: true,
            monitor_power: true,
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
        self.command(format!("workspace {}", Self::quote(id.as_str())?))
            .await
    }
    async fn focus_window(&self, id: &str) -> Result<(), String> {
        self.command(format!("[con_id={}] focus", self.id(id)?))
            .await
    }
    async fn move_window(&self, id: &str, workspace_id: &WorkspaceId) -> Result<(), String> {
        self.command(format!(
            "[con_id={}] move container to workspace {}",
            self.id(id)?,
            Self::quote(workspace_id.as_str())?
        ))
        .await
    }
    async fn toggle_floating(&self, id: &str) -> Result<(), String> {
        self.command(format!("[con_id={}] floating toggle", self.id(id)?))
            .await
    }
    async fn toggle_fullscreen(&self, id: &str) -> Result<(), String> {
        self.command(format!("[con_id={}] fullscreen toggle", self.id(id)?))
            .await
    }
    async fn close_window(&self, id: &str) -> Result<(), String> {
        self.command(format!("[con_id={}] kill", self.id(id)?))
            .await
    }
    async fn monitors(&self) -> Result<Vec<Monitor>, String> {
        let outputs = self.request(GET_OUTPUTS, &[]).await?;
        outputs
            .as_array()
            .ok_or_else(|| "invalid Sway output response".to_owned())
            .map(|outputs| {
                outputs
                    .iter()
                    .filter_map(|output| {
                        Some(Monitor {
                            name: output.get("name")?.as_str()?.to_owned(),
                            dpms_enabled: output
                                .get("power")
                                .and_then(Value::as_bool)
                                .or_else(|| output.get("dpms").and_then(Value::as_bool)),
                        })
                    })
                    .collect()
            })
    }
    async fn set_monitor_power(&self, name: &str, enabled: bool) -> Result<(), String> {
        let outputs = self.request(GET_OUTPUTS, &[]).await?;
        let outputs = outputs
            .as_array()
            .ok_or_else(|| "invalid Sway output response".to_owned())?;
        if !outputs
            .iter()
            .any(|output| output.get("name").and_then(Value::as_str) == Some(name))
        {
            return Err("unknown Sway output".into());
        }
        if matches!(name, "*" | "-" | "--") {
            return Err("monitor power requires a specific Sway output".into());
        }
        let power = if enabled { "on" } else { "off" };
        self.command(format!("output {} power {power}", Self::quote(name)?))
            .await
    }
    async fn lock_screen(&self) -> Result<(), String> {
        Err("Sway lock screen is not supported by this adapter".into())
    }
    async fn event_stream(&self) -> Result<UnixStream, String> {
        let mut stream = self.connection().await?;
        send_frame(
            &mut stream,
            SUBSCRIBE,
            br#"["workspace","window","output","shutdown"]"#,
        )
        .await?;
        let (response_type, response) = read_frame(&mut stream).await?;
        if response_type != SUBSCRIBE {
            return Err(format!(
                "unexpected Sway subscription reply type {response_type}"
            ));
        }
        let response: Value = serde_json::from_slice(&response).map_err(|e| e.to_string())?;
        if response.get("success").and_then(Value::as_bool) != Some(true) {
            return Err("Sway rejected event subscription".into());
        }
        let (reader, _) = stream.into_split();
        let (client, producer) = UnixStream::pair().map_err(|e| e.to_string())?;
        tokio::spawn(forward_events(reader, producer));
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

    #[tokio::test]
    #[ignore = "requires a live Sway compositor and WAYCONTROL_TEST_LIVE_SWAY=1"]
    async fn live_sway_supports_workspace_window_monitor_power_and_event_operations() {
        assert_eq!(
            std::env::var("WAYCONTROL_TEST_LIVE_SWAY").as_deref(),
            Ok("1"),
            "this live test must run only in its isolated headless CI session"
        );
        let adapter = SwayAdapter::from_env().expect("CI should provide SWAYSOCK");
        let monitors = adapter.monitors().await.expect("query Sway outputs");
        assert!(
            !monitors.is_empty(),
            "headless Sway should expose an output"
        );
        let output_name = monitors[0].name.clone();
        assert_eq!(monitors[0].dpms_enabled, Some(true));
        assert!(adapter.capabilities().monitor_power);
        adapter
            .set_monitor_power(&output_name, false)
            .await
            .expect("turn off the isolated headless output");
        assert_eq!(
            adapter
                .monitors()
                .await
                .unwrap()
                .iter()
                .find(|monitor| monitor.name == output_name)
                .unwrap()
                .dpms_enabled,
            Some(false)
        );
        adapter
            .set_monitor_power(&output_name, true)
            .await
            .expect("restore the isolated headless output");
        assert_eq!(
            adapter
                .monitors()
                .await
                .unwrap()
                .iter()
                .find(|monitor| monitor.name == output_name)
                .unwrap()
                .dpms_enabled,
            Some(true)
        );
        let workspaces = adapter.workspaces().await.expect("query workspaces");
        let original = workspaces
            .iter()
            .find(|workspace| workspace.active)
            .map(|workspace| workspace.id.clone())
            .expect("headless Sway should expose an active workspace");

        let window = tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                if let Ok(windows) = adapter.windows().await {
                    if let Some(window) = windows
                        .into_iter()
                        .find(|window| window.title == "WayControlLiveTest")
                    {
                        break window;
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        })
        .await
        .expect("CI xterm window should appear in Sway's tree");

        let target = WorkspaceId::from("WayControl Live Test");
        let mut events = tokio::io::BufReader::new(
            adapter
                .event_stream()
                .await
                .expect("subscribe to Sway events"),
        );
        adapter
            .switch_workspace(&target)
            .await
            .expect("create and focus named workspace");
        let event = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let mut line = String::new();
            loop {
                line.clear();
                if events.read_line(&mut line).await? == 0 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "Sway event stream closed",
                    ));
                }
                if line.contains("WayControl Live Test") {
                    return Ok::<_, std::io::Error>(line.clone());
                }
            }
        })
        .await
        .expect("Sway should publish a workspace event")
        .expect("read Sway workspace event");
        assert!(
            event.contains("WayControl Live Test"),
            "unexpected event: {event}"
        );
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
            .expect("move the test window to the named workspace");
        adapter
            .focus_window(&window.address)
            .await
            .expect("focus the test window");
        adapter
            .toggle_floating(&window.address)
            .await
            .expect("toggle the test window floating state");
        let floating = adapter.windows().await.unwrap();
        assert!(
            floating
                .iter()
                .any(|item| item.address == window.address && item.floating == Some(true))
        );

        adapter
            .toggle_fullscreen(&window.address)
            .await
            .expect("toggle fullscreen for the test window");
        let fullscreen = adapter.windows().await.unwrap();
        assert!(
            fullscreen
                .iter()
                .any(|item| item.address == window.address && item.fullscreen)
        );
        adapter
            .switch_workspace(&original)
            .await
            .expect("restore the original active workspace");
        adapter
            .close_window(&window.address)
            .await
            .expect("close only the temporary CI window");
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let remaining = adapter.windows().await.unwrap();
                if !remaining.iter().any(|item| item.address == window.address) {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        })
        .await
        .expect("closed XTerm window should disappear from Sway's tree");
    }

    async fn mock_socket() -> (PathBuf, UnixListener) {
        let path = std::env::temp_dir().join(format!(
            "waycontrol-sway-{}-{}.sock",
            std::process::id(),
            NEXT_SOCKET.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).unwrap();
        (path, listener)
    }

    async fn receive_request(stream: &mut UnixStream) -> (u32, Vec<u8>) {
        let mut header = [0_u8; 14];
        stream.read_exact(&mut header).await.unwrap();
        assert_eq!(&header[..6], MAGIC);
        let len = u32::from_le_bytes(header[6..10].try_into().unwrap()) as usize;
        let kind = u32::from_le_bytes(header[10..14].try_into().unwrap());
        let mut payload = vec![0; len];
        stream.read_exact(&mut payload).await.unwrap();
        (kind, payload)
    }

    async fn reply(stream: &mut UnixStream, kind: u32, body: &[u8]) {
        send_frame(stream, kind, body).await.unwrap();
    }

    #[test]
    fn workspace_names_are_quoted_and_container_ids_are_strict() {
        assert_eq!(SwayAdapter::quote("3: web").unwrap(), "\"3: web\"");
        assert!(SwayAdapter::quote("bad\nname").is_err());
        let adapter = SwayAdapter { socket: None };
        assert_eq!(adapter.id("sway:42").unwrap(), 42);
        assert!(adapter.id("sway:42;kill").is_err());
    }

    #[test]
    fn workspace_tree_model_keeps_named_workspace_ids() {
        let tree: Value = serde_json::from_str(r#"{"type":"workspace","name":"3: web","nodes":[{"id":42,"type":"con","app_id":"firefox","name":"Browser","focused":true,"fullscreen_mode":1,"nodes":[],"floating_nodes":[]}],"floating_nodes":[{"id":43,"type":"floating_con","app_id":"terminal","name":"Terminal","focused":false,"fullscreen_mode":0,"nodes":[],"floating_nodes":[]}] }"#).unwrap();
        let mut windows = Vec::new();
        collect_windows(&tree, None, None, false, &mut windows);
        assert_eq!(windows[0].workspace_id.as_ref().unwrap().as_str(), "3: web");
        assert_eq!(windows[0].address, "sway:42");
        assert!(windows[0].focused && windows[0].fullscreen);
        assert_eq!(windows[1].address, "sway:43");
        assert_eq!(windows[1].floating, Some(true));
    }

    #[tokio::test]
    async fn ipc_workspace_query_preserves_names_and_counts_windows() {
        let (path, listener) = mock_socket().await;
        let peer = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            assert_eq!(receive_request(&mut stream).await.0, GET_WORKSPACES);
            reply(
                &mut stream,
                GET_WORKSPACES,
                br#"[{"name":"3: web","output":"DP-1","focused":true}]"#,
            )
            .await;
            let (mut stream, _) = listener.accept().await.unwrap();
            let (kind, _) = receive_request(&mut stream).await;
            assert_eq!(kind, GET_TREE);
            reply(&mut stream, GET_TREE, br#"{"type":"workspace","name":"3: web","nodes":[{"id":42,"type":"con","app_id":"firefox","name":"Browser","nodes":[],"floating_nodes":[]}],"floating_nodes":[]}"#).await;
        });
        let adapter = SwayAdapter {
            socket: Some(path.clone()),
        };
        let workspaces = adapter.workspaces().await.unwrap();
        assert_eq!(workspaces[0].id.as_str(), "3: web");
        assert_eq!(workspaces[0].window_count, 1);
        assert!(workspaces[0].active);
        peer.await.unwrap();
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn output_query_reports_sway_power_state_and_legacy_dpms_state() {
        let (path, listener) = mock_socket().await;
        let peer = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let (kind, _) = receive_request(&mut stream).await;
            assert_eq!(kind, GET_OUTPUTS);
            reply(
                &mut stream,
                GET_OUTPUTS,
                br#"[{"name":"DP-1","power":true},{"name":"DP-2","dpms":false},{"name":"DP-3"}]"#,
            )
            .await;
        });
        let adapter = SwayAdapter {
            socket: Some(path.clone()),
        };

        let monitors = adapter.monitors().await.unwrap();

        assert_eq!(monitors[0].name, "DP-1");
        assert_eq!(monitors[0].dpms_enabled, Some(true));
        assert_eq!(monitors[1].name, "DP-2");
        assert_eq!(monitors[1].dpms_enabled, Some(false));
        assert_eq!(monitors[2].dpms_enabled, None);
        peer.await.unwrap();
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn monitor_power_uses_a_quoted_output_name_and_explicit_dpms_state() {
        let (path, listener) = mock_socket().await;
        let peer = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let (kind, _) = receive_request(&mut stream).await;
            assert_eq!(kind, GET_OUTPUTS);
            reply(
                &mut stream,
                GET_OUTPUTS,
                br#"[{"name":"DP-1","power":true}]"#,
            )
            .await;

            let (mut stream, _) = listener.accept().await.unwrap();
            let (kind, payload) = receive_request(&mut stream).await;
            assert_eq!(kind, RUN_COMMAND);
            assert_eq!(payload, br#"output "DP-1" power off"#);
            reply(&mut stream, RUN_COMMAND, br#"[{"success":true}]"#).await;
        });
        let adapter = SwayAdapter {
            socket: Some(path.clone()),
        };

        adapter.set_monitor_power("DP-1", false).await.unwrap();

        peer.await.unwrap();
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn monitor_power_rejects_wildcard_and_focused_output_aliases() {
        for selector in ["*", "-", "--"] {
            let (path, listener) = mock_socket().await;
            let peer = tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                let (kind, _) = receive_request(&mut stream).await;
                assert_eq!(kind, GET_OUTPUTS);
                let outputs = format!(r#"[{{"name":"{selector}"}}]"#);
                reply(&mut stream, GET_OUTPUTS, outputs.as_bytes()).await;
                assert!(
                    tokio::time::timeout(std::time::Duration::from_millis(50), listener.accept())
                        .await
                        .is_err(),
                    "Sway selector {selector:?} must not reach RUN_COMMAND"
                );
            });
            let adapter = SwayAdapter {
                socket: Some(path.clone()),
            };

            let error = adapter
                .set_monitor_power(selector, false)
                .await
                .unwrap_err();

            assert!(error.contains("specific Sway output"), "{error}");
            peer.await.unwrap();
            std::fs::remove_file(path).unwrap();
        }
    }

    #[tokio::test]
    async fn monitor_power_rejects_outputs_missing_from_sway_output_list() {
        let (path, listener) = mock_socket().await;
        let peer = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let (kind, _) = receive_request(&mut stream).await;
            assert_eq!(kind, GET_OUTPUTS);
            reply(
                &mut stream,
                GET_OUTPUTS,
                br#"[{"name":"DP-1","power":true}]"#,
            )
            .await;
            assert!(
                tokio::time::timeout(std::time::Duration::from_millis(50), listener.accept())
                    .await
                    .is_err(),
                "an unlisted output must not reach RUN_COMMAND"
            );
        });
        let adapter = SwayAdapter {
            socket: Some(path.clone()),
        };

        let error = adapter.set_monitor_power("DP-2", false).await.unwrap_err();

        assert!(error.contains("unknown Sway output"), "{error}");
        peer.await.unwrap();
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn sway_advertises_monitor_power_control() {
        assert!(SwayAdapter { socket: None }.capabilities().monitor_power);
    }

    #[tokio::test]
    async fn switch_workspace_sends_one_quoted_sway_command() {
        let (path, listener) = mock_socket().await;
        let peer = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let (kind, payload) = receive_request(&mut stream).await;
            assert_eq!(kind, RUN_COMMAND);
            assert_eq!(payload, br#"workspace "3: web""#);
            reply(&mut stream, RUN_COMMAND, br#"[{"success":true}]"#).await;
        });
        let adapter = SwayAdapter {
            socket: Some(path.clone()),
        };
        adapter
            .switch_workspace(&WorkspaceId::from("3: web"))
            .await
            .unwrap();
        peer.await.unwrap();
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn event_subscription_forwards_bounded_json_lines() {
        let (path, listener) = mock_socket().await;
        let peer = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let (kind, payload) = receive_request(&mut stream).await;
            assert_eq!(kind, SUBSCRIBE);
            assert_eq!(payload, br#"["workspace","window","output","shutdown"]"#);
            reply(&mut stream, SUBSCRIBE, br#"{"success":true}"#).await;
            send_frame(
                &mut stream,
                0x8000_0000,
                br#"{"change":"focus","current":{"name":"3: web"}}"#,
            )
            .await
            .unwrap();
        });
        let adapter = SwayAdapter {
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
        let event: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(event["type"], 0x8000_0000_u64);
        assert_eq!(event["payload"]["change"], "focus");
        peer.await.unwrap();
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn window_actions_send_only_validated_container_commands() {
        let (path, listener) = mock_socket().await;
        let peer = tokio::spawn(async move {
            let expected = [
                "[con_id=42] focus",
                "[con_id=42] move container to workspace \"3: web\"",
                "[con_id=42] floating toggle",
                "[con_id=42] fullscreen toggle",
                "[con_id=42] kill",
            ];
            for command in expected {
                let (mut stream, _) = listener.accept().await.unwrap();
                let (kind, payload) = receive_request(&mut stream).await;
                assert_eq!(kind, RUN_COMMAND);
                assert_eq!(payload, command.as_bytes());
                reply(&mut stream, RUN_COMMAND, br#"[{"success":true}]"#).await;
            }
        });
        let adapter = SwayAdapter {
            socket: Some(path.clone()),
        };
        adapter.focus_window("sway:42").await.unwrap();
        adapter
            .move_window("sway:42", &WorkspaceId::from("3: web"))
            .await
            .unwrap();
        adapter.toggle_floating("sway:42").await.unwrap();
        adapter.toggle_fullscreen("sway:42").await.unwrap();
        adapter.close_window("sway:42").await.unwrap();
        peer.await.unwrap();
        assert!(adapter.focus_window("sway:42; kill").await.is_err());
        std::fs::remove_file(path).unwrap();
    }
}
