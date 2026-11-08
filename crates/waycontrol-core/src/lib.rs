//! Shared, serializable domain models for WayControl.

use serde::{Deserialize, Serialize};

/// Opaque compositor workspace identifier.
///
/// Numeric IDs serialize as JSON numbers to preserve the version 1 Hyprland
/// wire format. Named workspaces serialize as strings. Deserialization accepts
/// both representations so existing clients can keep sending numeric IDs.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WorkspaceId(String);

impl WorkspaceId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn as_i64(&self) -> Option<i64> {
        self.0.parse().ok()
    }
}

impl From<i64> for WorkspaceId {
    fn from(value: i64) -> Self {
        Self(value.to_string())
    }
}

impl From<&str> for WorkspaceId {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl Serialize for WorkspaceId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        if let Some(number) = self.as_i64() {
            serializer.serialize_i64(number)
        } else {
            serializer.serialize_str(&self.0)
        }
    }
}

impl<'de> Deserialize<'de> for WorkspaceId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            Number(i64),
            String(String),
        }
        Ok(match Repr::deserialize(deserializer)? {
            Repr::Number(number) => Self::from(number),
            Repr::String(value) => Self(value),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    pub id: WorkspaceId,
    pub name: String,
    pub monitor: Option<String>,
    pub window_count: u64,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Window {
    pub address: String,
    pub title: String,
    pub app_id: String,
    pub workspace_id: Option<WorkspaceId>,
    pub monitor: Option<String>,
    pub floating: bool,
    pub fullscreen: bool,
    pub focused: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Monitor {
    pub name: String,
    pub dpms_enabled: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemorySnapshot {
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub used_bytes: u64,
    pub swap_total_bytes: u64,
    pub swap_used_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NetworkInterface {
    pub name: String,
    pub received_bytes: u64,
    pub transmitted_bytes: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_ids_preserve_numeric_wire_format_and_accept_names() {
        let numeric = WorkspaceId::from(3_i64);
        assert_eq!(serde_json::to_string(&numeric).unwrap(), "3");
        assert_eq!(serde_json::from_str::<WorkspaceId>("3").unwrap(), numeric);

        let named = WorkspaceId::from("web:main");
        assert_eq!(serde_json::to_string(&named).unwrap(), "\"web:main\"");
        assert_eq!(
            serde_json::from_str::<WorkspaceId>("\"web:main\"").unwrap(),
            named
        );
    }
}
