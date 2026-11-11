//! Versioned, typed request models. No arbitrary compositor command is represented.

use serde::{Deserialize, Serialize};
use thiserror::Error;
use waycontrol_core::WorkspaceId;

pub const PROTOCOL_VERSION: u16 = 1;
pub const MAX_REQUEST_ID_BYTES: usize = 64;
pub const MAX_WINDOW_ID_BYTES: usize = 128;
pub const MAX_WORKSPACE_ID_BYTES: usize = 128;
pub const MAX_MEDIA_PLAYER_ID_BYTES: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum Operation {
    #[serde(rename = "ping")]
    Ping,
    #[serde(rename = "desktop.info")]
    DesktopInfo,
    #[serde(rename = "workspace.list")]
    ListWorkspaces,
    #[serde(rename = "workspace.switch")]
    SwitchWorkspace { workspace_id: WorkspaceId },
    #[serde(rename = "app.launch")]
    LaunchApplication { desktop_id: String },
    #[serde(rename = "window.list")]
    ListWindows,
    #[serde(rename = "window.focus")]
    FocusWindow { address: String },
    #[serde(rename = "window.move")]
    MoveWindow {
        address: String,
        workspace_id: WorkspaceId,
    },
    #[serde(rename = "window.move_monitor")]
    MoveWindowToMonitor { address: String, monitor: String },
    #[serde(rename = "window.toggle_floating")]
    ToggleFloating { address: String },
    #[serde(rename = "window.toggle_fullscreen")]
    ToggleFullscreen { address: String },
    #[serde(rename = "window.toggle_pinned")]
    TogglePinned { address: String },
    #[serde(rename = "window.close")]
    CloseWindow { address: String },
    #[serde(rename = "monitor.list")]
    ListMonitors,
    #[serde(rename = "monitor.power")]
    SetMonitorPower { monitor: String, enabled: bool },
    #[serde(rename = "system.lock")]
    LockScreen,
    #[serde(rename = "system.power")]
    SystemPower { action: PowerAction },
    #[serde(rename = "audio.volume")]
    AdjustVolume { direction: VolumeDirection },
    #[serde(rename = "audio.info")]
    AudioInfo,
    #[serde(rename = "audio.volume.set")]
    SetVolume { percent: u8 },
    #[serde(rename = "audio.output.select")]
    SelectAudioOutput { id: u32 },
    #[serde(rename = "audio.input.select")]
    SelectAudioInput { id: u32 },
    #[serde(rename = "audio.output_mute")]
    ToggleOutputMute,
    #[serde(rename = "audio.microphone_mute")]
    ToggleMicrophoneMute,
    #[serde(rename = "media.play_pause")]
    MediaPlayPause,
    #[serde(rename = "media.info")]
    MediaInfo,
    #[serde(rename = "media.artwork")]
    MediaArtwork {
        player_id: String,
        artwork_id: String,
    },
    #[serde(rename = "media.play_pause_for")]
    MediaPlayPauseFor { player_id: String },
    #[serde(rename = "media.next")]
    MediaNext { player_id: String },
    #[serde(rename = "media.previous")]
    MediaPrevious { player_id: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PowerAction {
    Suspend,
    Shutdown,
    Reboot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VolumeDirection {
    Up,
    Down,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestEnvelope {
    pub version: u16,
    pub id: String,
    #[serde(flatten)]
    pub operation: Operation,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponseEnvelope {
    pub version: u16,
    pub id: String,
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ResponseError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResponseError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ProtocolError {
    #[error("unsupported protocol version {0}")]
    UnsupportedVersion(u16),
    #[error("request id must be 1-64 ASCII letters, digits, '-' or '_'")]
    InvalidRequestId,
    #[error(
        "window id must be a 1-128 byte opaque identifier using ASCII letters, digits, '.', ':', '_' or '-'"
    )]
    InvalidWindowId,
    #[error("monitor name is invalid")]
    InvalidMonitorName,
    #[error("workspace id is invalid")]
    InvalidWorkspaceId,
    #[error("volume percent must be between 0 and 100")]
    InvalidVolumePercent,
    #[error("media player id is invalid")]
    InvalidMediaPlayerId,
    #[error("media artwork id is invalid")]
    InvalidMediaArtworkId,
    #[error("desktop entry id is invalid")]
    InvalidDesktopEntryId,
}

impl RequestEnvelope {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.version != PROTOCOL_VERSION {
            return Err(ProtocolError::UnsupportedVersion(self.version));
        }
        if self.id.is_empty()
            || self.id.len() > MAX_REQUEST_ID_BYTES
            || !self
                .id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
        {
            return Err(ProtocolError::InvalidRequestId);
        }
        let address = match &self.operation {
            Operation::FocusWindow { address }
            | Operation::MoveWindow { address, .. }
            | Operation::MoveWindowToMonitor { address, .. }
            | Operation::ToggleFloating { address }
            | Operation::ToggleFullscreen { address }
            | Operation::TogglePinned { address }
            | Operation::CloseWindow { address } => Some(address),
            _ => None,
        };
        if let Some(address) = address {
            if address.is_empty()
                || address.len() > MAX_WINDOW_ID_BYTES
                || !address
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b':' | b'_' | b'-'))
            {
                return Err(ProtocolError::InvalidWindowId);
            }
        }
        let workspace_id = match &self.operation {
            Operation::SwitchWorkspace { workspace_id }
            | Operation::MoveWindow { workspace_id, .. } => Some(workspace_id),
            _ => None,
        };
        if let Some(id) = workspace_id {
            if id.as_str().is_empty()
                || id.as_str().len() > MAX_WORKSPACE_ID_BYTES
                || id.as_str().chars().any(char::is_control)
            {
                return Err(ProtocolError::InvalidWorkspaceId);
            }
        }
        if let Operation::LaunchApplication { desktop_id } = &self.operation
            && (desktop_id.is_empty()
                || desktop_id.len() > 256
                || !desktop_id
                    .bytes()
                    .next()
                    .is_some_and(|byte| byte.is_ascii_alphanumeric())
                || !desktop_id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')))
        {
            return Err(ProtocolError::InvalidDesktopEntryId);
        }
        if let Operation::SetMonitorPower { monitor, .. } = &self.operation {
            if !valid_monitor_id(monitor) {
                return Err(ProtocolError::InvalidMonitorName);
            }
        }
        if let Operation::MoveWindowToMonitor { monitor, .. } = &self.operation
            && !valid_monitor_id(monitor)
        {
            return Err(ProtocolError::InvalidMonitorName);
        }
        if let Operation::SetVolume { percent } = self.operation
            && percent > 100
        {
            return Err(ProtocolError::InvalidVolumePercent);
        }
        let player_id = match &self.operation {
            Operation::MediaPlayPauseFor { player_id }
            | Operation::MediaNext { player_id }
            | Operation::MediaPrevious { player_id }
            | Operation::MediaArtwork { player_id, .. } => Some(player_id),
            _ => None,
        };
        if let Some(player_id) = player_id
            && (player_id.is_empty()
                || player_id.len() > MAX_MEDIA_PLAYER_ID_BYTES
                || !player_id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')))
        {
            return Err(ProtocolError::InvalidMediaPlayerId);
        }
        if let Operation::MediaArtwork { artwork_id, .. } = &self.operation
            && (artwork_id.len() != 64
                || !artwork_id
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
        {
            return Err(ProtocolError::InvalidMediaArtworkId);
        }
        Ok(())
    }
}

fn valid_monitor_id(monitor: &str) -> bool {
    !monitor.is_empty()
        && monitor.len() <= 64
        && monitor
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_envelope_round_trips_typed_workspace_operation() {
        let request = RequestEnvelope {
            version: PROTOCOL_VERSION,
            id: "req-001".into(),
            operation: Operation::SwitchWorkspace {
                workspace_id: 3.into(),
            },
        };
        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"type\":\"workspace.switch\""));
        assert_eq!(
            serde_json::from_str::<RequestEnvelope>(&json).unwrap(),
            request
        );
    }

    #[test]
    fn invalid_request_ids_and_unknown_operations_are_rejected() {
        let mut request = RequestEnvelope {
            version: PROTOCOL_VERSION,
            id: "bad id".into(),
            operation: Operation::Ping,
        };
        assert_eq!(request.validate(), Err(ProtocolError::InvalidRequestId));
        assert!(
            serde_json::from_str::<RequestEnvelope>(
                r#"{"version":1,"id":"x","type":"exec","payload":{"command":"id"}}"#
            )
            .is_err()
        );
        request.id = "x".repeat(65);
        assert_eq!(request.validate(), Err(ProtocolError::InvalidRequestId));
    }

    #[test]
    fn unsupported_versions_are_rejected() {
        let request = RequestEnvelope {
            version: 2,
            id: "x".into(),
            operation: Operation::Ping,
        };
        assert_eq!(
            request.validate(),
            Err(ProtocolError::UnsupportedVersion(2))
        );
    }

    #[test]
    fn every_window_action_rejects_unsafe_window_ids() {
        let operations = [
            Operation::FocusWindow {
                address: "0x01;shutdown".into(),
            },
            Operation::MoveWindow {
                address: "active window".into(),
                workspace_id: 2.into(),
            },
            Operation::ToggleFloating { address: "".into() },
            Operation::ToggleFullscreen {
                address: "window\n".into(),
            },
            Operation::TogglePinned {
                address: "invalid address".into(),
            },
            Operation::CloseWindow {
                address: "a".repeat(MAX_WINDOW_ID_BYTES + 1),
            },
        ];
        for operation in operations {
            let request = RequestEnvelope {
                version: PROTOCOL_VERSION,
                id: "safe".into(),
                operation,
            };
            assert_eq!(request.validate(), Err(ProtocolError::InvalidWindowId));
        }
    }

    #[test]
    fn window_operations_accept_safe_backend_opaque_identifiers() {
        let request = RequestEnvelope {
            version: PROTOCOL_VERSION,
            id: "request".into(),
            operation: Operation::FocusWindow {
                address: "sway:container_42".into(),
            },
        };
        assert_eq!(request.validate(), Ok(()));
    }

    #[test]
    fn toggle_pinned_uses_the_typed_window_operation_name() {
        let request = RequestEnvelope {
            version: PROTOCOL_VERSION,
            id: "pin".into(),
            operation: Operation::TogglePinned {
                address: "0xabc".into(),
            },
        };
        assert_eq!(request.validate(), Ok(()));
        assert_eq!(
            serde_json::to_value(request).unwrap()["type"],
            "window.toggle_pinned"
        );
    }

    #[test]
    fn move_window_to_monitor_uses_a_bounded_typed_operation() {
        let request: RequestEnvelope = serde_json::from_str(
            r#"{"version":1,"id":"monitor","type":"window.move_monitor","payload":{"address":"0xabc","monitor":"DP-1"}}"#,
        )
        .unwrap();
        assert_eq!(request.validate(), Ok(()));

        let unsafe_monitor: RequestEnvelope = serde_json::from_str(
            r#"{"version":1,"id":"monitor","type":"window.move_monitor","payload":{"address":"0xabc","monitor":"DP-1;shutdown"}}"#,
        )
        .unwrap();
        assert_eq!(
            unsafe_monitor.validate(),
            Err(ProtocolError::InvalidMonitorName)
        );
    }

    #[test]
    fn named_workspace_ids_round_trip_and_are_bounded() {
        let request: RequestEnvelope = serde_json::from_str(
            r#"{"version":1,"id":"switch","type":"workspace.switch","payload":{"workspace_id":"3: web"}}"#,
        )
        .unwrap();
        assert_eq!(request.validate(), Ok(()));
        assert!(serde_json::to_string(&request).unwrap().contains("3: web"));

        let oversized: RequestEnvelope = serde_json::from_str(&format!(
            r#"{{"version":1,"id":"switch","type":"workspace.switch","payload":{{"workspace_id":"{}"}}}}"#,
            "w".repeat(MAX_WORKSPACE_ID_BYTES + 1)
        ))
        .unwrap();
        assert_eq!(oversized.validate(), Err(ProtocolError::InvalidWorkspaceId));
    }

    #[test]
    fn system_actions_are_a_closed_allowlist_and_monitor_names_are_validated() {
        let reboot = RequestEnvelope {
            version: PROTOCOL_VERSION,
            id: "power".into(),
            operation: Operation::SystemPower {
                action: PowerAction::Reboot,
            },
        };
        let json = serde_json::to_string(&reboot).unwrap();
        assert!(json.contains("system.power"));
        assert_eq!(
            serde_json::from_str::<RequestEnvelope>(&json).unwrap(),
            reboot
        );
        assert!(
            serde_json::from_str::<RequestEnvelope>(
                r#"{"version":1,"id":"x","type":"system.exec","payload":{"command":"reboot"}}"#
            )
            .is_err()
        );

        let invalid_monitor = RequestEnvelope {
            version: PROTOCOL_VERSION,
            id: "monitor".into(),
            operation: Operation::SetMonitorPower {
                monitor: "DP-1;reboot".into(),
                enabled: false,
            },
        };
        assert_eq!(
            invalid_monitor.validate(),
            Err(ProtocolError::InvalidMonitorName)
        );
    }

    #[test]
    fn audio_state_and_volume_set_are_typed_allowlisted_operations() {
        let info: RequestEnvelope =
            serde_json::from_str(r#"{"version":1,"id":"audio","type":"audio.info"}"#).unwrap();
        assert_eq!(info.operation, Operation::AudioInfo);

        let set: RequestEnvelope = serde_json::from_str(
            r#"{"version":1,"id":"volume","type":"audio.volume.set","payload":{"percent":42}}"#,
        )
        .unwrap();
        assert_eq!(set.operation, Operation::SetVolume { percent: 42 });
        assert_eq!(set.validate(), Ok(()));
        assert!(
            serde_json::from_str::<RequestEnvelope>(
                r#"{"version":1,"id":"shell","type":"system.exec","payload":{"command":"reboot"}}"#
            )
            .is_err()
        );
        let out_of_range: RequestEnvelope = serde_json::from_str(
            r#"{"version":1,"id":"volume","type":"audio.volume.set","payload":{"percent":101}}"#,
        )
        .unwrap();
        assert_eq!(
            out_of_range.validate(),
            Err(ProtocolError::InvalidVolumePercent)
        );
    }

    #[test]
    fn mpris_operations_are_typed_and_player_ids_are_validated() {
        let info: RequestEnvelope =
            serde_json::from_str(r#"{"version":1,"id":"media","type":"media.info"}"#).unwrap();
        assert_eq!(info.operation, Operation::MediaInfo);

        let artwork: RequestEnvelope = serde_json::from_str(&format!(
            r#"{{"version":1,"id":"art","type":"media.artwork","payload":{{"player_id":"org.mpris.MediaPlayer2.test","artwork_id":"{}"}}}}"#,
            "a".repeat(64)
        ))
        .unwrap();
        assert!(artwork.validate().is_ok());
        let invalid_artwork: RequestEnvelope = serde_json::from_str(
            r#"{"version":1,"id":"art","type":"media.artwork","payload":{"player_id":"org.mpris.MediaPlayer2.test","artwork_id":"../../etc/passwd"}}"#,
        )
        .unwrap();
        assert_eq!(
            invalid_artwork.validate(),
            Err(ProtocolError::InvalidMediaArtworkId)
        );

        for (type_name, operation) in [
            (
                "media.play_pause_for",
                Operation::MediaPlayPauseFor {
                    player_id: "org.mpris.MediaPlayer2.test".into(),
                },
            ),
            (
                "media.next",
                Operation::MediaNext {
                    player_id: "org.mpris.MediaPlayer2.test".into(),
                },
            ),
            (
                "media.previous",
                Operation::MediaPrevious {
                    player_id: "org.mpris.MediaPlayer2.test".into(),
                },
            ),
        ] {
            let request: RequestEnvelope = serde_json::from_value(serde_json::json!({
                "version": 1,
                "id": "media-action",
                "type": type_name,
                "payload": { "player_id": "org.mpris.MediaPlayer2.test" }
            }))
            .unwrap();
            assert_eq!(request.operation, operation);
            assert_eq!(request.validate(), Ok(()));
        }
        let bad: RequestEnvelope = serde_json::from_str(
            r#"{"version":1,"id":"media-action","type":"media.next","payload":{"player_id":"player;reboot"}}"#,
        ).unwrap();
        assert_eq!(bad.validate(), Err(ProtocolError::InvalidMediaPlayerId));
    }

    #[test]
    fn application_launch_accepts_only_a_single_desktop_entry_id() {
        let valid: RequestEnvelope = serde_json::from_str(
            r#"{"version":1,"id":"launch","type":"app.launch","payload":{"desktop_id":"org.mozilla.Firefox"}}"#,
        )
        .expect("app.launch is a typed operation");
        assert_eq!(valid.validate(), Ok(()));

        let invalid: RequestEnvelope = serde_json::from_str(
            r#"{"version":1,"id":"launch","type":"app.launch","payload":{"desktop_id":"terminal; reboot"}}"#,
        )
        .expect("launch IDs are parsed before validation");
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn audio_device_selection_uses_typed_pipewire_ids() {
        let output: RequestEnvelope = serde_json::from_str(
            r#"{"version":1,"id":"output","type":"audio.output.select","payload":{"id":53}}"#,
        )
        .unwrap();
        let input: RequestEnvelope = serde_json::from_str(
            r#"{"version":1,"id":"input","type":"audio.input.select","payload":{"id":82}}"#,
        )
        .unwrap();
        assert_eq!(output.operation, Operation::SelectAudioOutput { id: 53 });
        assert_eq!(input.operation, Operation::SelectAudioInput { id: 82 });
    }

    #[test]
    fn output_mute_is_a_typed_allowlisted_audio_operation() {
        let request: RequestEnvelope =
            serde_json::from_str(r#"{"version":1,"id":"mute","type":"audio.output_mute"}"#)
                .unwrap();
        assert_eq!(request.operation, Operation::ToggleOutputMute);
        assert_eq!(request.validate(), Ok(()));
    }
}
