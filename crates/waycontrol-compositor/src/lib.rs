//! Stable compositor-facing API shared by WayControl's daemon and adapters.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::net::UnixStream;
use waycontrol_core::{Monitor, Window, Workspace, WorkspaceId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompositorKind {
    Hyprland,
    Sway,
    Niri,
    KdeKWin,
    Gnome,
    Unknown,
}

impl CompositorKind {
    pub const fn id(self) -> &'static str {
        match self {
            Self::Hyprland => "hyprland",
            Self::Sway => "sway",
            Self::Niri => "niri",
            Self::KdeKWin => "kde_kwin",
            Self::Gnome => "gnome",
            Self::Unknown => "unknown",
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Hyprland => "Hyprland",
            Self::Sway => "Sway",
            Self::Niri => "Niri",
            Self::KdeKWin => "KDE KWin",
            Self::Gnome => "GNOME",
            Self::Unknown => "Unknown compositor",
        }
    }
}

/// Detect a desktop using a captured environment, allowing deterministic tests.
/// Socket variables are stronger evidence than the generic desktop labels.
pub fn detect_compositor_from(env: &[(String, String)]) -> CompositorKind {
    let get = |key: &str| {
        env.iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
            .unwrap_or_default()
    };
    let current = get("XDG_CURRENT_DESKTOP");
    let session = get("XDG_SESSION_DESKTOP");
    let desktop = format!("{current}:{session}").to_ascii_lowercase();

    if !get("HYPRLAND_INSTANCE_SIGNATURE").is_empty()
        || desktop.split([':', ';']).any(|part| part == "hyprland")
    {
        CompositorKind::Hyprland
    } else if !get("SWAYSOCK").is_empty() || desktop.split([':', ';']).any(|part| part == "sway") {
        CompositorKind::Sway
    } else if !get("NIRI_SOCKET").is_empty() || desktop.split([':', ';']).any(|part| part == "niri")
    {
        CompositorKind::Niri
    } else if desktop
        .split([':', ';'])
        .any(|part| matches!(part, "kde" | "plasma" | "plasmawayland"))
    {
        CompositorKind::KdeKWin
    } else if desktop
        .split([':', ';'])
        .any(|part| matches!(part, "gnome" | "gnome-wayland"))
    {
        CompositorKind::Gnome
    } else {
        CompositorKind::Unknown
    }
}

pub fn detect_compositor() -> CompositorKind {
    detect_compositor_from(&std::env::vars().collect::<Vec<_>>())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompositorInfo {
    pub id: String,
    pub name: String,
    pub version: Option<String>,
}

impl CompositorInfo {
    pub fn detected(kind: CompositorKind) -> Self {
        Self {
            id: kind.id().into(),
            name: kind.display_name().into(),
            version: None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompositorCapabilities {
    pub workspaces: bool,
    pub switch_workspace: bool,
    pub windows: bool,
    pub focus_window: bool,
    pub move_window: bool,
    pub move_window_to_monitor: bool,
    pub toggle_floating: bool,
    pub toggle_fullscreen: bool,
    pub toggle_pinned: bool,
    pub close_window: bool,
    pub monitors: bool,
    pub monitor_power: bool,
    pub events: bool,
    pub lock_screen: bool,
}

impl CompositorCapabilities {
    pub const UNSUPPORTED: Self = Self::none();
    pub const HYPRLAND: Self = Self {
        workspaces: true,
        switch_workspace: true,
        windows: true,
        focus_window: true,
        move_window: true,
        move_window_to_monitor: true,
        toggle_floating: true,
        toggle_fullscreen: true,
        toggle_pinned: true,
        close_window: true,
        monitors: true,
        monitor_power: true,
        events: true,
        lock_screen: true,
    };

    pub const fn none() -> Self {
        Self {
            workspaces: false,
            switch_workspace: false,
            windows: false,
            focus_window: false,
            move_window: false,
            move_window_to_monitor: false,
            toggle_floating: false,
            toggle_fullscreen: false,
            toggle_pinned: false,
            close_window: false,
            monitors: false,
            monitor_power: false,
            events: false,
            lock_screen: false,
        }
    }
}

#[async_trait]
pub trait CompositorBackend: Send + Sync + 'static {
    fn info(&self) -> CompositorInfo;
    fn capabilities(&self) -> CompositorCapabilities;
    async fn version(&self) -> Option<String> {
        None
    }
    async fn workspaces(&self) -> Result<Vec<Workspace>, String>;
    async fn windows(&self) -> Result<Vec<Window>, String>;
    async fn switch_workspace(&self, id: &WorkspaceId) -> Result<(), String>;
    async fn focus_window(&self, id: &str) -> Result<(), String>;
    async fn move_window(&self, id: &str, workspace_id: &WorkspaceId) -> Result<(), String>;
    async fn move_window_to_monitor(&self, _id: &str, _monitor: &str) -> Result<(), String> {
        Err("moving windows between monitors is unsupported by this compositor".into())
    }
    async fn toggle_floating(&self, id: &str) -> Result<(), String>;
    async fn toggle_fullscreen(&self, id: &str) -> Result<(), String>;
    async fn toggle_pinned(&self, _id: &str) -> Result<(), String> {
        Err("pinning windows is unsupported by this compositor".into())
    }
    async fn close_window(&self, id: &str) -> Result<(), String>;
    async fn monitors(&self) -> Result<Vec<Monitor>, String>;
    async fn set_monitor_power(&self, name: &str, enabled: bool) -> Result<(), String>;
    async fn lock_screen(&self) -> Result<(), String>;
    async fn event_stream(&self) -> Result<UnixStream, String>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(values: &[(&str, &str)]) -> Vec<(String, String)> {
        values
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect()
    }

    #[test]
    fn compositor_signatures_take_precedence_over_generic_desktop_names() {
        assert_eq!(
            detect_compositor_from(&env(&[
                ("XDG_CURRENT_DESKTOP", "KDE"),
                ("HYPRLAND_INSTANCE_SIGNATURE", "instance-1"),
            ])),
            CompositorKind::Hyprland
        );
        assert_eq!(
            detect_compositor_from(&env(&[("SWAYSOCK", "/run/user/1000/sway-ipc.sock")])),
            CompositorKind::Sway
        );
        assert_eq!(
            detect_compositor_from(&env(&[("NIRI_SOCKET", "/run/user/1000/niri.sock")])),
            CompositorKind::Niri
        );
    }

    #[test]
    fn desktop_names_detect_kwin_and_gnome_and_unknown_defaults_safely() {
        assert_eq!(
            detect_compositor_from(&env(&[("XDG_CURRENT_DESKTOP", "KDE:Plasma")])),
            CompositorKind::KdeKWin
        );
        assert_eq!(
            detect_compositor_from(&env(&[("XDG_CURRENT_DESKTOP", "GNOME")])),
            CompositorKind::Gnome
        );
        assert_eq!(
            detect_compositor_from(&env(&[("XDG_CURRENT_DESKTOP", "Weston")])),
            CompositorKind::Unknown
        );
    }

    #[test]
    fn unsupported_adapters_have_no_capabilities() {
        assert_eq!(
            CompositorCapabilities::UNSUPPORTED,
            CompositorCapabilities::none()
        );
        let hyprland: CompositorCapabilities = serde_json::from_str(
            &serde_json::to_string(&CompositorCapabilities::HYPRLAND).unwrap(),
        )
        .unwrap();
        assert!(hyprland.workspaces);
        assert!(hyprland.events);
        assert!(hyprland.lock_screen);
    }
}
