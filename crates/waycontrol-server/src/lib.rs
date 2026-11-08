//! Authenticated API and local pairing state for the WayControl daemon.

use async_trait::async_trait;
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{
        IntoResponse,
        sse::{Event, Sse},
    },
    routing::{get, post},
};
use nvml_wrapper::{Nvml, enum_wrappers::device::TemperatureSensor};
use rcgen::{CertifiedKey, generate_simple_self_signed};
use rustls_pki_types::{CertificateDer, pem::PemObject};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    convert::Infallible,
    ffi::{CStr, CString},
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    path::{Path as FsPath, PathBuf},
    sync::Arc,
    time::Duration,
};
use thiserror::Error;
use tokio::net::{UdpSocket, UnixStream};
use tokio::sync::Mutex;
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt},
    sync::Semaphore,
};
use waycontrol_backend_hyprland::HyprlandAdapter;
use waycontrol_backend_kwin::KWinAdapter;
use waycontrol_backend_niri::NiriAdapter;
use waycontrol_backend_sway::SwayAdapter;
use waycontrol_compositor::{
    CompositorBackend, CompositorCapabilities, CompositorInfo, CompositorKind, detect_compositor,
};
use waycontrol_core::{Monitor, Window, Workspace};
use waycontrol_protocol::{
    Operation, PROTOCOL_VERSION, PowerAction, RequestEnvelope, ResponseEnvelope, ResponseError,
    VolumeDirection,
};

const PAIRING_TTL: Duration = Duration::from_secs(180);
const PENDING_PAIR_TTL: Duration = Duration::from_secs(300);
const PAIRING_RATE_WINDOW: Duration = Duration::from_secs(60);
const PAIRING_ATTEMPT_LIMIT: u8 = 8;
const API_REQUESTS_PER_DEVICE_PER_SECOND: u16 = 30;
const EVENT_REAUTH_INTERVAL: Duration = Duration::from_secs(5);
const MAX_EVENT_BYTES: usize = 8192;
const MAX_MPRIS_PLAYERS: usize = 16;
pub const LAN_DISCOVERY_PORT: u16 = 8766;
pub const LAN_DISCOVERY_PROBE: &[u8] = b"WAYCONTROL_DISCOVER_V1";
const LAN_DISCOVERY_RATE_LIMIT: Duration = Duration::from_secs(1);
const LAN_DISCOVERY_CLIENT_LIMIT: usize = 256;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LanDiscoveryAnnouncement {
    pub version: u8,
    pub name: String,
    pub https_port: u16,
}

pub fn lan_discovery_bind_address(api_bind: SocketAddr) -> Option<SocketAddr> {
    let IpAddr::V4(ip) = api_bind.ip() else {
        return None;
    };
    if !(ip.is_private() || ip.is_link_local()) {
        return None;
    }
    Some(SocketAddr::from((ip, LAN_DISCOVERY_PORT)))
}

pub fn discovery_probe_is_valid(payload: &[u8]) -> bool {
    payload == LAN_DISCOVERY_PROBE
}

pub fn discovery_source_is_local(source: SocketAddr) -> bool {
    let IpAddr::V4(ip) = source.ip() else {
        return false;
    };
    ip.is_private() || ip.is_link_local() || ip.is_loopback()
}

pub fn lan_discovery_announcement(hostname: &str, https_port: u16) -> LanDiscoveryAnnouncement {
    LanDiscoveryAnnouncement {
        version: 1,
        name: hostname
            .chars()
            .filter(|character| !character.is_control())
            .take(64)
            .collect::<String>()
            .trim()
            .to_owned(),
        https_port,
    }
}

pub async fn run_lan_discovery(
    socket: UdpSocket,
    api_bind: SocketAddr,
    hostname: &str,
) -> std::io::Result<()> {
    let announcement = lan_discovery_announcement(hostname, api_bind.port());
    let response = serde_json::to_vec(&announcement).expect("discovery model serializes");
    let mut buffer = [0_u8; 128];
    let mut last_reply_by_client = HashMap::<IpAddr, tokio::time::Instant>::new();

    loop {
        let (length, peer) = socket.recv_from(&mut buffer).await?;
        let now = tokio::time::Instant::now();
        if last_reply_by_client.len() >= LAN_DISCOVERY_CLIENT_LIMIT {
            last_reply_by_client
                .retain(|_, last| now.saturating_duration_since(*last) < Duration::from_secs(60));
        }
        if !discovery_probe_is_valid(&buffer[..length]) || !discovery_source_is_local(peer) {
            continue;
        }
        if let Some(last) = last_reply_by_client.get(&peer.ip())
            && now.saturating_duration_since(*last) < LAN_DISCOVERY_RATE_LIMIT
        {
            continue;
        }
        if last_reply_by_client.len() >= LAN_DISCOVERY_CLIENT_LIMIT {
            continue;
        }

        let IpAddr::V4(endpoint_ip) = api_bind.ip() else {
            continue;
        };
        if !(endpoint_ip.is_private() || endpoint_ip.is_link_local() || endpoint_ip.is_loopback()) {
            continue;
        }

        socket.send_to(&response, peer).await?;
        last_reply_by_client.insert(peer.ip(), now);
    }
}
pub const MAX_API_BODY_BYTES: usize = 64 * 1024;

struct RequestRateWindow {
    started: tokio::time::Instant,
    count: u16,
}

impl RequestRateWindow {
    fn allow_at(&mut self, now: tokio::time::Instant) -> bool {
        if now.saturating_duration_since(self.started) >= Duration::from_secs(1) {
            self.started = now;
            self.count = 0;
        }
        if self.count >= API_REQUESTS_PER_DEVICE_PER_SECOND {
            return false;
        }
        self.count += 1;
        true
    }
}

#[async_trait]
pub trait SystemControlBackend: Send + Sync + 'static {
    async fn system_power(&self, action: PowerAction) -> Result<(), String>;
    async fn adjust_volume(&self, direction: VolumeDirection) -> Result<(), String>;
    async fn audio_state(&self) -> Result<AudioState, String>;
    async fn set_volume_percent(&self, percent: u8) -> Result<(), String>;
    async fn toggle_output_mute(&self) -> Result<(), String>;
    async fn select_audio_output(&self, id: u32) -> Result<(), String>;
    async fn select_audio_input(&self, id: u32) -> Result<(), String>;
    async fn toggle_microphone_mute(&self) -> Result<(), String>;
    async fn media_play_pause(&self) -> Result<(), String>;
    async fn media_state(&self) -> Result<MediaState, String>;
    async fn media_play_pause_for(&self, player_id: &str) -> Result<(), String>;
    async fn media_next(&self, player_id: &str) -> Result<(), String>;
    async fn media_previous(&self, player_id: &str) -> Result<(), String>;
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AudioState {
    pub output_volume_percent: u8,
    pub output_muted: bool,
    pub microphone_muted: Option<bool>,
    pub output_devices: Vec<AudioDevice>,
    pub input_devices: Vec<AudioDevice>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AudioDevice {
    pub id: u32,
    pub name: String,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MediaState {
    pub players: Vec<MediaPlayer>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MediaPlayer {
    pub player_id: String,
    pub name: String,
    pub playback_status: String,
    pub title: String,
    pub artist: String,
    pub album: String,
}

pub enum WaylandBackend {
    Hyprland(HyprlandAdapter),
    Sway(SwayAdapter),
    Niri(NiriAdapter),
    KWin(KWinAdapter),
    Unsupported(CompositorInfo),
}

impl WaylandBackend {
    pub fn detect() -> Self {
        let kind = detect_compositor();
        let discovered_hyprland =
            kind == CompositorKind::Unknown && HyprlandAdapter::can_discover_active_session();
        Self::for_detected_kind(kind, discovered_hyprland)
    }

    pub fn for_kind(kind: CompositorKind) -> Self {
        Self::for_detected_kind(kind, false)
    }

    fn for_detected_kind(kind: CompositorKind, hyprland_discovered: bool) -> Self {
        if kind == CompositorKind::Hyprland
            || (kind == CompositorKind::Unknown && hyprland_discovered)
        {
            Self::Hyprland(HyprlandAdapter::from_env_or_unavailable())
        } else if kind == CompositorKind::Sway {
            Self::Sway(SwayAdapter::from_env_or_unavailable())
        } else if kind == CompositorKind::Niri {
            Self::Niri(NiriAdapter::from_env_or_unavailable())
        } else if kind == CompositorKind::KdeKWin {
            Self::KWin(KWinAdapter::default())
        } else {
            Self::Unsupported(CompositorInfo::detected(kind))
        }
    }

    fn unsupported(&self) -> String {
        match self {
            Self::Hyprland(_) | Self::Sway(_) | Self::Niri(_) | Self::KWin(_) => {
                "unreachable unsupported compositor operation".into()
            }
            Self::Unsupported(info) => format!(
                "{} was detected, but its WayControl compositor adapter is not implemented",
                info.name
            ),
        }
    }
}

#[async_trait]
impl CompositorBackend for WaylandBackend {
    fn info(&self) -> CompositorInfo {
        match self {
            Self::Hyprland(adapter) => adapter.info(),
            Self::Sway(adapter) => adapter.info(),
            Self::Niri(adapter) => adapter.info(),
            Self::KWin(adapter) => adapter.info(),
            Self::Unsupported(info) => info.clone(),
        }
    }

    fn capabilities(&self) -> CompositorCapabilities {
        match self {
            Self::Hyprland(adapter) => adapter.capabilities(),
            Self::Sway(adapter) => adapter.capabilities(),
            Self::Niri(adapter) => adapter.capabilities(),
            Self::KWin(adapter) => adapter.capabilities(),
            Self::Unsupported(_) => CompositorCapabilities::UNSUPPORTED,
        }
    }

    async fn workspaces(&self) -> Result<Vec<Workspace>, String> {
        match self {
            Self::Hyprland(adapter) => adapter.workspaces().await,
            Self::Sway(adapter) => adapter.workspaces().await,
            Self::Niri(adapter) => adapter.workspaces().await,
            Self::KWin(adapter) => adapter.workspaces().await,
            _ => Err(self.unsupported()),
        }
    }
    async fn windows(&self) -> Result<Vec<Window>, String> {
        match self {
            Self::Hyprland(adapter) => adapter.windows().await,
            Self::Sway(adapter) => adapter.windows().await,
            Self::Niri(adapter) => adapter.windows().await,
            Self::KWin(adapter) => adapter.windows().await,
            _ => Err(self.unsupported()),
        }
    }
    async fn switch_workspace(&self, id: &waycontrol_core::WorkspaceId) -> Result<(), String> {
        match self {
            Self::Hyprland(adapter) => adapter.switch_workspace(id).await,
            Self::Sway(adapter) => adapter.switch_workspace(id).await,
            Self::Niri(adapter) => adapter.switch_workspace(id).await,
            Self::KWin(adapter) => adapter.switch_workspace(id).await,
            _ => Err(self.unsupported()),
        }
    }
    async fn focus_window(&self, id: &str) -> Result<(), String> {
        match self {
            Self::Hyprland(adapter) => adapter.focus_window(id).await,
            Self::Sway(adapter) => adapter.focus_window(id).await,
            Self::Niri(adapter) => adapter.focus_window(id).await,
            Self::KWin(adapter) => adapter.focus_window(id).await,
            _ => Err(self.unsupported()),
        }
    }
    async fn move_window(
        &self,
        id: &str,
        workspace_id: &waycontrol_core::WorkspaceId,
    ) -> Result<(), String> {
        match self {
            Self::Hyprland(adapter) => adapter.move_window(id, workspace_id).await,
            Self::Sway(adapter) => adapter.move_window(id, workspace_id).await,
            Self::Niri(adapter) => adapter.move_window(id, workspace_id).await,
            Self::KWin(adapter) => adapter.move_window(id, workspace_id).await,
            _ => Err(self.unsupported()),
        }
    }
    async fn toggle_floating(&self, id: &str) -> Result<(), String> {
        match self {
            Self::Hyprland(adapter) => adapter.toggle_floating(id).await,
            Self::Sway(adapter) => adapter.toggle_floating(id).await,
            Self::Niri(adapter) => adapter.toggle_floating(id).await,
            Self::KWin(adapter) => adapter.toggle_floating(id).await,
            _ => Err(self.unsupported()),
        }
    }
    async fn toggle_fullscreen(&self, id: &str) -> Result<(), String> {
        match self {
            Self::Hyprland(adapter) => adapter.toggle_fullscreen(id).await,
            Self::Sway(adapter) => adapter.toggle_fullscreen(id).await,
            Self::Niri(adapter) => adapter.toggle_fullscreen(id).await,
            Self::KWin(adapter) => adapter.toggle_fullscreen(id).await,
            _ => Err(self.unsupported()),
        }
    }
    async fn close_window(&self, id: &str) -> Result<(), String> {
        match self {
            Self::Hyprland(adapter) => adapter.close_window(id).await,
            Self::Sway(adapter) => adapter.close_window(id).await,
            Self::Niri(adapter) => adapter.close_window(id).await,
            Self::KWin(adapter) => adapter.close_window(id).await,
            _ => Err(self.unsupported()),
        }
    }
    async fn monitors(&self) -> Result<Vec<Monitor>, String> {
        match self {
            Self::Hyprland(adapter) => adapter.monitors().await,
            Self::Sway(adapter) => adapter.monitors().await,
            Self::Niri(adapter) => adapter.monitors().await,
            Self::KWin(adapter) => adapter.monitors().await,
            _ => Err(self.unsupported()),
        }
    }
    async fn set_monitor_power(&self, name: &str, enabled: bool) -> Result<(), String> {
        match self {
            Self::Hyprland(adapter) => adapter.set_monitor_power(name, enabled).await,
            Self::Sway(adapter) => adapter.set_monitor_power(name, enabled).await,
            Self::Niri(adapter) => adapter.set_monitor_power(name, enabled).await,
            Self::KWin(adapter) => adapter.set_monitor_power(name, enabled).await,
            _ => Err(self.unsupported()),
        }
    }
    async fn lock_screen(&self) -> Result<(), String> {
        match self {
            Self::Hyprland(adapter) => adapter.lock_screen().await,
            Self::Sway(adapter) => adapter.lock_screen().await,
            Self::Niri(adapter) => adapter.lock_screen().await,
            Self::KWin(adapter) => adapter.lock_screen().await,
            _ => Err(self.unsupported()),
        }
    }
    async fn event_stream(&self) -> Result<UnixStream, String> {
        match self {
            Self::Hyprland(adapter) => adapter.event_stream().await,
            Self::Sway(adapter) => adapter.event_stream().await,
            Self::Niri(adapter) => adapter.event_stream().await,
            Self::KWin(adapter) => adapter.event_stream().await,
            _ => Err(self.unsupported()),
        }
    }
}

pub struct HostSystemControls;

#[async_trait]
impl SystemControlBackend for HostSystemControls {
    async fn system_power(&self, action: PowerAction) -> Result<(), String> {
        let action = match action {
            PowerAction::Suspend => "suspend",
            PowerAction::Shutdown => "poweroff",
            PowerAction::Reboot => "reboot",
        };
        spawn_allowlisted("/usr/bin/systemctl", &[action]).await
    }
    async fn adjust_volume(&self, direction: VolumeDirection) -> Result<(), String> {
        let value = match direction {
            VolumeDirection::Up => "5%+",
            VolumeDirection::Down => "5%-",
        };
        run_allowlisted(
            "/usr/bin/wpctl",
            &["set-volume", "@DEFAULT_SINK@", value, "--limit", "1.0"],
        )
        .await
    }
    async fn audio_state(&self) -> Result<AudioState, String> {
        let device_output = run_allowlisted_output("/usr/bin/wpctl", &["status", "-n"]).await?;
        let (output_devices, input_devices) = parse_wpctl_audio_devices(&device_output)?;
        let output =
            run_allowlisted_output("/usr/bin/wpctl", &["get-volume", "@DEFAULT_SINK@"]).await?;
        let mut state = parse_wpctl_audio_state(&output)?;
        state.output_devices = output_devices;
        state.input_devices = input_devices;
        state.microphone_muted =
            run_allowlisted_output("/usr/bin/wpctl", &["get-volume", "@DEFAULT_SOURCE@"])
                .await
                .ok()
                .and_then(|output| parse_wpctl_mute_state(&output).ok());
        Ok(state)
    }
    async fn set_volume_percent(&self, percent: u8) -> Result<(), String> {
        run_allowlisted(
            "/usr/bin/wpctl",
            &[
                "set-volume",
                "@DEFAULT_SINK@",
                &format!("{percent}%"),
                "--limit",
                "1.0",
            ],
        )
        .await
    }
    async fn toggle_output_mute(&self) -> Result<(), String> {
        run_allowlisted("/usr/bin/wpctl", &["set-mute", "@DEFAULT_SINK@", "toggle"]).await
    }
    async fn select_audio_output(&self, id: u32) -> Result<(), String> {
        self.select_audio_device(id, true).await
    }
    async fn select_audio_input(&self, id: u32) -> Result<(), String> {
        self.select_audio_device(id, false).await
    }
    async fn toggle_microphone_mute(&self) -> Result<(), String> {
        run_allowlisted(
            "/usr/bin/wpctl",
            &["set-mute", "@DEFAULT_SOURCE@", "toggle"],
        )
        .await
    }
    async fn media_play_pause(&self) -> Result<(), String> {
        run_allowlisted("/usr/bin/playerctl", &["play-pause"]).await
    }
    async fn media_state(&self) -> Result<MediaState, String> {
        let output = run_allowlisted_output("/usr/bin/playerctl", &["--list-all"]).await?;
        let mut players = Vec::new();
        for player_id in parse_playerctl_list(&output) {
            let status =
                run_allowlisted_output("/usr/bin/playerctl", &["--player", &player_id, "status"])
                    .await;
            let metadata = run_allowlisted_output(
                "/usr/bin/playerctl",
                &[
                    "--player",
                    &player_id,
                    "--format",
                    "{{title}}\t{{artist}}\t{{album}}",
                    "metadata",
                ],
            )
            .await;
            let (Ok(status), Ok(metadata)) = (status, metadata) else {
                continue;
            };
            let (title, artist, album) = parse_playerctl_metadata(&metadata);
            players.push(MediaPlayer {
                name: playerctl_display_name(&player_id),
                player_id,
                playback_status: sanitize_media_text(&status, 32),
                title,
                artist,
                album,
            });
        }
        Ok(MediaState { players })
    }
    async fn media_play_pause_for(&self, player_id: &str) -> Result<(), String> {
        self.control_player(player_id, "play-pause").await
    }
    async fn media_next(&self, player_id: &str) -> Result<(), String> {
        self.control_player(player_id, "next").await
    }
    async fn media_previous(&self, player_id: &str) -> Result<(), String> {
        self.control_player(player_id, "previous").await
    }
}

impl HostSystemControls {
    async fn control_player(&self, player_id: &str, command: &str) -> Result<(), String> {
        if !valid_playerctl_id(player_id) {
            return Err("media player id is invalid".into());
        }
        let listed = run_allowlisted_output("/usr/bin/playerctl", &["--list-all"]).await?;
        if !parse_playerctl_list(&listed)
            .iter()
            .any(|id| id == player_id)
        {
            return Err("media player is no longer available".into());
        }
        run_allowlisted("/usr/bin/playerctl", &["--player", player_id, command]).await
    }

    async fn select_audio_device(&self, id: u32, output: bool) -> Result<(), String> {
        let status = run_allowlisted_output("/usr/bin/wpctl", &["status", "-n"]).await?;
        let (outputs, inputs) = parse_wpctl_audio_devices(&status)?;
        if !audio_device_is_selectable(id, output, &outputs, &inputs) {
            return Err("audio device is not available for the requested direction".into());
        }
        run_allowlisted("/usr/bin/wpctl", &["set-default", &id.to_string()]).await
    }
}

fn valid_playerctl_id(player_id: &str) -> bool {
    !player_id.is_empty()
        && player_id.len() <= waycontrol_protocol::MAX_MEDIA_PLAYER_ID_BYTES
        && player_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn parse_playerctl_list(output: &str) -> Vec<String> {
    let mut players = Vec::new();
    for line in output.lines().map(str::trim).take(MAX_MPRIS_PLAYERS) {
        if valid_playerctl_id(line) && !players.iter().any(|id| id == line) {
            players.push(line.to_owned());
        }
    }
    players
}

fn parse_playerctl_metadata(output: &str) -> (String, String, String) {
    let mut fields = output.trim_end_matches(['\r', '\n']).splitn(3, '\t');
    (
        sanitize_media_text(fields.next().unwrap_or_default(), 512),
        sanitize_media_text(fields.next().unwrap_or_default(), 512),
        sanitize_media_text(fields.next().unwrap_or_default(), 512),
    )
}

fn playerctl_display_name(player_id: &str) -> String {
    player_id
        .strip_prefix("org.mpris.MediaPlayer2.")
        .unwrap_or(player_id)
        .replace(['.', '_', '-'], " ")
}

fn sanitize_media_text(value: &str, max_chars: usize) -> String {
    value
        .chars()
        .filter(|character| !character.is_control())
        .take(max_chars)
        .collect::<String>()
        .trim()
        .to_owned()
}

async fn spawn_allowlisted(program: &str, args: &[&str]) -> Result<(), String> {
    tokio::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("could not start {program}: {error}"))
}

async fn run_allowlisted(program: &str, args: &[&str]) -> Result<(), String> {
    let output = tokio::time::timeout(
        Duration::from_secs(5),
        tokio::process::Command::new(program)
            .args(args)
            .stdin(std::process::Stdio::null())
            .output(),
    )
    .await
    .map_err(|_| format!("{program} timed out"))?
    .map_err(|error| format!("could not start {program}: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

async fn run_allowlisted_output(program: &str, args: &[&str]) -> Result<String, String> {
    let output = tokio::time::timeout(
        Duration::from_secs(5),
        tokio::process::Command::new(program)
            .args(args)
            .stdin(std::process::Stdio::null())
            .output(),
    )
    .await
    .map_err(|_| format!("{program} timed out"))?
    .map_err(|error| format!("could not start {program}: {error}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

fn parse_wpctl_audio_state(output: &str) -> Result<AudioState, String> {
    let mut parts = output.split_whitespace();
    if parts.next() != Some("Volume:") {
        return Err("unexpected wpctl volume output".into());
    }
    let volume = parts
        .next()
        .ok_or_else(|| "wpctl volume output is missing a value".to_owned())?
        .parse::<f64>()
        .map_err(|_| "wpctl volume output has an invalid value".to_owned())?;
    if !volume.is_finite() || volume < 0.0 {
        return Err("wpctl volume output has an invalid value".into());
    }
    let muted = parts.any(|part| part == "[MUTED]");
    Ok(AudioState {
        output_volume_percent: (volume * 100.0).round().clamp(0.0, 100.0) as u8,
        output_muted: muted,
        microphone_muted: None,
        output_devices: Vec::new(),
        input_devices: Vec::new(),
    })
}

fn parse_wpctl_mute_state(output: &str) -> Result<bool, String> {
    parse_wpctl_audio_state(output).map(|state| state.output_muted)
}

fn parse_wpctl_audio_devices(output: &str) -> Result<(Vec<AudioDevice>, Vec<AudioDevice>), String> {
    const MAX_STATUS_BYTES: usize = 64 * 1024;
    if output.len() > MAX_STATUS_BYTES {
        return Err("wpctl status output is too large".into());
    }
    #[derive(Clone, Copy)]
    enum Section {
        None,
        Outputs,
        Inputs,
    }
    let mut in_audio = false;
    let mut audio_seen = false;
    let mut section = Section::None;
    let mut outputs = Vec::new();
    let mut inputs = Vec::new();
    for line in output.lines() {
        let content = line.trim().trim_start_matches(['│', '├', '└', '─', ' ']);
        match content {
            "Audio" => {
                in_audio = true;
                audio_seen = true;
                section = Section::None;
                continue;
            }
            "Video" | "Settings" => {
                in_audio = false;
                section = Section::None;
                continue;
            }
            _ => {}
        }
        if !in_audio {
            continue;
        }
        if content.ends_with("Sinks:") {
            section = Section::Outputs;
            continue;
        }
        if content.ends_with("Sources:") {
            section = Section::Inputs;
            continue;
        }
        if content.ends_with("Devices:")
            || content.ends_with("Filters:")
            || content.ends_with("Streams:")
        {
            section = Section::None;
            continue;
        }
        let active = content.starts_with('*');
        let entry = content.strip_prefix('*').unwrap_or(content).trim();
        let Some((id, details)) = entry.split_once('.') else {
            continue;
        };
        let Ok(id) = id.trim().parse::<u32>() else {
            continue;
        };
        let name = details
            .split_once(" [")
            .map_or(details, |(name, _)| name)
            .trim();
        if name.is_empty() || name.len() > 256 {
            continue;
        }
        let device = AudioDevice {
            id,
            name: name.to_owned(),
            active,
        };
        match section {
            Section::Outputs => outputs.push(device),
            Section::Inputs => inputs.push(device),
            Section::None => {}
        }
    }
    if !audio_seen {
        return Err("wpctl status output has no Audio section".into());
    }
    Ok((outputs, inputs))
}

fn audio_device_is_selectable(
    id: u32,
    output: bool,
    outputs: &[AudioDevice],
    inputs: &[AudioDevice],
) -> bool {
    let devices = if output { outputs } else { inputs };
    devices.iter().any(|device| device.id == id)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrustedDevice {
    pub device_id: String,
    pub name: String,
    pub token_sha256: String,
    pub created_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingDevice {
    pub request_id: String,
    pub device_name: String,
    pub created_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingInfo {
    pub code: String,
    pub expires_in_seconds: u64,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PairingError {
    #[error("pairing code is invalid or expired")]
    InvalidCode,
    #[error("pairing request was not found or has expired")]
    UnknownRequest,
    #[error("pairing request limit reached; try again later")]
    RateLimited,
    #[error("invalid device name")]
    InvalidDeviceName,
    #[error("trusted device was not found")]
    UnknownDevice,
    #[error("secure random generation failed")]
    Random,
    #[error("could not persist trusted-device database")]
    Persistence,
}

struct PendingRecord {
    public: PendingDevice,
    token: String,
    approved: bool,
    expires_at: tokio::time::Instant,
}

struct PairingState {
    code: String,
    code_expires: tokio::time::Instant,
    pending: Vec<PendingRecord>,
    attempts: u8,
    attempt_window: tokio::time::Instant,
}

struct Inner {
    compositor: Arc<dyn CompositorBackend>,
    system: Arc<dyn SystemControlBackend>,
    pairing: Mutex<PairingState>,
    devices: Mutex<Vec<TrustedDevice>>,
    device_file: PathBuf,
    event_slots: Arc<Semaphore>,
    api_rate_windows: Mutex<HashMap<String, RequestRateWindow>>,
}

#[derive(Clone)]
pub struct AppState(Arc<Inner>);

impl AppState {
    pub async fn new(
        compositor: Arc<dyn CompositorBackend>,
        system: Arc<dyn SystemControlBackend>,
        data_dir: impl AsRef<FsPath>,
    ) -> Result<Self, PairingError> {
        let data_dir = data_dir.as_ref();
        std::fs::create_dir_all(data_dir).map_err(|_| PairingError::Persistence)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(data_dir, std::fs::Permissions::from_mode(0o700))
                .map_err(|_| PairingError::Persistence)?;
        }
        let device_file = data_dir.join("trusted-devices.json");
        let devices = if device_file.exists() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&device_file, std::fs::Permissions::from_mode(0o600))
                    .map_err(|_| PairingError::Persistence)?;
            }
            serde_json::from_slice(
                &std::fs::read(&device_file).map_err(|_| PairingError::Persistence)?,
            )
            .map_err(|_| PairingError::Persistence)?
        } else {
            Vec::new()
        };
        let now = tokio::time::Instant::now();
        let code = fresh_code()?;
        Ok(Self(Arc::new(Inner {
            compositor,
            system,
            pairing: Mutex::new(PairingState {
                code,
                code_expires: now + PAIRING_TTL,
                pending: Vec::new(),
                attempts: 0,
                attempt_window: now,
            }),
            devices: Mutex::new(devices),
            device_file,
            event_slots: Arc::new(Semaphore::new(4)),
            api_rate_windows: Mutex::new(HashMap::new()),
        })))
    }

    pub async fn pairing_info(&self) -> Result<PairingInfo, PairingError> {
        let mut state = self.0.pairing.lock().await;
        if tokio::time::Instant::now() >= state.code_expires {
            state.code = fresh_code()?;
            state.code_expires = tokio::time::Instant::now() + PAIRING_TTL;
        }
        Ok(PairingInfo {
            code: state.code.clone(),
            expires_in_seconds: state
                .code_expires
                .saturating_duration_since(tokio::time::Instant::now())
                .as_secs(),
        })
    }

    pub async fn begin_pairing(&self, code: &str, name: &str) -> Result<String, PairingError> {
        if !valid_device_name(name) {
            return Err(PairingError::InvalidDeviceName);
        }
        let mut state = self.0.pairing.lock().await;
        let now = tokio::time::Instant::now();
        if now.saturating_duration_since(state.attempt_window) >= PAIRING_RATE_WINDOW {
            state.attempt_window = now;
            state.attempts = 0;
        }
        state.attempts = state.attempts.saturating_add(1);
        if state.attempts > PAIRING_ATTEMPT_LIMIT {
            return Err(PairingError::RateLimited);
        }
        if now >= state.code_expires || !constant_time_eq(code.as_bytes(), state.code.as_bytes()) {
            return Err(PairingError::InvalidCode);
        }
        state.code_expires = now;
        let request_id = random_hex(16)?;
        let token = random_hex(32)?;
        state.pending.push(PendingRecord {
            public: PendingDevice {
                request_id: request_id.clone(),
                device_name: name.trim().to_owned(),
                created_unix: unix_now(),
            },
            token,
            approved: false,
            expires_at: now + PENDING_PAIR_TTL,
        });
        Ok(request_id)
    }

    pub async fn pairing_status(
        &self,
        request_id: &str,
    ) -> Result<Option<(String, String)>, PairingError> {
        let mut state = self.0.pairing.lock().await;
        state
            .pending
            .retain(|pending| pending.expires_at > tokio::time::Instant::now());
        let Some(index) = state
            .pending
            .iter()
            .position(|p| p.public.request_id == request_id)
        else {
            return Err(PairingError::UnknownRequest);
        };
        if !state.pending[index].approved {
            return Ok(None);
        }
        let record = state.pending.remove(index);
        Ok(Some((record.public.device_name, record.token)))
    }

    pub async fn pending_devices(&self) -> Vec<PendingDevice> {
        let mut state = self.0.pairing.lock().await;
        state
            .pending
            .retain(|pending| pending.expires_at > tokio::time::Instant::now());
        state.pending.iter().map(|p| p.public.clone()).collect()
    }

    pub async fn approve_pairing(&self, request_id: &str) -> Result<(), PairingError> {
        let mut pending = self.0.pairing.lock().await;
        pending
            .pending
            .retain(|item| item.expires_at > tokio::time::Instant::now());
        let Some(index) = pending
            .pending
            .iter()
            .position(|p| p.public.request_id == request_id)
        else {
            return Err(PairingError::UnknownRequest);
        };
        if pending.pending[index].approved {
            return Ok(());
        }
        let device = TrustedDevice {
            device_id: random_hex(12)?,
            name: pending.pending[index].public.device_name.clone(),
            token_sha256: token_hash(&pending.pending[index].token),
            created_unix: unix_now(),
        };
        self.add_device(device).await?;
        pending.pending[index].approved = true;
        Ok(())
    }

    pub async fn revoke_device(&self, id: &str) -> Result<(), PairingError> {
        let mut devices = self.0.devices.lock().await;
        let candidate = devices
            .iter()
            .filter(|device| device.device_id != id)
            .cloned()
            .collect::<Vec<_>>();
        if candidate.len() == devices.len() {
            return Err(PairingError::UnknownDevice);
        }
        self.persist_devices(&candidate).await?;
        *devices = candidate;
        self.0.api_rate_windows.lock().await.remove(id);
        Ok(())
    }

    async fn allow_api_request(&self, device_id: &str) -> bool {
        let now = tokio::time::Instant::now();
        let mut windows = self.0.api_rate_windows.lock().await;
        windows.retain(|_, window| {
            now.saturating_duration_since(window.started) < Duration::from_secs(60)
        });
        windows
            .entry(device_id.to_owned())
            .or_insert(RequestRateWindow {
                started: now,
                count: 0,
            })
            .allow_at(now)
    }

    pub async fn devices(&self) -> Vec<TrustedDevice> {
        self.0.devices.lock().await.clone()
    }

    async fn add_device(&self, device: TrustedDevice) -> Result<(), PairingError> {
        let mut devices = self.0.devices.lock().await;
        let mut candidate = devices.clone();
        candidate.push(device);
        self.persist_devices(&candidate).await?;
        *devices = candidate;
        Ok(())
    }

    async fn persist_devices(&self, devices: &[TrustedDevice]) -> Result<(), PairingError> {
        let temp = self.0.device_file.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(devices).map_err(|_| PairingError::Persistence)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .mode(0o600)
                .open(&temp)
                .map_err(|_| PairingError::Persistence)?;
            use std::io::Write;
            file.write_all(&bytes)
                .map_err(|_| PairingError::Persistence)?;
            file.sync_all().map_err(|_| PairingError::Persistence)?;
        }
        #[cfg(not(unix))]
        std::fs::write(&temp, bytes).map_err(|_| PairingError::Persistence)?;
        std::fs::rename(temp, &self.0.device_file).map_err(|_| PairingError::Persistence)
    }

    async fn authenticated_device(&self, token: &str) -> Option<TrustedDevice> {
        let hash = token_hash(token);
        self.0
            .devices
            .lock()
            .await
            .iter()
            .find(|d| constant_time_eq(d.token_sha256.as_bytes(), hash.as_bytes()))
            .cloned()
    }
}

fn valid_device_name(name: &str) -> bool {
    let name = name.trim();
    !name.is_empty()
        && name.len() <= 80
        && name.chars().all(|character| {
            character.is_ascii() && (character.is_ascii_graphic() || character == ' ')
        })
}

#[derive(Debug, Deserialize)]
pub struct PairRequest {
    pub code: String,
    pub device_name: String,
}

#[derive(Debug, Serialize)]
pub struct PairCreated {
    pub request_id: String,
    pub poll_after_ms: u64,
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PairStatus {
    Pending,
    Approved { device_name: String, token: String },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AdminRequest {
    pub action: String,
    #[serde(default)]
    pub id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AdminResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pairing: Option<PairingInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending: Option<Vec<PendingDevice>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub devices: Option<Vec<TrustedDevice>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub certificate_sha256: Option<String>,
}

pub async fn admin_command(state: &AppState, request: AdminRequest) -> AdminResponse {
    match request.action.as_str() {
        "pairing_info" => AdminResponse {
            ok: true,
            pairing: state.pairing_info().await.ok(),
            pending: None,
            devices: None,
            message: None,
            server_address: None,
            certificate_sha256: None,
        },
        "pending" => AdminResponse {
            ok: true,
            pairing: None,
            pending: Some(state.pending_devices().await),
            devices: None,
            message: None,
            server_address: None,
            certificate_sha256: None,
        },
        "devices" => AdminResponse {
            ok: true,
            pairing: None,
            pending: None,
            devices: Some(state.devices().await),
            message: None,
            server_address: None,
            certificate_sha256: None,
        },
        "approve" => {
            let ok = if let Some(id) = request.id {
                state.approve_pairing(&id).await.is_ok()
            } else {
                false
            };
            AdminResponse {
                ok,
                pairing: None,
                pending: None,
                devices: None,
                message: Some(
                    if ok {
                        "pairing approved"
                    } else {
                        "pairing request not found"
                    }
                    .into(),
                ),
                server_address: None,
                certificate_sha256: None,
            }
        }
        "revoke" => {
            let ok = if let Some(id) = request.id {
                state.revoke_device(&id).await.is_ok()
            } else {
                false
            };
            AdminResponse {
                ok,
                pairing: None,
                pending: None,
                devices: None,
                message: Some(
                    if ok {
                        "device revoked"
                    } else {
                        "trusted device not found"
                    }
                    .into(),
                ),
                server_address: None,
                certificate_sha256: None,
            }
        }
        _ => AdminResponse {
            ok: false,
            pairing: None,
            pending: None,
            devices: None,
            message: Some("unknown local action".into()),
            server_address: None,
            certificate_sha256: None,
        },
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/pair", post(pair))
        .route("/api/v1/pair/{request_id}", get(pair_status))
        .route("/api/v1/request", post(request))
        .route("/api/v1/events", get(event_stream))
        .layer(DefaultBodyLimit::max(MAX_API_BODY_BYTES))
        .with_state(state)
}

async fn event_stream(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> axum::response::Response {
    let Some(token) = bearer(&headers) else {
        return api_error(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "trusted device authentication required",
            "events",
        );
    };
    if state.authenticated_device(token).await.is_none() {
        return api_error(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "trusted device authentication required",
            "events",
        );
    }
    if !state.0.compositor.capabilities().events {
        return api_error(
            StatusCode::NOT_IMPLEMENTED,
            "unsupported_operation",
            "event subscriptions are not supported by the detected compositor",
            "events",
        );
    }
    let permit = match state.0.event_slots.clone().try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => {
            return api_error(
                StatusCode::TOO_MANY_REQUESTS,
                "too_many_streams",
                "event stream limit reached",
                "events",
            );
        }
    };
    let socket = match state.0.compositor.event_stream().await {
        Ok(socket) => socket,
        Err(message) => {
            return api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "backend_unavailable",
                &message,
                "events",
            );
        }
    };
    let events = futures_util::stream::unfold(
        (
            tokio::io::BufReader::new(socket),
            permit,
            state,
            token.to_owned(),
            Vec::with_capacity(256),
        ),
        |(mut reader, permit, state, token, mut partial_line)| async move {
            enum Next {
                Event(Vec<u8>),
                Heartbeat,
                Closed,
            }
            let next = tokio::select! {
                line = read_bounded_line(&mut reader, &mut partial_line, MAX_EVENT_BYTES) => {
                    match line {
                        Ok(Some(line)) => Next::Event(line),
                        Ok(None) | Err(_) => Next::Closed,
                    }
                }
                _ = tokio::time::sleep(EVENT_REAUTH_INTERVAL) => Next::Heartbeat,
            };
            match next {
                Next::Event(mut line) => {
                    state.authenticated_device(&token).await?;
                    while matches!(line.last(), Some(b'\n' | b'\r')) {
                        line.pop();
                    }
                    let data = String::from_utf8_lossy(&line).into_owned();
                    Some((
                        Ok::<Event, Infallible>(Event::default().event("compositor").data(data)),
                        (reader, permit, state, token, partial_line),
                    ))
                }
                Next::Heartbeat => {
                    state.authenticated_device(&token).await?;
                    Some((
                        Ok::<Event, Infallible>(Event::default().comment("keepalive")),
                        (reader, permit, state, token, partial_line),
                    ))
                }
                Next::Closed => None,
            }
        },
    );
    Sse::new(events).into_response()
}

async fn read_bounded_line<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    line: &mut Vec<u8>,
    max_bytes: usize,
) -> std::io::Result<Option<Vec<u8>>> {
    loop {
        let (count, complete, chunk) = {
            let available = reader.fill_buf().await?;
            if available.is_empty() {
                return Ok(if line.is_empty() {
                    None
                } else {
                    Some(std::mem::take(line))
                });
            }
            let count = available
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(available.len(), |index| index + 1);
            if line.len().saturating_add(count) > max_bytes {
                return Ok(None);
            }
            (
                count,
                available[count - 1] == b'\n',
                available[..count].to_vec(),
            )
        };
        reader.consume(count);
        line.extend_from_slice(&chunk);
        if complete {
            return Ok(Some(std::mem::take(line)));
        }
    }
}

async fn pair(
    State(state): State<AppState>,
    Json(request): Json<PairRequest>,
) -> impl IntoResponse {
    match state
        .begin_pairing(&request.code, &request.device_name)
        .await
    {
        Ok(request_id) => (
            StatusCode::ACCEPTED,
            Json(PairCreated {
                request_id,
                poll_after_ms: 800,
            }),
        )
            .into_response(),
        Err(error) => pairing_error(error).into_response(),
    }
}

async fn pair_status(
    State(state): State<AppState>,
    Path(request_id): Path<String>,
) -> impl IntoResponse {
    let mut response = match state.pairing_status(&request_id).await {
        Ok(None) => (StatusCode::OK, Json(PairStatus::Pending)).into_response(),
        Ok(Some((device_name, token))) => (
            StatusCode::OK,
            Json(PairStatus::Approved { device_name, token }),
        )
            .into_response(),
        Err(error) => pairing_error(error).into_response(),
    };
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    response
}

async fn request(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<RequestEnvelope>,
) -> impl IntoResponse {
    let Some(token) = bearer(&headers) else {
        return api_error(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "trusted device authentication required",
            "auth",
        );
    };
    let Some(device) = state.authenticated_device(token).await else {
        return api_error(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "trusted device authentication required",
            &request.id,
        );
    };
    if !state.allow_api_request(&device.device_id).await {
        return api_error(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            "trusted device request limit exceeded; retry shortly",
            &request.id,
        );
    }
    if let Err(error) = request.validate() {
        return api_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            &error.to_string(),
            &request.id,
        );
    }
    if let Some(capability) = required_capability(&request.operation) {
        let capabilities = state.0.compositor.capabilities();
        if !capability(&capabilities) {
            return api_error(
                StatusCode::NOT_IMPLEMENTED,
                "unsupported_operation",
                "the detected compositor adapter does not support this operation",
                &request.id,
            );
        }
    }
    let result = match request.operation {
        Operation::Ping => Ok(json!({"pong": true})),
        Operation::DesktopInfo => desktop_snapshot(state.0.compositor.as_ref()).await,
        Operation::ListWorkspaces => state
            .0
            .compositor
            .workspaces()
            .await
            .map(|data| json!(data)),
        Operation::ListWindows => state.0.compositor.windows().await.map(|data| json!(data)),
        Operation::SwitchWorkspace { workspace_id } => state
            .0
            .compositor
            .switch_workspace(&workspace_id)
            .await
            .map(|()| json!({})),
        Operation::FocusWindow { address } => state
            .0
            .compositor
            .focus_window(&address)
            .await
            .map(|()| json!({})),
        Operation::MoveWindow {
            address,
            workspace_id,
        } => state
            .0
            .compositor
            .move_window(&address, &workspace_id)
            .await
            .map(|()| json!({})),
        Operation::ToggleFloating { address } => state
            .0
            .compositor
            .toggle_floating(&address)
            .await
            .map(|()| json!({})),
        Operation::ToggleFullscreen { address } => state
            .0
            .compositor
            .toggle_fullscreen(&address)
            .await
            .map(|()| json!({})),
        Operation::CloseWindow { address } => state
            .0
            .compositor
            .close_window(&address)
            .await
            .map(|()| json!({})),
        Operation::ListMonitors => state.0.compositor.monitors().await.map(|data| json!(data)),
        Operation::SetMonitorPower { monitor, enabled } => state
            .0
            .compositor
            .set_monitor_power(&monitor, enabled)
            .await
            .map(|()| json!({})),
        Operation::LockScreen => state.0.compositor.lock_screen().await.map(|()| json!({})),
        Operation::SystemPower { action } => state
            .0
            .system
            .system_power(action)
            .await
            .map(|()| json!({})),
        Operation::AdjustVolume { direction } => state
            .0
            .system
            .adjust_volume(direction)
            .await
            .map(|()| json!({})),
        Operation::AudioInfo => state.0.system.audio_state().await.map(|audio| json!(audio)),
        Operation::SetVolume { percent } => state
            .0
            .system
            .set_volume_percent(percent)
            .await
            .map(|()| json!({})),
        Operation::ToggleOutputMute => state
            .0
            .system
            .toggle_output_mute()
            .await
            .map(|()| json!({})),
        Operation::SelectAudioOutput { id } => state
            .0
            .system
            .select_audio_output(id)
            .await
            .map(|()| json!({})),
        Operation::SelectAudioInput { id } => state
            .0
            .system
            .select_audio_input(id)
            .await
            .map(|()| json!({})),
        Operation::ToggleMicrophoneMute => state
            .0
            .system
            .toggle_microphone_mute()
            .await
            .map(|()| json!({})),
        Operation::MediaPlayPause => state.0.system.media_play_pause().await.map(|()| json!({})),
        Operation::MediaInfo => state.0.system.media_state().await.map(|media| json!(media)),
        Operation::MediaPlayPauseFor { player_id } => state
            .0
            .system
            .media_play_pause_for(&player_id)
            .await
            .map(|()| json!({})),
        Operation::MediaNext { player_id } => state
            .0
            .system
            .media_next(&player_id)
            .await
            .map(|()| json!({})),
        Operation::MediaPrevious { player_id } => state
            .0
            .system
            .media_previous(&player_id)
            .await
            .map(|()| json!({})),
    };
    match result {
        Ok(payload) => Json(ResponseEnvelope {
            version: PROTOCOL_VERSION,
            id: request.id,
            success: true,
            payload: Some(payload),
            error: None,
        })
        .into_response(),
        Err(message) => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "backend_unavailable",
            &message,
            &request.id,
        ),
    }
}

fn required_capability(operation: &Operation) -> Option<fn(&CompositorCapabilities) -> bool> {
    match operation {
        Operation::ListWorkspaces => Some(|capabilities| capabilities.workspaces),
        Operation::SwitchWorkspace { .. } => Some(|capabilities| capabilities.switch_workspace),
        Operation::ListWindows => Some(|capabilities| capabilities.windows),
        Operation::FocusWindow { .. } => Some(|capabilities| capabilities.focus_window),
        Operation::MoveWindow { .. } => Some(|capabilities| capabilities.move_window),
        Operation::ToggleFloating { .. } => Some(|capabilities| capabilities.toggle_floating),
        Operation::ToggleFullscreen { .. } => Some(|capabilities| capabilities.toggle_fullscreen),
        Operation::CloseWindow { .. } => Some(|capabilities| capabilities.close_window),
        Operation::ListMonitors => Some(|capabilities| capabilities.monitors),
        Operation::SetMonitorPower { .. } => Some(|capabilities| capabilities.monitor_power),
        Operation::LockScreen => Some(|capabilities| capabilities.lock_screen),
        _ => None,
    }
}

async fn desktop_snapshot(backend: &dyn CompositorBackend) -> Result<Value, String> {
    let sample_started = tokio::time::Instant::now();
    let uptime = std::fs::read_to_string("/proc/uptime")
        .map_err(|e| e.to_string())?
        .split_whitespace()
        .next()
        .unwrap_or("0")
        .parse::<f64>()
        .map_err(|e| e.to_string())?;
    let memory = std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|s| waycontrol_telemetry::parse_memory(&s).ok());
    let cpu_sample_a = std::fs::read_to_string("/proc/stat").ok();
    let cpu_a = cpu_sample_a
        .as_deref()
        .and_then(|s| waycontrol_telemetry::parse_cpu_times(s).ok());
    let cpu_cores_a = cpu_sample_a
        .as_deref()
        .and_then(|s| waycontrol_telemetry::parse_cpu_times_by_core(s).ok());
    let network_a = std::fs::read_to_string("/proc/net/dev")
        .ok()
        .and_then(|s| waycontrol_telemetry::parse_network_interfaces(&s).ok());
    tokio::time::sleep(Duration::from_millis(120)).await;
    let cpu_sample_b = std::fs::read_to_string("/proc/stat").ok();
    let cpu_b = cpu_sample_b
        .as_deref()
        .and_then(|s| waycontrol_telemetry::parse_cpu_times(s).ok());
    let cpu_cores_b = cpu_sample_b
        .as_deref()
        .and_then(|s| waycontrol_telemetry::parse_cpu_times_by_core(s).ok());
    let network_b = std::fs::read_to_string("/proc/net/dev")
        .ok()
        .and_then(|s| waycontrol_telemetry::parse_network_interfaces(&s).ok());
    let cpu = cpu_a
        .zip(cpu_b)
        .and_then(|(a, b)| waycontrol_telemetry::cpu_percent(a.total, b.total, a.idle, b.idle));
    let cpu_cores = cpu_cores_a
        .zip(cpu_cores_b)
        .map(|(previous, current)| waycontrol_telemetry::core_cpu_percentages(&previous, &current))
        .unwrap_or_default()
        .into_iter()
        .map(|(name, usage_percent)| {
            let frequency_khz = read_cpu_core_frequency_khz(&name);
            json!({"id":name,"usage_percent":usage_percent,"frequency_khz":frequency_khz})
        })
        .collect::<Vec<_>>();
    let cpu_core_temperatures = read_cpu_core_temperatures()
        .into_iter()
        .map(|temperature| {
            json!({
                "package_id":temperature.package_id,
                "core_id":temperature.core_id,
                "temperature_millidegrees":temperature.temperature_millidegrees
            })
        })
        .collect::<Vec<_>>();
    let storage = std::fs::read_to_string("/proc/self/mounts")
        .ok()
        .map(|input| waycontrol_telemetry::parse_storage_mounts(&input))
        .into_iter()
        .flatten()
        .take(64)
        .filter_map(storage_mount_snapshot)
        .collect::<Vec<_>>();
    let load_average = std::fs::read_to_string("/proc/loadavg")
        .ok()
        .and_then(|input| waycontrol_telemetry::parse_load_average(&input).ok())
        .map(|[one_minute, five_minutes, fifteen_minutes]| {
            json!({"one_minute":one_minute,"five_minutes":five_minutes,"fifteen_minutes":fifteen_minutes})
        });
    let elapsed = sample_started.elapsed().as_secs_f64();
    let network_rates = network_a
        .zip(network_b)
        .map(|(a, b)| waycontrol_telemetry::network_rates(&a, &b, elapsed))
        .unwrap_or_default()
        .into_iter()
        .map(|rate| {
            json!({"name":rate.name,"received_bytes_per_second":rate.received_bytes_per_second,"transmitted_bytes_per_second":rate.transmitted_bytes_per_second})
        })
        .collect::<Vec<_>>();
    let network_interfaces = read_network_interfaces();
    let cpu_frequency_khz = read_cpu_core_frequency_khz("cpu0");
    Ok(json!({
        "compositor":backend.info(),
        "capabilities":backend.capabilities(),
        "computer_name":std::fs::read_to_string("/etc/hostname").unwrap_or_default().trim(),
        "uptime_seconds":uptime,
        "cpu_usage_percent":cpu,
        "cpu_frequency_khz":cpu_frequency_khz,
        "cpu_cores":cpu_cores,
        "cpu_core_temperatures":cpu_core_temperatures,
        "storage":storage,
        "load_average":load_average,
        "memory":memory,
        "network_rates":network_rates,
        "network_interfaces":network_interfaces,
        "gpu":gpu_snapshot()
    }))
}

fn storage_mount_snapshot(mount: waycontrol_telemetry::StorageMount) -> Option<Value> {
    let path = CString::new(mount.mount_point.as_bytes()).ok()?;
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: `path` is NUL terminated and `stats` points to writable storage for statvfs.
    if unsafe { libc::statvfs(path.as_ptr(), stats.as_mut_ptr()) } != 0 {
        return None;
    }
    // SAFETY: a successful statvfs call initializes the output structure.
    let stats = unsafe { stats.assume_init() };
    let block_size = if stats.f_frsize > 0 {
        stats.f_frsize
    } else {
        stats.f_bsize
    };
    let total_bytes = stats.f_blocks.saturating_mul(block_size);
    if total_bytes == 0 {
        return None;
    }
    let free_bytes = stats.f_bfree.saturating_mul(block_size).min(total_bytes);
    let available_bytes = stats.f_bavail.saturating_mul(block_size).min(total_bytes);
    Some(json!({
        "source":mount.source,
        "mount_point":mount.mount_point,
        "filesystem_type":mount.filesystem_type,
        "total_bytes":total_bytes,
        "used_bytes":total_bytes.saturating_sub(free_bytes),
        "available_bytes":available_bytes
    }))
}

#[derive(Default)]
struct NetworkInterfaceSnapshot {
    is_up: bool,
    addresses: BTreeSet<String>,
}

struct IfAddrsGuard(*mut libc::ifaddrs);

impl Drop for IfAddrsGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: this pointer was allocated by `getifaddrs` and is released once.
            unsafe { libc::freeifaddrs(self.0) };
        }
    }
}

fn read_network_interfaces() -> Vec<Value> {
    let mut addresses = std::ptr::null_mut();
    // SAFETY: `getifaddrs` initializes the output pointer on success.
    if unsafe { libc::getifaddrs(&mut addresses) } != 0 {
        return Vec::new();
    }
    let _guard = IfAddrsGuard(addresses);
    let ipv4_routes = std::fs::read_to_string("/proc/net/route").unwrap_or_default();
    let ipv6_routes = std::fs::read_to_string("/proc/net/ipv6_route").unwrap_or_default();
    let default_routes = waycontrol_telemetry::default_route_interfaces(&ipv4_routes, &ipv6_routes);
    let mut interfaces = BTreeMap::<String, NetworkInterfaceSnapshot>::new();
    let mut current = addresses;

    while !current.is_null() {
        // SAFETY: each node remains valid until the guard calls `freeifaddrs`.
        let entry = unsafe { &*current };
        if !entry.ifa_name.is_null() {
            // SAFETY: `ifa_name` is a NUL-terminated interface name owned by the list.
            let name = unsafe { CStr::from_ptr(entry.ifa_name) }
                .to_string_lossy()
                .into_owned();
            if valid_network_interface_name(&name) {
                let interface = interfaces.entry(name).or_default();
                interface.is_up |= entry.ifa_flags & libc::IFF_UP as u32 != 0;
                if !entry.ifa_addr.is_null() {
                    // SAFETY: `ifa_addr` points to a sockaddr whose first field is its family.
                    let family = unsafe { (*entry.ifa_addr).sa_family };
                    let ip_address = match family as i32 {
                        libc::AF_INET => {
                            // SAFETY: AF_INET entries are `sockaddr_in` values.
                            let address = unsafe { &*(entry.ifa_addr as *const libc::sockaddr_in) };
                            Some(Ipv4Addr::from(address.sin_addr.s_addr.to_ne_bytes()).to_string())
                        }
                        libc::AF_INET6 => {
                            // SAFETY: AF_INET6 entries are `sockaddr_in6` values.
                            let address =
                                unsafe { &*(entry.ifa_addr as *const libc::sockaddr_in6) };
                            Some(Ipv6Addr::from(address.sin6_addr.s6_addr).to_string())
                        }
                        _ => None,
                    };
                    if let Some(ip_address) = ip_address
                        && interface.addresses.len() < 16
                    {
                        interface.addresses.insert(ip_address);
                    }
                }
            }
        }
        current = entry.ifa_next;
    }

    interfaces
        .into_iter()
        .take(64)
        .map(|(name, interface)| {
            let operational_state =
                std::fs::read_to_string(format!("/sys/class/net/{name}/operstate"))
                    .ok()
                    .map(|state| state.trim().to_owned())
                    .filter(|state| {
                        matches!(
                            state.as_str(),
                            "up" | "down" | "unknown" | "dormant" | "lowerlayerdown" | "notpresent"
                        )
                    });
            json!({
                "name":name,
                "is_up":interface.is_up,
                "operational_state":operational_state,
                "addresses":interface.addresses,
                "default_route":default_routes.contains(&name)
            })
        })
        .collect()
}

fn valid_network_interface_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && name.len() < 16
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.:".contains(&byte))
}

fn read_cpu_core_frequency_khz(core_id: &str) -> Option<u64> {
    let suffix = core_id.strip_prefix("cpu")?;
    if suffix.is_empty() || !suffix.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    [
        format!("/sys/devices/system/cpu/{core_id}/cpufreq/scaling_cur_freq"),
        format!("/sys/devices/system/cpu/{core_id}/cpufreq/cpuinfo_cur_freq"),
    ]
    .iter()
    .find_map(|path| {
        let contents = std::fs::read_to_string(path).ok()?;
        waycontrol_telemetry::parse_cpu_frequency_khz(&contents)
    })
}

fn read_cpu_core_temperatures() -> Vec<waycontrol_telemetry::CpuCoreTemperature> {
    let Ok(hwmon_devices) = std::fs::read_dir("/sys/class/hwmon") else {
        return Vec::new();
    };
    let mut temperatures = std::collections::BTreeMap::new();

    for device in hwmon_devices.flatten() {
        let directory = device.path();
        if std::fs::read_to_string(directory.join("name"))
            .map(|name| name.trim() != "coretemp")
            .unwrap_or(true)
        {
            continue;
        }

        let Ok(files) = std::fs::read_dir(&directory) else {
            continue;
        };
        let labels = files
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().into_string().ok()?;
                let index = name.strip_prefix("temp")?.strip_suffix("_label")?;
                if index.is_empty() || !index.bytes().all(|byte| byte.is_ascii_digit()) {
                    return None;
                }
                let label = std::fs::read_to_string(entry.path()).ok()?;
                Some((name, label.trim().to_owned()))
            })
            .collect::<Vec<_>>();
        let package_label = labels
            .iter()
            .map(|(_, label)| label.as_str())
            .find(|label| label.starts_with("Package id "));

        for (label_file, label) in &labels {
            if !label.starts_with("Core ") {
                continue;
            }
            let Some(stem) = label_file.strip_suffix("_label") else {
                continue;
            };
            let Ok(input) = std::fs::read_to_string(directory.join(format!("{stem}_input"))) else {
                continue;
            };
            let Some(temperature) =
                waycontrol_telemetry::parse_coretemp_temperature(package_label, label, &input)
            else {
                continue;
            };
            temperatures
                .entry((temperature.package_id, temperature.core_id))
                .or_insert(temperature);
        }
    }

    temperatures.into_values().collect()
}

fn gpu_snapshot() -> Value {
    static NVML: std::sync::OnceLock<Result<Nvml, String>> = std::sync::OnceLock::new();
    let nvml = match NVML.get_or_init(|| Nvml::init().map_err(|error| error.to_string())) {
        Ok(nvml) => nvml,
        Err(error) => return json!({"available":false,"reason":error}),
    };
    let device = match nvml.device_by_index(0) {
        Ok(device) => device,
        Err(error) => return json!({"available":false,"reason":error.to_string()}),
    };
    let utilization = device.utilization_rates().ok();
    let memory = device.memory_info().ok();
    json!({
        "available":true,
        "name":device.name().ok(),
        "utilization_percent":utilization.as_ref().map(|value| value.gpu),
        "memory_utilization_percent":utilization.as_ref().map(|value| value.memory),
        "temperature_celsius":device.temperature(TemperatureSensor::Gpu).ok(),
        "vram_used_bytes":memory.as_ref().map(|value| value.used),
        "vram_total_bytes":memory.as_ref().map(|value| value.total),
        "power_watts":device.power_usage().ok().map(|milliwatts| f64::from(milliwatts) / 1000.0)
    })
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
}

fn api_error(status: StatusCode, code: &str, message: &str, id: &str) -> axum::response::Response {
    (
        status,
        Json(ResponseEnvelope {
            version: PROTOCOL_VERSION,
            id: id.to_owned(),
            success: false,
            payload: None,
            error: Some(ResponseError {
                code: code.into(),
                message: message.into(),
            }),
        }),
    )
        .into_response()
}

fn pairing_error(error: PairingError) -> axum::response::Response {
    let status = match error {
        PairingError::RateLimited => StatusCode::TOO_MANY_REQUESTS,
        PairingError::UnknownRequest => StatusCode::NOT_FOUND,
        _ => StatusCode::UNAUTHORIZED,
    };
    (
        status,
        Json(json!({"error":{"code":"pairing_rejected","message":error.to_string()}})),
    )
        .into_response()
}

fn fresh_code() -> Result<String, PairingError> {
    let mut bytes = [0_u8; 4];
    getrandom::fill(&mut bytes).map_err(|_| PairingError::Random)?;
    Ok(format!("{:06}", u32::from_be_bytes(bytes) % 1_000_000))
}

fn random_hex(bytes: usize) -> Result<String, PairingError> {
    let mut buffer = vec![0_u8; bytes];
    getrandom::fill(&mut buffer).map_err(|_| PairingError::Random)?;
    Ok(hex::encode(buffer))
}

fn token_hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    let mut difference = a.len() ^ b.len();
    let size = a.len().max(b.len());
    for i in 0..size {
        difference |= usize::from(a.get(i).copied().unwrap_or(0) ^ b.get(i).copied().unwrap_or(0));
    }
    difference == 0
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn socket_address_from_env() -> Result<SocketAddr, Box<dyn std::error::Error>> {
    Ok(bind_address_from_env_values(
        std::env::var("WAYCONTROL_BIND_ADDR").ok().as_deref(),
        std::env::var("HYPRCONTROL_BIND_ADDR").ok().as_deref(),
    )?)
}

fn bind_address_from_env_values(
    waycontrol: Option<&str>,
    legacy_hyprcontrol: Option<&str>,
) -> Result<SocketAddr, std::net::AddrParseError> {
    waycontrol
        .or(legacy_hyprcontrol)
        .unwrap_or("127.0.0.1:8765")
        .parse()
}

pub fn certificate_names(bind: SocketAddr) -> Vec<String> {
    let mut names = vec![
        "localhost".to_owned(),
        "127.0.0.1".to_owned(),
        "::1".to_owned(),
    ];
    if !bind.ip().is_unspecified() {
        names.push(bind.ip().to_string());
    }
    names
}

pub fn ensure_server_certificate(
    data_dir: impl AsRef<FsPath>,
    bind: SocketAddr,
) -> Result<(PathBuf, PathBuf, String), Box<dyn std::error::Error>> {
    let data_dir = data_dir.as_ref();
    std::fs::create_dir_all(data_dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(data_dir, std::fs::Permissions::from_mode(0o700))?;
    }
    let cert_path = data_dir.join("server-cert.pem");
    let key_path = data_dir.join("server-key.pem");
    if !cert_path.exists() && !key_path.exists() {
        let CertifiedKey { cert, signing_key } =
            generate_simple_self_signed(certificate_names(bind))?;
        write_private_file(&cert_path, cert.pem().as_bytes())?;
        write_private_file(&key_path, signing_key.serialize_pem().as_bytes())?;
    } else if !cert_path.exists() || !key_path.exists() {
        return Err("incomplete TLS certificate/key pair; remove both files to regenerate".into());
    }
    let pem = std::fs::read(&cert_path)?;
    let der = CertificateDer::from_pem_slice(&pem)?;
    let fingerprint = hex::encode(Sha256::digest(der.as_ref()));
    Ok((cert_path, key_path, fingerprint))
}

fn write_private_file(path: &FsPath, data: &[u8]) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::{io::Write, os::unix::fs::OpenOptionsExt};
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(data)?;
        file.sync_all()
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, data)
    }
}

pub async fn serve_tls(
    state: AppState,
    bind: SocketAddr,
    cert_path: &FsPath,
    key_path: &FsPath,
) -> Result<(), Box<dyn std::error::Error>> {
    let tls = axum_server::tls_rustls::RustlsConfig::from_pem_file(cert_path, key_path).await?;
    axum_server::bind_rustls(bind, tls)
        .serve(router(state).into_make_service())
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playerctl_mpris_discovery_and_metadata_are_bounded_and_sanitized() {
        let players = parse_playerctl_list(
            "org.mpris.MediaPlayer2.spotify\norg.mpris.MediaPlayer2.vlc\norg.mpris.MediaPlayer2.spotify\nplayer;reboot\n\n",
        );
        assert_eq!(
            players,
            [
                "org.mpris.MediaPlayer2.spotify",
                "org.mpris.MediaPlayer2.vlc"
            ]
        );
        let (title, artist, album) =
            parse_playerctl_metadata("Track\u{7} one\tArtist\nName\tAlbum\n");
        assert_eq!(title, "Track one");
        assert_eq!(artist, "ArtistName");
        assert_eq!(album, "Album");
        assert_eq!(playerctl_display_name(&players[0]), "spotify");
        assert!(!valid_playerctl_id("player;reboot"));
        assert!(!valid_playerctl_id(&"x".repeat(257)));
        let many = (0..32)
            .map(|index| format!("org.mpris.MediaPlayer2.player{index}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(parse_playerctl_list(&many).len(), MAX_MPRIS_PLAYERS);
    }

    #[test]
    fn wpctl_audio_output_parsing_reports_linear_percent_and_mute_state() {
        assert_eq!(
            parse_wpctl_audio_state("Volume: 0.42 [MUTED]\n").unwrap(),
            AudioState {
                output_volume_percent: 42,
                output_muted: true,
                microphone_muted: None,
                output_devices: Vec::new(),
                input_devices: Vec::new(),
            }
        );
        assert_eq!(
            parse_wpctl_audio_state("Volume: 1.25\n").unwrap(),
            AudioState {
                output_volume_percent: 100,
                output_muted: false,
                microphone_muted: None,
                output_devices: Vec::new(),
                input_devices: Vec::new(),
            }
        );
        assert!(parse_wpctl_audio_state("not wpctl output").is_err());
    }

    #[test]
    fn wpctl_microphone_state_parser_detects_muted_sources() {
        assert_eq!(parse_wpctl_mute_state("Volume: 0.73 [MUTED]\n"), Ok(true));
        assert_eq!(parse_wpctl_mute_state("Volume: 0.73\n"), Ok(false));
        assert!(parse_wpctl_mute_state("unexpected").is_err());
    }

    #[test]
    fn wpctl_status_parser_separates_active_audio_sinks_and_sources() {
        let output = "PipeWire 'pipewire-0'\nAudio\n ├─ Sinks:\n │  *   53. alsa_output.pci-1.analog-stereo [vol: 0.98]\n │      54. alsa_output.usb-headset [vol: 0.50]\n ├─ Sources:\n │  *   63. alsa_input.usb-mic [vol: 0.47]\n │      82. alsa_input.pci-mic [vol: 1.00]\nVideo\n ├─ Sources:\n │  *   57. camera [v4l2]\n";
        let (outputs, inputs) = parse_wpctl_audio_devices(output).unwrap();
        assert_eq!(outputs.len(), 2);
        assert_eq!(outputs[0].id, 53);
        assert_eq!(outputs[0].name, "alsa_output.pci-1.analog-stereo");
        assert!(outputs[0].active);
        assert!(!outputs[1].active);
        assert_eq!(inputs.len(), 2);
        assert_eq!(inputs[0].id, 63);
        assert!(inputs[0].active);
        assert!(audio_device_is_selectable(54, true, &outputs, &inputs));
        assert!(!audio_device_is_selectable(63, true, &outputs, &inputs));
        assert!(audio_device_is_selectable(82, false, &outputs, &inputs));
        assert!(!audio_device_is_selectable(54, false, &outputs, &inputs));
    }
    use axum::{
        body::{Body, to_bytes},
        http::{Request, header},
    };
    use std::sync::atomic::{AtomicI64, Ordering};
    use tokio::io::{AsyncWriteExt, BufReader};
    use tower::ServiceExt;

    #[tokio::test]
    async fn hyprland_event_lines_are_bounded_before_allocation() {
        let mut accepted = BufReader::new(&b"workspace>>3\n"[..]);
        let mut partial = Vec::new();
        assert_eq!(
            read_bounded_line(&mut accepted, &mut partial, 32)
                .await
                .unwrap(),
            Some(b"workspace>>3\n".to_vec())
        );

        let mut oversized = BufReader::new(&b"0123456789abcdef\n"[..]);
        assert_eq!(
            read_bounded_line(&mut oversized, &mut Vec::new(), 8)
                .await
                .unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn bounded_event_reader_preserves_partial_lines_across_heartbeat_checks() {
        use tokio::net::UnixStream;

        let (stream, mut peer) = UnixStream::pair().unwrap();
        let mut reader = BufReader::new(stream);
        let mut partial = Vec::new();
        peer.write_all(b"workspace>>").await.unwrap();
        assert!(
            tokio::time::timeout(
                Duration::from_millis(20),
                read_bounded_line(&mut reader, &mut partial, MAX_EVENT_BYTES)
            )
            .await
            .is_err()
        );
        assert_eq!(partial, b"workspace>>");
        peer.write_all(b"3\n").await.unwrap();
        assert_eq!(
            read_bounded_line(&mut reader, &mut partial, MAX_EVENT_BYTES)
                .await
                .unwrap(),
            Some(b"workspace>>3\n".to_vec())
        );
    }

    #[test]
    fn per_device_api_rate_limit_caps_requests_and_resets_after_one_second() {
        let started = tokio::time::Instant::now();
        let mut window = RequestRateWindow { started, count: 0 };
        for _ in 0..API_REQUESTS_PER_DEVICE_PER_SECOND {
            assert!(window.allow_at(started));
        }
        assert!(!window.allow_at(started));
        assert!(window.allow_at(started + Duration::from_secs(1)));
    }

    #[test]
    fn bind_address_prefers_waycontrol_and_falls_back_to_historical_name() {
        assert_eq!(
            bind_address_from_env_values(Some("127.0.0.1:9999"), Some("127.0.0.1:8888")).unwrap(),
            "127.0.0.1:9999".parse::<SocketAddr>().unwrap()
        );
        assert_eq!(
            bind_address_from_env_values(None, Some("127.0.0.1:8888")).unwrap(),
            "127.0.0.1:8888".parse::<SocketAddr>().unwrap()
        );
    }

    #[test]
    fn lan_discovery_is_only_advertised_for_lan_reachable_https_binds() {
        assert_eq!(
            lan_discovery_bind_address("127.0.0.1:8765".parse().unwrap()),
            None
        );
        assert_eq!(
            lan_discovery_bind_address("192.168.1.20:8765".parse().unwrap()),
            Some("192.168.1.20:8766".parse().unwrap())
        );
        assert_eq!(
            lan_discovery_bind_address("0.0.0.0:8765".parse().unwrap()),
            None
        );
        assert_eq!(
            lan_discovery_bind_address("203.0.113.10:8765".parse().unwrap()),
            None
        );
    }

    #[test]
    fn lan_discovery_accepts_only_bounded_probe_from_a_local_address() {
        assert!(discovery_probe_is_valid(LAN_DISCOVERY_PROBE));
        assert!(!discovery_probe_is_valid(b"WAYCONTROL_DISCOVER_V1 extra"));
        assert!(!discovery_probe_is_valid(&[b'x'; 512]));
        assert!(discovery_source_is_local(
            "192.168.1.12:4567".parse().unwrap()
        ));
        assert!(discovery_source_is_local("10.20.0.9:4567".parse().unwrap()));
        assert!(discovery_source_is_local(
            "169.254.8.2:4567".parse().unwrap()
        ));
        assert!(!discovery_source_is_local("8.8.8.8:4567".parse().unwrap()));
    }

    #[test]
    fn discovery_announcement_sanitizes_machine_name_and_contains_no_trust_material() {
        let announcement = lan_discovery_announcement("living-room\nserver", 8765);
        let value = serde_json::to_value(announcement).unwrap();
        assert_eq!(value["version"], 1);
        assert_eq!(value["name"], "living-roomserver");
        assert_eq!(value["https_port"], 8765);
        assert!(value.get("fingerprint").is_none());
    }

    #[tokio::test]
    async fn lan_discovery_udp_listener_answers_valid_local_probes() {
        let server = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let server_address = server.local_addr().unwrap();
        let api_bind: SocketAddr = "127.0.0.1:8765".parse().unwrap();
        let listener = tokio::spawn(run_lan_discovery(server, api_bind, "test desktop"));
        let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        client
            .send_to(LAN_DISCOVERY_PROBE, server_address)
            .await
            .unwrap();
        let mut response = [0_u8; 256];
        let (length, _) =
            tokio::time::timeout(Duration::from_secs(1), client.recv_from(&mut response))
                .await
                .unwrap()
                .unwrap();
        let response: LanDiscoveryAnnouncement =
            serde_json::from_slice(&response[..length]).unwrap();
        assert_eq!(response.name, "test desktop");
        assert_eq!(response.https_port, 8765);
        listener.abort();
    }

    #[test]
    fn pairing_device_names_reject_terminal_and_unicode_direction_controls() {
        assert!(valid_device_name("Android device"));
        assert!(!valid_device_name("phone\u{1b}[31m"));
        assert!(!valid_device_name("trusted\u{202e}device"));
    }

    #[tokio::test]
    async fn detected_sway_uses_its_adapter_and_never_falls_back_to_hyprland() {
        let backend = WaylandBackend::for_kind(CompositorKind::Sway);
        assert_eq!(backend.info().id, "sway");
        assert_eq!(backend.info().name, "Sway");
        assert!(backend.capabilities().workspaces);
        assert!(backend.capabilities().windows);
        assert!(!backend.capabilities().monitor_power);
        assert!(!backend.capabilities().lock_screen);
        assert!(
            backend
                .workspaces()
                .await
                .unwrap_err()
                .contains("SWAYSOCK is unavailable")
        );
    }

    #[tokio::test]
    async fn detected_niri_uses_its_adapter_and_only_advertises_supported_features() {
        let backend = WaylandBackend::for_kind(CompositorKind::Niri);
        assert_eq!(backend.info().id, "niri");
        assert!(backend.capabilities().workspaces);
        assert!(backend.capabilities().windows);
        assert!(backend.capabilities().events);
        assert!(!backend.capabilities().toggle_fullscreen);
        assert!(!backend.capabilities().monitor_power);
        assert!(
            backend
                .workspaces()
                .await
                .unwrap_err()
                .contains("NIRI_SOCKET is unavailable")
        );
    }

    #[test]
    fn kwin_exposes_only_workspace_operations_and_gnome_remains_unsupported() {
        let kwin = WaylandBackend::for_kind(CompositorKind::KdeKWin);
        assert_eq!(kwin.info().id, "kde_kwin");
        assert!(kwin.capabilities().workspaces);
        assert!(kwin.capabilities().switch_workspace);
        assert!(kwin.capabilities().events);
        assert!(!kwin.capabilities().windows);
        assert!(!kwin.capabilities().monitor_power);

        let gnome = WaylandBackend::for_kind(CompositorKind::Gnome);
        assert_eq!(gnome.info().id, "gnome");
        assert_eq!(gnome.capabilities(), CompositorCapabilities::none());
    }

    #[test]
    fn hyprland_socket_discovery_fills_missing_session_desktop_metadata() {
        assert_eq!(
            WaylandBackend::for_detected_kind(CompositorKind::Unknown, true)
                .info()
                .id,
            "hyprland"
        );
        assert_eq!(
            WaylandBackend::for_detected_kind(CompositorKind::Sway, true)
                .info()
                .id,
            "sway"
        );
    }

    struct MockBackend {
        switches: AtomicI64,
        capabilities: CompositorCapabilities,
        power_calls: Arc<Mutex<Vec<PowerAction>>>,
        event_peers: Arc<Mutex<Vec<UnixStream>>>,
    }
    #[async_trait]
    impl CompositorBackend for MockBackend {
        fn info(&self) -> CompositorInfo {
            CompositorInfo::detected(CompositorKind::Hyprland)
        }

        fn capabilities(&self) -> CompositorCapabilities {
            self.capabilities
        }

        async fn workspaces(&self) -> Result<Vec<Workspace>, String> {
            Ok(vec![Workspace {
                id: 3.into(),
                name: "3".into(),
                monitor: Some("DP-1".into()),
                window_count: 0,
                active: true,
            }])
        }
        async fn windows(&self) -> Result<Vec<Window>, String> {
            Ok(vec![])
        }
        async fn switch_workspace(&self, id: &waycontrol_core::WorkspaceId) -> Result<(), String> {
            self.switches
                .store(id.as_i64().unwrap_or_default(), Ordering::SeqCst);
            Ok(())
        }
        async fn focus_window(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
        async fn move_window(
            &self,
            _: &str,
            _: &waycontrol_core::WorkspaceId,
        ) -> Result<(), String> {
            Ok(())
        }
        async fn toggle_floating(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
        async fn toggle_fullscreen(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
        async fn close_window(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
        async fn monitors(&self) -> Result<Vec<Monitor>, String> {
            Ok(vec![Monitor {
                name: "DP-1".into(),
                dpms_enabled: Some(true),
            }])
        }
        async fn set_monitor_power(&self, _: &str, _: bool) -> Result<(), String> {
            Ok(())
        }
        async fn lock_screen(&self) -> Result<(), String> {
            Ok(())
        }
        async fn event_stream(&self) -> Result<UnixStream, String> {
            let (stream, peer) = UnixStream::pair().map_err(|error| error.to_string())?;
            self.event_peers.lock().await.push(peer);
            Ok(stream)
        }
    }

    #[async_trait]
    impl SystemControlBackend for MockBackend {
        async fn system_power(&self, action: PowerAction) -> Result<(), String> {
            self.power_calls.lock().await.push(action);
            Ok(())
        }
        async fn adjust_volume(&self, _: VolumeDirection) -> Result<(), String> {
            Ok(())
        }
        async fn audio_state(&self) -> Result<AudioState, String> {
            Ok(AudioState {
                output_volume_percent: 40,
                output_muted: false,
                microphone_muted: Some(false),
                output_devices: Vec::new(),
                input_devices: Vec::new(),
            })
        }
        async fn set_volume_percent(&self, _: u8) -> Result<(), String> {
            Ok(())
        }
        async fn toggle_output_mute(&self) -> Result<(), String> {
            Ok(())
        }
        async fn select_audio_output(&self, _: u32) -> Result<(), String> {
            Ok(())
        }
        async fn select_audio_input(&self, _: u32) -> Result<(), String> {
            Ok(())
        }
        async fn toggle_microphone_mute(&self) -> Result<(), String> {
            Ok(())
        }
        async fn media_play_pause(&self) -> Result<(), String> {
            Ok(())
        }
        async fn media_state(&self) -> Result<MediaState, String> {
            Ok(MediaState {
                players: Vec::new(),
            })
        }
        async fn media_play_pause_for(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
        async fn media_next(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
        async fn media_previous(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
    }

    async fn test_state_with_recorders() -> (
        AppState,
        PathBuf,
        Arc<Mutex<Vec<PowerAction>>>,
        Arc<Mutex<Vec<UnixStream>>>,
    ) {
        test_state_with_capabilities(CompositorCapabilities::HYPRLAND).await
    }

    async fn test_state_with_capabilities(
        capabilities: CompositorCapabilities,
    ) -> (
        AppState,
        PathBuf,
        Arc<Mutex<Vec<PowerAction>>>,
        Arc<Mutex<Vec<UnixStream>>>,
    ) {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("waycontrol-test-{}-{nonce}", std::process::id()));
        let power_calls = Arc::new(Mutex::new(Vec::new()));
        let event_peers = Arc::new(Mutex::new(Vec::new()));
        let backend = Arc::new(MockBackend {
            switches: AtomicI64::new(0),
            capabilities,
            power_calls: power_calls.clone(),
            event_peers: event_peers.clone(),
        });
        let state = AppState::new(backend.clone(), backend, &path)
            .await
            .unwrap();
        (state, path, power_calls, event_peers)
    }

    async fn test_state() -> (AppState, PathBuf) {
        let (state, path, _, _) = test_state_with_recorders().await;
        (state, path)
    }

    async fn send(app: Router, token: Option<&str>, body: &str) -> axum::response::Response {
        let mut builder =
            Request::post("/api/v1/request").header(header::CONTENT_TYPE, "application/json");
        if let Some(token) = token {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        app.oneshot(builder.body(Body::from(body.to_owned())).unwrap())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn untrusted_devices_cannot_read_or_control_desktop() {
        let (state, path) = test_state().await;
        let app = router(state);
        let response = send(
            app.clone(),
            None,
            r#"{"version":1,"id":"1","type":"workspace.switch","payload":{"workspace_id":3}}"#,
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let response = send(
            app,
            None,
            r#"{"version":1,"id":"power","type":"system.power","payload":{"action":"shutdown"}}"#,
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let _ = std::fs::remove_dir_all(path);
    }

    #[tokio::test]
    async fn authenticated_audio_controls_work_without_compositor_capabilities() {
        let (state, path, _, _) =
            test_state_with_capabilities(CompositorCapabilities::none()).await;
        let pairing = state.pairing_info().await.unwrap();
        let request_id = state
            .begin_pairing(&pairing.code, "audio-test")
            .await
            .unwrap();
        state.approve_pairing(&request_id).await.unwrap();
        let (_, token) = state.pairing_status(&request_id).await.unwrap().unwrap();
        let app = router(state);

        let response = send(
            app.clone(),
            Some(&token),
            r#"{"version":1,"id":"audio","type":"audio.info"}"#,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let value: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["payload"]["output_volume_percent"], 40);
        assert_eq!(value["payload"]["output_muted"], false);
        assert_eq!(value["payload"]["microphone_muted"], false);

        let response = send(
            app.clone(),
            Some(&token),
            r#"{"version":1,"id":"volume","type":"audio.volume.set","payload":{"percent":65}}"#,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);

        for (id, operation) in [(53, "audio.output.select"), (82, "audio.input.select")] {
            let body = format!(
                r#"{{"version":1,"id":"select-{id}","type":"{operation}","payload":{{"id":{id}}}}}"#
            );
            let response = send(app.clone(), Some(&token), &body).await;
            assert_eq!(response.status(), StatusCode::OK);
        }

        let response = send(
            app.clone(),
            Some(&token),
            r#"{"version":1,"id":"output-mute","type":"audio.output_mute"}"#,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);

        let response = send(
            app,
            Some(&token),
            r#"{"version":1,"id":"bad-volume","type":"audio.volume.set","payload":{"percent":101}}"#,
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let _ = std::fs::remove_dir_all(path);
    }

    #[tokio::test]
    async fn pairing_requires_code_and_linux_approval_before_token_works() {
        let (state, path) = test_state().await;
        let info = state.pairing_info().await.unwrap();
        let request_id = state.begin_pairing(&info.code, "Poco M5").await.unwrap();
        assert!(state.pairing_status(&request_id).await.unwrap().is_none());
        state.approve_pairing(&request_id).await.unwrap();
        let (_, token) = state.pairing_status(&request_id).await.unwrap().unwrap();
        assert!(state.authenticated_device(&token).await.is_some());
        let _ = std::fs::remove_dir_all(path);
    }

    #[tokio::test]
    async fn approved_pairing_credentials_are_marked_non_cacheable() {
        let (state, path) = test_state().await;
        let info = state.pairing_info().await.unwrap();
        let request_id = state.begin_pairing(&info.code, "phone").await.unwrap();
        state.approve_pairing(&request_id).await.unwrap();
        let response = router(state)
            .oneshot(
                Request::get(format!("/api/v1/pair/{request_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        let body = to_bytes(response.into_body(), MAX_API_BODY_BYTES)
            .await
            .unwrap();
        let status: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(status["status"], "approved");
        assert!(
            status["token"]
                .as_str()
                .is_some_and(|token| !token.is_empty())
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[tokio::test]
    async fn expired_codes_and_revoked_devices_are_rejected() {
        let (state, path) = test_state().await;
        {
            let mut p = state.0.pairing.lock().await;
            p.code_expires = tokio::time::Instant::now() - Duration::from_secs(1);
        }
        assert_eq!(
            state.begin_pairing("000000", "phone").await.unwrap_err(),
            PairingError::InvalidCode
        );
        let info = state.pairing_info().await.unwrap();
        let rid = state.begin_pairing(&info.code, "phone").await.unwrap();
        state.approve_pairing(&rid).await.unwrap();
        let (_, token) = state.pairing_status(&rid).await.unwrap().unwrap();
        let device = state.devices().await.remove(0);
        state.revoke_device(&device.device_id).await.unwrap();
        assert!(state.authenticated_device(&token).await.is_none());
        let _ = std::fs::remove_dir_all(path);
    }

    #[tokio::test]
    async fn authenticated_request_returns_real_typed_workspace_payload() {
        let (state, path) = test_state().await;
        let info = state.pairing_info().await.unwrap();
        let rid = state.begin_pairing(&info.code, "phone").await.unwrap();
        state.approve_pairing(&rid).await.unwrap();
        let (_, token) = state.pairing_status(&rid).await.unwrap().unwrap();
        let app = router(state);
        let response = send(
            app.clone(),
            Some(&token),
            r#"{"version":1,"id":"ws-1","type":"workspace.list"}"#,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), MAX_API_BODY_BYTES)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["payload"][0]["id"], 3);
        assert_eq!(json["id"], "ws-1");
        let _ = std::fs::remove_dir_all(path);
    }

    #[tokio::test]
    async fn desktop_info_negotiates_compositor_identity_and_capabilities() {
        let (state, path) = test_state().await;
        let info = state.pairing_info().await.unwrap();
        let rid = state.begin_pairing(&info.code, "phone").await.unwrap();
        state.approve_pairing(&rid).await.unwrap();
        let (_, token) = state.pairing_status(&rid).await.unwrap().unwrap();
        let app = router(state);
        let response = send(
            app.clone(),
            Some(&token),
            r#"{"version":1,"id":"desktop","type":"desktop.info"}"#,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), MAX_API_BODY_BYTES)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["payload"]["compositor"]["id"], "hyprland");
        assert_eq!(json["payload"]["capabilities"]["switch_workspace"], true);
        assert_eq!(json["payload"]["capabilities"]["events"], true);
        let _ = std::fs::remove_dir_all(path);
    }

    #[tokio::test]
    async fn unsupported_compositor_operations_are_rejected_before_dispatch() {
        let (state, path, _, _) =
            test_state_with_capabilities(CompositorCapabilities::none()).await;
        let info = state.pairing_info().await.unwrap();
        let rid = state.begin_pairing(&info.code, "phone").await.unwrap();
        state.approve_pairing(&rid).await.unwrap();
        let (_, token) = state.pairing_status(&rid).await.unwrap().unwrap();
        let app = router(state);
        let response = send(
            app.clone(),
            Some(&token),
            r#"{"version":1,"id":"ws","type":"workspace.list"}"#,
        )
        .await;
        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
        let bytes = to_bytes(response.into_body(), MAX_API_BODY_BYTES)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["error"]["code"], "unsupported_operation");
        let lock = send(
            app,
            Some(&token),
            r#"{"version":1,"id":"lock","type":"system.lock"}"#,
        )
        .await;
        assert_eq!(lock.status(), StatusCode::NOT_IMPLEMENTED);
        let _ = std::fs::remove_dir_all(path);
    }

    #[tokio::test]
    async fn host_system_controls_do_not_depend_on_compositor_capabilities() {
        let (state, path, power_calls, _) =
            test_state_with_capabilities(CompositorCapabilities::none()).await;
        let info = state.pairing_info().await.unwrap();
        let rid = state.begin_pairing(&info.code, "phone").await.unwrap();
        state.approve_pairing(&rid).await.unwrap();
        let (_, token) = state.pairing_status(&rid).await.unwrap().unwrap();
        let response = send(
            router(state),
            Some(&token),
            r#"{"version":1,"id":"power","type":"system.power","payload":{"action":"suspend"}}"#,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(*power_calls.lock().await, vec![PowerAction::Suspend]);
        let _ = std::fs::remove_dir_all(path);
    }

    #[tokio::test]
    async fn event_stream_requires_a_trusted_device_and_returns_sse() {
        let (state, path) = test_state().await;
        let unauthenticated = router(state.clone())
            .oneshot(Request::get("/api/v1/events").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);

        let info = state.pairing_info().await.unwrap();
        let request_id = state.begin_pairing(&info.code, "phone").await.unwrap();
        state.approve_pairing(&request_id).await.unwrap();
        let (_, token) = state.pairing_status(&request_id).await.unwrap().unwrap();
        let response = router(state)
            .oneshot(
                Request::get("/api/v1/events")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "text/event-stream"
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[tokio::test]
    async fn revoking_device_closes_its_idle_event_stream() {
        use futures_util::StreamExt;

        let (state, path, _, event_peers) = test_state_with_recorders().await;
        let info = state.pairing_info().await.unwrap();
        let request_id = state.begin_pairing(&info.code, "phone").await.unwrap();
        state.approve_pairing(&request_id).await.unwrap();
        let (_, token) = state.pairing_status(&request_id).await.unwrap().unwrap();
        let device_id = state.devices().await.remove(0).device_id;

        let response = router(state.clone())
            .oneshot(
                Request::get("/api/v1/events")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(event_peers.lock().await.len(), 1);

        state.revoke_device(&device_id).await.unwrap();
        let mut body = response.into_body().into_data_stream();
        let next =
            tokio::time::timeout(EVENT_REAUTH_INTERVAL + Duration::from_secs(1), body.next())
                .await
                .expect("revoked stream was not rechecked");
        assert!(
            next.is_none(),
            "revoked device received an event or heartbeat"
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[tokio::test]
    async fn monitor_list_and_power_actions_are_behind_authentication() {
        let (state, path, power_calls, _) = test_state_with_recorders().await;
        let unauthenticated = send(
            router(state.clone()),
            None,
            r#"{"version":1,"id":"power","type":"system.power","payload":{"action":"reboot"}}"#,
        )
        .await;
        assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);
        assert!(power_calls.lock().await.is_empty());

        let info = state.pairing_info().await.unwrap();
        let request_id = state.begin_pairing(&info.code, "phone").await.unwrap();
        state.approve_pairing(&request_id).await.unwrap();
        let (_, token) = state.pairing_status(&request_id).await.unwrap().unwrap();
        let authorized_power = send(
            router(state.clone()),
            Some(&token),
            r#"{"version":1,"id":"power-authorized","type":"system.power","payload":{"action":"suspend"}}"#,
        )
        .await;
        assert_eq!(authorized_power.status(), StatusCode::OK);
        assert_eq!(*power_calls.lock().await, vec![PowerAction::Suspend]);
        let monitor_response = send(
            router(state),
            Some(&token),
            r#"{"version":1,"id":"monitor-list","type":"monitor.list"}"#,
        )
        .await;
        assert_eq!(monitor_response.status(), StatusCode::OK);
        let bytes = to_bytes(monitor_response.into_body(), MAX_API_BODY_BYTES)
            .await
            .unwrap();
        let payload: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(payload["payload"][0]["name"], "DP-1");
        let _ = std::fs::remove_dir_all(path);
    }

    #[tokio::test]
    async fn authenticated_api_route_enforces_per_device_rate_limit() {
        let (state, path) = test_state().await;
        let info = state.pairing_info().await.unwrap();
        let request_id = state.begin_pairing(&info.code, "phone").await.unwrap();
        state.approve_pairing(&request_id).await.unwrap();
        let (_, token) = state.pairing_status(&request_id).await.unwrap().unwrap();
        let device_id = state.devices().await.remove(0).device_id;
        state.0.api_rate_windows.lock().await.insert(
            device_id,
            RequestRateWindow {
                started: tokio::time::Instant::now(),
                count: API_REQUESTS_PER_DEVICE_PER_SECOND,
            },
        );
        let response = send(
            router(state),
            Some(&token),
            r#"{"version":1,"id":"rate-limit","type":"ping"}"#,
        )
        .await;
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let _ = std::fs::remove_dir_all(path);
    }
}
