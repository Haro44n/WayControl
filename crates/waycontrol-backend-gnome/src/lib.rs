//! Optional GNOME Shell adapter over the WayControl v1 session D-Bus interface.

use async_trait::async_trait;
use futures_util::StreamExt;
use serde::Deserialize;
use waycontrol_compositor::{
    CompositorBackend, CompositorCapabilities, CompositorInfo, CompositorKind,
};
use waycontrol_core::{Monitor, Window, Workspace, WorkspaceId};
use zbus::{Connection, proxy::Proxy as GenericProxy};

const SERVICE: &str = "org.waycontrol.GnomeShell";
const PATH: &str = "/org/waycontrol/GnomeShell";
const INTERFACE: &str = "org.waycontrol.GnomeShell1";
const INTERFACE_VERSION: u32 = 1;
const MAX_JSON_BYTES: usize = 256 * 1024;
const MAX_WORKSPACES: usize = 256;
const MAX_WINDOWS: usize = 128;
const MAX_TEXT_BYTES: usize = 4096;

#[derive(Clone, Debug)]
pub struct GnomeAdapter {
    connection: Connection,
}

impl GnomeAdapter {
    /// Connect and negotiate the fixed interface before advertising any support.
    pub async fn connect() -> Result<Self, String> {
        let connection = Connection::session()
            .await
            .map_err(|error| format!("could not connect to the session D-Bus: {error}"))?;
        Self::from_connection(connection).await
    }

    async fn from_connection(connection: Connection) -> Result<Self, String> {
        let adapter = Self { connection };
        let version: u32 = adapter
            .proxy()
            .await?
            .call("GetVersion", &())
            .await
            .map_err(|error| format!("WayControl GNOME extension is unavailable: {error}"))?;
        if version != INTERFACE_VERSION {
            return Err(format!(
                "unsupported WayControl GNOME extension interface version {version}"
            ));
        }
        adapter.workspaces().await?;
        adapter.windows().await?;
        Ok(adapter)
    }

    async fn proxy(&self) -> Result<GenericProxy<'_>, String> {
        GenericProxy::new(&self.connection, SERVICE, PATH, INTERFACE)
            .await
            .map_err(|error| {
                format!("could not connect to the WayControl GNOME extension: {error}")
            })
    }

    async fn invoke(
        &self,
        method: &str,
        arguments: &(impl zbus::zvariant::DynamicType + serde::Serialize),
    ) -> Result<(), String> {
        let reply = self
            .proxy()
            .await?
            .call_method(method, arguments)
            .await
            .map_err(|error| format!("GNOME compositor operation failed: {error}"))?;
        let accepted = reply.body().deserialize::<bool>().map_err(|error| {
            format!("GNOME extension returned an invalid operation result: {error}")
        })?;
        if accepted {
            Ok(())
        } else {
            Err("GNOME extension rejected a stale or invalid compositor ID".into())
        }
    }

    async fn read_json<T: for<'de> Deserialize<'de>>(&self, method: &str) -> Result<T, String> {
        let response: String = self
            .proxy()
            .await?
            .call(method, &())
            .await
            .map_err(|error| format!("GNOME compositor query failed: {error}"))?;
        if response.len() > MAX_JSON_BYTES {
            return Err("GNOME extension response exceeded the size limit".into());
        }
        serde_json::from_str(&response)
            .map_err(|error| format!("GNOME extension returned invalid JSON: {error}"))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkspaceRecord {
    id: String,
    name: String,
    monitor: Option<String>,
    window_count: u64,
    active: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WindowRecord {
    id: String,
    title: String,
    app_id: String,
    workspace_id: Option<String>,
    monitor: Option<String>,
    fullscreen: bool,
    focused: bool,
}

fn bounded_text(value: String, max_bytes: usize) -> Result<String, String> {
    if value.len() > max_bytes || value.chars().any(char::is_control) {
        return Err("GNOME extension returned invalid or oversized text".into());
    }
    Ok(value)
}

fn map_workspaces(records: Vec<WorkspaceRecord>) -> Result<Vec<Workspace>, String> {
    if records.len() > MAX_WORKSPACES {
        return Err("GNOME extension returned too many workspaces".into());
    }
    records
        .into_iter()
        .map(|record| {
            if !valid_workspace_id(&record.id) {
                return Err("GNOME extension returned an invalid workspace ID".into());
            }
            Ok(Workspace {
                id: WorkspaceId::new(record.id),
                name: bounded_text(record.name, MAX_TEXT_BYTES)?,
                monitor: record
                    .monitor
                    .map(|value| bounded_text(value, 256))
                    .transpose()?,
                window_count: record.window_count,
                active: record.active,
            })
        })
        .collect()
}

fn map_windows(records: Vec<WindowRecord>) -> Result<Vec<Window>, String> {
    if records.len() > MAX_WINDOWS {
        return Err("GNOME extension returned too many windows".into());
    }
    records
        .into_iter()
        .map(|record| {
            if !valid_window_id(&record.id) {
                return Err("GNOME extension returned an invalid window ID".into());
            }
            let workspace_id = record
                .workspace_id
                .map(|id| -> Result<WorkspaceId, String> {
                    if !valid_workspace_id(&id) {
                        return Err("GNOME extension returned an invalid workspace ID".into());
                    }
                    Ok(WorkspaceId::new(id))
                })
                .transpose()?;
            Ok(Window {
                address: record.id,
                title: bounded_text(record.title, MAX_TEXT_BYTES)?,
                app_id: bounded_text(record.app_id, 256)?,
                workspace_id,
                monitor: record
                    .monitor
                    .map(|value| bounded_text(value, 256))
                    .transpose()?,
                floating: None,
                pinned: None,
                icon_png_base64: None,
                fullscreen: record.fullscreen,
                focused: record.focused,
            })
        })
        .collect()
}

fn canonical_numeric_id(value: &str, prefix: &str, max_digits: usize) -> bool {
    let Some(digits) = value.strip_prefix(prefix) else {
        return false;
    };
    !digits.is_empty()
        && digits.len() <= max_digits
        && digits.bytes().all(|byte| byte.is_ascii_digit())
        && (digits == "0" || !digits.starts_with('0'))
}

fn valid_workspace_id(value: &str) -> bool {
    canonical_numeric_id(value, "gnome:", 20)
}

fn valid_window_id(value: &str) -> bool {
    canonical_numeric_id(value, "gnome:", 20)
}

#[async_trait]
impl CompositorBackend for GnomeAdapter {
    fn info(&self) -> CompositorInfo {
        CompositorInfo::detected(CompositorKind::Gnome)
    }

    fn capabilities(&self) -> CompositorCapabilities {
        CompositorCapabilities {
            workspaces: true,
            switch_workspace: true,
            windows: true,
            focus_window: true,
            move_window: true,
            toggle_fullscreen: true,
            close_window: true,
            events: true,
            ..CompositorCapabilities::none()
        }
    }

    async fn workspaces(&self) -> Result<Vec<Workspace>, String> {
        map_workspaces(self.read_json("ListWorkspaces").await?)
    }

    async fn windows(&self) -> Result<Vec<Window>, String> {
        map_windows(self.read_json("ListWindows").await?)
    }

    async fn switch_workspace(&self, id: &WorkspaceId) -> Result<(), String> {
        if !valid_workspace_id(id.as_str()) {
            return Err("invalid GNOME workspace ID".into());
        }
        self.invoke("SwitchWorkspace", &(id.as_str(),)).await
    }

    async fn focus_window(&self, id: &str) -> Result<(), String> {
        if !valid_window_id(id) {
            return Err("invalid GNOME window ID".into());
        }
        self.invoke("FocusWindow", &(id,)).await
    }

    async fn move_window(&self, id: &str, workspace_id: &WorkspaceId) -> Result<(), String> {
        if !valid_window_id(id) || !valid_workspace_id(workspace_id.as_str()) {
            return Err("invalid GNOME window or workspace ID".into());
        }
        self.invoke("MoveWindow", &(id, workspace_id.as_str()))
            .await
    }

    async fn toggle_floating(&self, _id: &str) -> Result<(), String> {
        Err("GNOME floating-state control is not supported".into())
    }

    async fn toggle_fullscreen(&self, id: &str) -> Result<(), String> {
        if !valid_window_id(id) {
            return Err("invalid GNOME window ID".into());
        }
        self.invoke("ToggleFullscreen", &(id,)).await
    }

    async fn close_window(&self, id: &str) -> Result<(), String> {
        if !valid_window_id(id) {
            return Err("invalid GNOME window ID".into());
        }
        self.invoke("CloseWindow", &(id,)).await
    }

    async fn monitors(&self) -> Result<Vec<Monitor>, String> {
        Err("GNOME monitor listing is not supported by this adapter".into())
    }

    async fn set_monitor_power(&self, _name: &str, _enabled: bool) -> Result<(), String> {
        Err("GNOME monitor power control is not supported".into())
    }

    async fn lock_screen(&self) -> Result<(), String> {
        Err("GNOME lock-screen control is not supported".into())
    }

    async fn event_stream(&self) -> Result<tokio::net::UnixStream, String> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let proxy = self.proxy().await?;
        let mut signals = proxy
            .receive_signal("Changed")
            .await
            .map_err(|error| format!("could not subscribe to GNOME compositor changes: {error}"))?;
        let (socket, reader) = tokio::net::UnixStream::pair()
            .map_err(|error| format!("could not create GNOME event stream: {error}"))?;
        let (mut monitor, mut writer) = socket.into_split();
        tokio::spawn(async move {
            let mut close_probe = [0_u8; 1];
            loop {
                tokio::select! {
                    signal = signals.next() => {
                        let Some(signal) = signal else { break };
                        let Ok(event) = signal.body().deserialize::<String>() else { continue };
                        let line = match event.as_str() {
                            "workspace-changed" => "workspace>>gnome:changed\n",
                            "windows-changed" => "windows>>gnome:changed\n",
                            _ => continue,
                        };
                        if writer.write_all(line.as_bytes()).await.is_err() { break; }
                    }
                    closed = monitor.read(&mut close_probe) => {
                        if !matches!(closed, Ok(count) if count > 0) { break; }
                        break;
                    }
                }
            }
        });
        Ok(reader)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::{Arc, Mutex};

    static DBUS_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    #[derive(Clone)]
    struct MockShell(Arc<Mutex<MockState>>);

    #[derive(Default)]
    struct MockState {
        version: u32,
        calls: Vec<String>,
        workspaces_json: Option<String>,
    }

    #[zbus::interface(name = "org.waycontrol.GnomeShell1")]
    impl MockShell {
        fn get_version(&self) -> u32 {
            self.0.lock().unwrap().version
        }

        fn list_workspaces(&self) -> String {
            self.0.lock().unwrap().workspaces_json.clone().unwrap_or_else(|| {
                r#"[{"id":"gnome:0","name":"Main","monitor":"0","window_count":1,"active":true}]"#.into()
            })
        }

        fn list_windows(&self) -> String {
            r#"[{"id":"gnome:42","title":"Terminal","app_id":"org.example.Terminal","workspace_id":"gnome:0","monitor":"0","fullscreen":false,"focused":true}]"#.into()
        }

        fn switch_workspace(&self, id: &str) -> bool {
            self.0.lock().unwrap().calls.push(format!("workspace:{id}"));
            id == "gnome:0"
        }

        fn focus_window(&self, id: &str) -> bool {
            self.0.lock().unwrap().calls.push(format!("focus:{id}"));
            id == "gnome:42"
        }

        fn move_window(&self, id: &str, workspace_id: &str) -> bool {
            self.0
                .lock()
                .unwrap()
                .calls
                .push(format!("move:{id}:{workspace_id}"));
            id == "gnome:42" && workspace_id == "gnome:0"
        }

        fn toggle_fullscreen(&self, id: &str) -> bool {
            self.0
                .lock()
                .unwrap()
                .calls
                .push(format!("fullscreen:{id}"));
            id == "gnome:42"
        }

        fn close_window(&self, id: &str) -> bool {
            self.0.lock().unwrap().calls.push(format!("close:{id}"));
            id == "gnome:42"
        }

        #[zbus(signal)]
        async fn changed(
            emitter: &zbus::object_server::SignalEmitter<'_>,
            event: &str,
        ) -> zbus::Result<()>;
    }

    async fn mock_service(version: u32) -> (Connection, Arc<Mutex<MockState>>) {
        let state = Arc::new(Mutex::new(MockState {
            version,
            calls: Vec::new(),
            workspaces_json: None,
        }));
        let service = zbus::connection::Builder::session()
            .unwrap()
            .name(SERVICE)
            .unwrap()
            .serve_at(PATH, MockShell(state.clone()))
            .unwrap()
            .build()
            .await
            .unwrap();
        (service, state)
    }

    #[tokio::test]
    async fn negotiation_rejects_incompatible_extension_version() {
        let _lock = DBUS_TEST_LOCK.lock().await;
        let (_service, _state) = mock_service(INTERFACE_VERSION + 1).await;
        let client = Connection::session().await.unwrap();
        let error = GnomeAdapter::from_connection(client).await.unwrap_err();
        assert!(error.contains("unsupported WayControl GNOME extension interface version"));
    }

    #[tokio::test]
    async fn negotiation_rejects_missing_extension() {
        let _lock = DBUS_TEST_LOCK.lock().await;
        let client = Connection::session().await.unwrap();
        assert!(GnomeAdapter::from_connection(client).await.is_err());
    }

    #[tokio::test]
    async fn negotiation_rejects_bridge_with_invalid_live_workspace_data() {
        let _lock = DBUS_TEST_LOCK.lock().await;
        let (_service, state) = mock_service(INTERFACE_VERSION).await;
        state.lock().unwrap().workspaces_json = Some("not-json".into());
        let client = Connection::session().await.unwrap();
        assert!(GnomeAdapter::from_connection(client).await.is_err());
    }

    #[tokio::test]
    async fn negotiated_adapter_maps_state_and_uses_typed_allowlisted_calls() {
        use tokio::io::{AsyncBufReadExt, BufReader};

        let _lock = DBUS_TEST_LOCK.lock().await;
        let (_service, state) = mock_service(INTERFACE_VERSION).await;
        let client = Connection::session().await.unwrap();
        let adapter = GnomeAdapter::from_connection(client).await.unwrap();
        assert!(adapter.capabilities().workspaces);
        assert!(adapter.capabilities().windows);
        assert!(adapter.capabilities().toggle_fullscreen);
        assert!(!adapter.capabilities().toggle_floating);
        assert!(!adapter.capabilities().monitor_power);
        assert!(!adapter.capabilities().lock_screen);

        let mut events = BufReader::new(adapter.event_stream().await.unwrap());

        let workspaces = adapter.workspaces().await.unwrap();
        assert_eq!(workspaces[0].id.as_str(), "gnome:0");
        assert!(workspaces[0].active);
        let windows = adapter.windows().await.unwrap();
        assert_eq!(windows[0].floating, None);
        assert_eq!(windows[0].address, "gnome:42");
        adapter
            .switch_workspace(&WorkspaceId::new("gnome:0"))
            .await
            .unwrap();
        adapter.focus_window("gnome:42").await.unwrap();
        adapter
            .move_window("gnome:42", &WorkspaceId::new("gnome:0"))
            .await
            .unwrap();
        adapter.toggle_fullscreen("gnome:42").await.unwrap();
        adapter.close_window("gnome:42").await.unwrap();
        assert!(adapter.focus_window("gnome:999").await.is_err());
        assert_eq!(state.lock().unwrap().calls.len(), 6);

        let emitter = zbus::object_server::SignalEmitter::new(&_service, PATH).unwrap();
        MockShell::changed(&emitter, "workspace-changed")
            .await
            .unwrap();
        let mut line = String::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            events.read_line(&mut line),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(line, "workspace>>gnome:changed\n");
    }

    #[test]
    fn gnome_ids_are_canonical_and_bounded() {
        assert!(valid_workspace_id("gnome:0"));
        assert!(valid_window_id("gnome:9007199254740991"));
        assert!(!valid_workspace_id("gnome:-1"));
        assert!(!valid_workspace_id("gnome:01"));
        assert!(!valid_window_id("gnome:1\nCloseWindow"));
        assert!(!valid_window_id(&format!("gnome:{}", "9".repeat(32))));
    }

    #[test]
    fn gnome_mapping_preserves_unknown_floating_state() {
        let windows = map_windows(vec![WindowRecord {
            id: "gnome:42".into(),
            title: "Terminal".into(),
            app_id: "org.example.Terminal".into(),
            workspace_id: Some("gnome:0".into()),
            monitor: Some("0".into()),
            fullscreen: false,
            focused: true,
        }])
        .unwrap();
        assert_eq!(windows[0].floating, None);
        assert_eq!(
            windows[0].workspace_id.as_ref().unwrap().as_str(),
            "gnome:0"
        );
    }

    #[test]
    fn gnome_mapping_rejects_bad_ids_controls_and_oversized_collections() {
        assert!(
            map_workspaces(vec![WorkspaceRecord {
                id: "gnome:00".into(),
                name: "Workspace".into(),
                monitor: None,
                window_count: 0,
                active: true,
            }])
            .is_err()
        );
        assert!(bounded_text("bad\nvalue".into(), 32).is_err());
        assert!(
            map_windows(vec![WindowRecord {
                id: "gnome:1".into(),
                title: "x".repeat(MAX_TEXT_BYTES + 1),
                app_id: "app".into(),
                workspace_id: None,
                monitor: None,
                fullscreen: false,
                focused: false,
            }])
            .is_err()
        );
    }
}
