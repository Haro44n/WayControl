//! Workspace-only adapter for KDE KWin's VirtualDesktopManager D-Bus API.

use async_trait::async_trait;
use futures_util::StreamExt;
use waycontrol_compositor::{CompositorBackend, CompositorCapabilities, CompositorInfo};
use waycontrol_core::{Monitor, Window, Workspace, WorkspaceId};
use zbus::{Connection, proxy::Proxy as GenericProxy};

const SERVICE: &str = "org.kde.KWin";
const PATH: &str = "/VirtualDesktopManager";
const INTERFACE: &str = "org.kde.KWin.VirtualDesktopManager";
const MAX_EVENT_ID_BYTES: usize = 128;

#[derive(Clone, Debug, Default)]
pub struct KWinAdapter {
    connection: Option<Connection>,
}

impl KWinAdapter {
    #[cfg(test)]
    fn with_connection(connection: Connection) -> Self {
        Self {
            connection: Some(connection),
        }
    }

    async fn connection(&self) -> Result<Connection, String> {
        if let Some(connection) = &self.connection {
            return Ok(connection.clone());
        }
        Connection::session()
            .await
            .map_err(|error| format!("could not connect to the session D-Bus: {error}"))
    }

    async fn proxy<'a>(connection: &'a Connection) -> Result<GenericProxy<'a>, String> {
        GenericProxy::new(connection, SERVICE, PATH, INTERFACE)
            .await
            .map_err(|error| format!("could not connect to KWin virtual desktop API: {error}"))
    }
}

fn map_desktops(desktops: Vec<(i32, String, String)>, current: &str) -> Vec<Workspace> {
    desktops
        .into_iter()
        .map(|(position, id, name)| Workspace {
            id: WorkspaceId::new(format!("kwin:{id}")),
            name: if name.is_empty() {
                format!("Desktop {}", position.saturating_add(1).max(1))
            } else {
                name
            },
            monitor: None,
            window_count: 0,
            active: id == current,
        })
        .collect()
}

#[async_trait]
impl CompositorBackend for KWinAdapter {
    fn info(&self) -> CompositorInfo {
        CompositorInfo::detected(waycontrol_compositor::CompositorKind::KdeKWin)
    }

    fn capabilities(&self) -> CompositorCapabilities {
        CompositorCapabilities {
            workspaces: true,
            switch_workspace: true,
            events: true,
            ..CompositorCapabilities::none()
        }
    }

    async fn workspaces(&self) -> Result<Vec<Workspace>, String> {
        let connection = self.connection().await?;
        let proxy = Self::proxy(&connection).await?;
        let desktops: Vec<(i32, String, String)> = proxy
            .get_property("desktops")
            .await
            .map_err(|error| format!("could not read KWin virtual desktops: {error}"))?;
        let current: String = proxy
            .get_property("current")
            .await
            .map_err(|error| format!("could not read KWin current desktop: {error}"))?;
        Ok(map_desktops(desktops, &current))
    }

    async fn switch_workspace(&self, id: &WorkspaceId) -> Result<(), String> {
        let raw_id = id
            .as_str()
            .strip_prefix("kwin:")
            .filter(|id| !id.is_empty())
            .ok_or_else(|| "invalid KWin desktop ID".to_owned())?;
        let connection = self.connection().await?;
        let proxy = Self::proxy(&connection).await?;
        let desktops: Vec<(i32, String, String)> = proxy
            .get_property("desktops")
            .await
            .map_err(|error| format!("could not read KWin virtual desktops: {error}"))?;
        if !desktops
            .iter()
            .any(|(_, desktop_id, _)| desktop_id == raw_id)
        {
            return Err("KWin desktop ID was not found".into());
        }
        proxy
            .set_property("current", raw_id)
            .await
            .map_err(|error| format!("could not switch KWin virtual desktop: {error}"))
    }

    async fn windows(&self) -> Result<Vec<Window>, String> {
        Err("KWin window listing is not supported by this adapter".into())
    }
    async fn focus_window(&self, _id: &str) -> Result<(), String> {
        Err("KWin window focus is not supported by this adapter".into())
    }
    async fn move_window(&self, _id: &str, _workspace_id: &WorkspaceId) -> Result<(), String> {
        Err("KWin window movement is not supported by this adapter".into())
    }
    async fn toggle_floating(&self, _id: &str) -> Result<(), String> {
        Err("KWin floating state is not supported by this adapter".into())
    }
    async fn toggle_fullscreen(&self, _id: &str) -> Result<(), String> {
        Err("KWin fullscreen control is not supported by this adapter".into())
    }
    async fn close_window(&self, _id: &str) -> Result<(), String> {
        Err("KWin window closing is not supported by this adapter".into())
    }
    async fn monitors(&self) -> Result<Vec<Monitor>, String> {
        Err("KWin monitor listing is not supported by this adapter".into())
    }
    async fn set_monitor_power(&self, _name: &str, _enabled: bool) -> Result<(), String> {
        Err("KWin monitor power control is not supported by this adapter".into())
    }
    async fn lock_screen(&self) -> Result<(), String> {
        Err("KWin lock screen is not supported by this adapter".into())
    }
    async fn event_stream(&self) -> Result<tokio::net::UnixStream, String> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let connection = self.connection().await?;
        let (socket, reader) = tokio::net::UnixStream::pair()
            .map_err(|error| format!("could not create KWin event stream: {error}"))?;
        let (mut monitor, mut writer) = socket.into_split();
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let proxy = match Self::proxy(&connection).await {
                Ok(proxy) => proxy,
                Err(error) => {
                    let _ = ready_tx.send(Err(error));
                    return;
                }
            };
            let mut signals = match proxy.receive_signal("currentChanged").await {
                Ok(signals) => signals,
                Err(error) => {
                    let _ = ready_tx.send(Err(format!(
                        "could not subscribe to KWin workspace changes: {error}"
                    )));
                    return;
                }
            };
            let mut desktop_signals = match proxy.receive_signal("desktopsChanged").await {
                Ok(signals) => signals,
                Err(error) => {
                    let _ = ready_tx.send(Err(format!(
                        "could not subscribe to KWin workspace-list changes: {error}"
                    )));
                    return;
                }
            };
            if ready_tx.send(Ok(())).is_err() {
                return;
            }
            let mut close_probe = [0_u8; 1];
            loop {
                tokio::select! {
                    signal = signals.next() => {
                        let Some(signal) = signal else { break };
                        let Ok(id) = signal.body().deserialize::<String>() else { continue };
                        if !valid_event_id(&id) {
                            continue;
                        }
                        let line = format!("workspace>>kwin:{id}\n");
                        if writer.write_all(line.as_bytes()).await.is_err() {
                            break;
                        }
                    }
                    signal = desktop_signals.next() => {
                        if signal.is_none() || writer.write_all(b"workspaces>>kwin:changed\n").await.is_err() {
                            break;
                        }
                    }
                    closed = monitor.read(&mut close_probe) => {
                        match closed {
                            Ok(0) | Err(_) => break,
                            Ok(_) => break,
                        }
                    }
                }
            }
        });
        ready_rx
            .await
            .map_err(|_| "KWin event subscription ended during setup".to_owned())??;
        Ok(reader)
    }
}

fn valid_event_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_EVENT_ID_BYTES
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[tokio::test]
    #[ignore = "requires an isolated live KWin session and WAYCONTROL_TEST_LIVE_KWIN=1"]
    async fn live_kwin_lists_switches_and_streams_virtual_desktops() {
        use tokio::io::{AsyncBufReadExt, BufReader};

        assert_eq!(
            std::env::var("WAYCONTROL_TEST_LIVE_KWIN").as_deref(),
            Ok("1"),
            "this live test must run only in its isolated CI session"
        );
        let adapter = KWinAdapter::default();
        let workspaces = adapter.workspaces().await.expect("query KWin desktops");
        assert!(
            workspaces.len() >= 2,
            "isolated KWin config should define at least two desktops"
        );
        let original = workspaces
            .iter()
            .find(|workspace| workspace.active)
            .expect("KWin should expose its current desktop")
            .id
            .clone();
        let target = workspaces
            .iter()
            .find(|workspace| !workspace.active)
            .expect("isolated KWin should expose another desktop")
            .id
            .clone();
        let mut events = BufReader::new(adapter.event_stream().await.expect("subscribe to KWin"));

        adapter
            .switch_workspace(&target)
            .await
            .expect("switch the current KWin desktop");
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let expected = format!("workspace>>{}", target.as_str());
            let mut line = String::new();
            loop {
                line.clear();
                assert_ne!(
                    events.read_line(&mut line).await.unwrap(),
                    0,
                    "KWin event stream closed"
                );
                if line.trim_end() == expected {
                    break;
                }
            }
        })
        .await
        .expect("KWin should stream its current desktop signal");
        assert!(
            adapter
                .workspaces()
                .await
                .unwrap()
                .iter()
                .any(|workspace| workspace.id == target && workspace.active)
        );
        adapter
            .switch_workspace(&original)
            .await
            .expect("restore the initial KWin desktop");
    }

    static DBUS_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    #[derive(Clone)]
    struct MockKWin(Arc<Mutex<MockState>>);

    #[derive(Clone)]
    struct MockState {
        desktops: Vec<(i32, String, String)>,
        current: String,
    }

    #[zbus::interface(name = "org.kde.KWin.VirtualDesktopManager")]
    impl MockKWin {
        #[zbus(property, name = "desktops")]
        fn desktops(&self) -> Vec<(i32, String, String)> {
            self.0.lock().unwrap().desktops.clone()
        }

        #[zbus(property, name = "current")]
        fn current(&self) -> String {
            self.0.lock().unwrap().current.clone()
        }

        #[zbus(property, name = "current")]
        fn set_current(&mut self, value: String) -> zbus::fdo::Result<()> {
            self.0.lock().unwrap().current = value;
            Ok(())
        }

        #[zbus(signal, name = "currentChanged")]
        async fn emit_current_changed(
            emitter: &zbus::object_server::SignalEmitter<'_>,
            id: &str,
        ) -> zbus::Result<()>;

        #[zbus(signal, name = "desktopsChanged")]
        async fn emit_desktops_changed(
            emitter: &zbus::object_server::SignalEmitter<'_>,
            desktops: Vec<(i32, String, String)>,
        ) -> zbus::Result<()>;
    }

    #[test]
    fn desktop_mapping_preserves_kwin_ids_and_active_state() {
        let workspaces = map_desktops(
            vec![
                (0, "desk-a".into(), "Main".into()),
                (1, "desk-b".into(), String::new()),
            ],
            "desk-b",
        );
        assert_eq!(workspaces[0].id.as_str(), "kwin:desk-a");
        assert_eq!(workspaces[0].name, "Main");
        assert!(!workspaces[0].active);
        assert_eq!(workspaces[1].name, "Desktop 2");
        assert!(workspaces[1].active);
    }

    #[test]
    fn event_ids_reject_empty_oversized_and_line_injection_values() {
        assert!(valid_event_id("e6fb540b-2d17-4a75-8bdb-4dd7fcd1"));
        assert!(!valid_event_id(""));
        assert!(!valid_event_id("desktop\nshutdown"));
        assert!(!valid_event_id(&"a".repeat(MAX_EVENT_ID_BYTES + 1)));
    }

    #[tokio::test]
    async fn dbus_workspace_list_and_switch_use_the_kwin_interface() {
        let _lock = DBUS_TEST_LOCK.lock().await;
        let state = Arc::new(Mutex::new(MockState {
            desktops: vec![
                (1, "desk-a".into(), "Main".into()),
                (2, "desk-b".into(), "Chat".into()),
            ],
            current: "desk-a".into(),
        }));
        let _service = zbus::connection::Builder::session()
            .unwrap()
            .name(SERVICE)
            .unwrap()
            .serve_at(PATH, MockKWin(state.clone()))
            .unwrap()
            .build()
            .await
            .unwrap();
        let client = Connection::session().await.unwrap();
        let adapter = KWinAdapter::with_connection(client);

        let workspaces = adapter.workspaces().await.unwrap();
        assert_eq!(workspaces.len(), 2);
        assert_eq!(workspaces[0].id.as_str(), "kwin:desk-a");
        assert!(workspaces[0].active);
        assert!(!workspaces[1].active);

        adapter
            .switch_workspace(&WorkspaceId::new("kwin:desk-b"))
            .await
            .unwrap();
        assert_eq!(state.lock().unwrap().current, "desk-b");
        assert!(
            adapter
                .switch_workspace(&WorkspaceId::new("kwin:missing"))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn current_desktop_signal_is_forwarded_as_a_bounded_event_line() {
        use tokio::io::{AsyncBufReadExt, BufReader};

        let _lock = DBUS_TEST_LOCK.lock().await;
        let _service = zbus::connection::Builder::session()
            .unwrap()
            .name(SERVICE)
            .unwrap()
            .serve_at(
                PATH,
                MockKWin(Arc::new(Mutex::new(MockState {
                    desktops: vec![(0, "desk-a".into(), "Main".into())],
                    current: "desk-a".into(),
                }))),
            )
            .unwrap()
            .build()
            .await
            .unwrap();
        let client = Connection::session().await.unwrap();
        let adapter = KWinAdapter::with_connection(client);
        let inspection = GenericProxy::new(
            adapter.connection.as_ref().unwrap(),
            SERVICE,
            PATH,
            INTERFACE,
        )
        .await
        .unwrap()
        .introspect()
        .await
        .unwrap();
        assert!(inspection.contains("<signal name=\"currentChanged\">"));
        let event_socket = adapter.event_stream().await.unwrap();

        let interface = _service
            .object_server()
            .interface::<_, MockKWin>(PATH)
            .await
            .unwrap();
        MockKWin::emit_current_changed(interface.signal_emitter(), "desk-a")
            .await
            .unwrap();

        let mut reader = BufReader::new(event_socket);
        let mut line = String::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            reader.read_line(&mut line),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(line, "workspace>>kwin:desk-a\n");

        MockKWin::emit_desktops_changed(
            interface.signal_emitter(),
            vec![(0, "desk-a".into(), "Main".into())],
        )
        .await
        .unwrap();
        line.clear();
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            reader.read_line(&mut line),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(line, "workspaces>>kwin:changed\n");
    }
}
