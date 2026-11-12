use base64::{Engine as _, engine::general_purpose::STANDARD};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

const MAX_DESKTOP_ENTRY_BYTES: u64 = 64 * 1024;
const MAX_ICON_PNG_BYTES: u64 = 32 * 1024;
const MAX_ICON_DIMENSION: u32 = 512;
const MAX_ICON_CACHE_ENTRIES: usize = 256;
pub(crate) const MAX_ICONS_PER_RESPONSE: usize = 32;

pub(crate) struct DesktopIconCache {
    entries: Mutex<HashMap<String, Option<String>>>,
    data_dirs: Vec<PathBuf>,
}

impl Default for DesktopIconCache {
    fn default() -> Self {
        Self {
            entries: Mutex::default(),
            data_dirs: xdg_data_dirs(),
        }
    }
}

impl DesktopIconCache {
    pub(crate) fn get(&self, app_id: &str) -> Option<String> {
        if let Some(icon) = self.entries.lock().ok()?.get(app_id) {
            return icon.clone();
        }
        let icon = resolve_icon_png_in(app_id, &self.data_dirs);
        if let Ok(mut entries) = self.entries.lock()
            && entries.len() < MAX_ICON_CACHE_ENTRIES
        {
            entries.insert(app_id.to_owned(), icon.clone());
        }
        icon
    }

    #[cfg(test)]
    fn with_data_dirs(data_dirs: Vec<PathBuf>) -> Self {
        Self {
            entries: Mutex::default(),
            data_dirs,
        }
    }
}

fn xdg_data_dirs() -> Vec<PathBuf> {
    let mut data_dirs = Vec::new();
    if let Some(data_home) = std::env::var_os("XDG_DATA_HOME").filter(|value| !value.is_empty()) {
        data_dirs.push(PathBuf::from(data_home));
    } else if let Some(home) = std::env::var_os("HOME").filter(|value| !value.is_empty()) {
        data_dirs.push(PathBuf::from(home).join(".local/share"));
    }
    let system_data_dirs =
        std::env::var_os("XDG_DATA_DIRS").unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    data_dirs.extend(std::env::split_paths(&system_data_dirs));
    data_dirs
}

pub(crate) fn assign_window_icons(
    windows: &mut [waycontrol_core::Window],
    cache: &DesktopIconCache,
) {
    let mut seen = std::collections::HashSet::new();
    let mut icons = HashMap::new();
    for app_id in windows.iter().map(|window| window.app_id.as_str()) {
        if app_id.is_empty() || !seen.insert(app_id) || icons.len() >= MAX_ICONS_PER_RESPONSE {
            continue;
        }
        icons.insert(app_id.to_owned(), cache.get(app_id));
    }
    for window in windows {
        window.icon_png_base64 = icons.get(&window.app_id).cloned().flatten();
    }
}

fn resolve_icon_png_in(app_id: &str, data_dirs: &[PathBuf]) -> Option<String> {
    if !safe_desktop_id(app_id) {
        return None;
    }
    for data_dir in data_dirs {
        let Ok(canonical_data_dir) = fs::canonicalize(data_dir) else {
            continue;
        };
        let desktop_path = canonical_data_dir
            .join("applications")
            .join(format!("{app_id}.desktop"));
        let Ok(desktop_path) = fs::canonicalize(desktop_path) else {
            continue;
        };
        let Ok(applications_root) = fs::canonicalize(canonical_data_dir.join("applications"))
        else {
            continue;
        };
        let Ok(metadata) = fs::metadata(&desktop_path) else {
            continue;
        };
        if !desktop_path.starts_with(&applications_root)
            || !metadata.is_file()
            || metadata.len() > MAX_DESKTOP_ENTRY_BYTES
        {
            continue;
        }
        let Ok(entry) = fs::read_to_string(desktop_path) else {
            continue;
        };
        let Some(icon_name) = desktop_icon_name(&entry) else {
            continue;
        };
        let candidates = icon_candidates(&canonical_data_dir, &icon_name);
        for candidate in candidates {
            let Some(bytes) = read_allowed_png(&candidate, &canonical_data_dir) else {
                continue;
            };
            return Some(STANDARD.encode(bytes));
        }
    }
    None
}

fn safe_desktop_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn desktop_icon_name(contents: &str) -> Option<String> {
    let mut in_desktop_entry = false;
    for line in contents.lines().take(512) {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_desktop_entry = line == "[Desktop Entry]";
        } else if in_desktop_entry && let Some(icon) = line.strip_prefix("Icon=") {
            let icon = icon.trim();
            return safe_icon_name(icon).then(|| icon.to_owned());
        }
    }
    None
}

fn safe_icon_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'/'))
}

fn icon_candidates(data_dir: &Path, name: &str) -> Vec<PathBuf> {
    if name.starts_with('/') {
        return vec![PathBuf::from(name)];
    }
    let mut candidates = Vec::new();
    for size in ["64x64", "48x48", "32x32", "128x128", "scalable"] {
        candidates.push(
            data_dir
                .join("icons/hicolor")
                .join(size)
                .join("apps")
                .join(format!("{name}.png")),
        );
    }
    candidates.push(data_dir.join("pixmaps").join(format!("{name}.png")));
    candidates
}

fn read_allowed_png(candidate: &Path, data_dir: &Path) -> Option<Vec<u8>> {
    let candidate = fs::canonicalize(candidate).ok()?;
    let icon_roots = [data_dir.join("icons"), data_dir.join("pixmaps")];
    if !icon_roots.iter().any(|root| {
        fs::canonicalize(root).is_ok_and(|canonical_root| candidate.starts_with(canonical_root))
    }) {
        return None;
    }
    let metadata = fs::metadata(&candidate).ok()?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_ICON_PNG_BYTES {
        return None;
    }
    let bytes = fs::read(candidate).ok()?;
    valid_png_dimensions(&bytes).then_some(bytes)
}

fn valid_png_dimensions(bytes: &[u8]) -> bool {
    if bytes.len() < 24 || !bytes.starts_with(b"\x89PNG\r\n\x1a\n") || &bytes[12..16] != b"IHDR" {
        return false;
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap_or_default());
    let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap_or_default());
    (1..=MAX_ICON_DIMENSION).contains(&width) && (1..=MAX_ICON_DIMENSION).contains(&height)
}

#[cfg(test)]
mod tests {
    use super::{DesktopIconCache, assign_window_icons, resolve_icon_png_in, valid_png_dimensions};
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use std::sync::atomic::{AtomicU64, Ordering};
    use waycontrol_core::Window;

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    struct TempDirectory(std::path::PathBuf);

    impl TempDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "waycontrol-icons-{}-{}",
                std::process::id(),
                NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn resolves_desktop_entry_to_a_bounded_png_icon() {
        let data_dir = TempDirectory::new();
        std::fs::create_dir_all(data_dir.0.join("applications")).unwrap();
        let icon_dir = data_dir.0.join("icons/hicolor/48x48/apps");
        std::fs::create_dir_all(&icon_dir).unwrap();
        std::fs::write(
            data_dir.0.join("applications/org.example.Editor.desktop"),
            format!(
                "[Desktop Entry]\nName=Editor\nComment={}\nIcon=editor-icon\n",
                "x".repeat(20 * 1024)
            ),
        )
        .unwrap();
        let png = STANDARD
            .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScLttAAAAABJRU5ErkJggg==")
            .unwrap();
        std::fs::write(icon_dir.join("editor-icon.png"), png).unwrap();

        let icon = resolve_icon_png_in("org.example.Editor", std::slice::from_ref(&data_dir.0));

        assert_eq!(
            icon.as_deref(),
            Some(
                "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScLttAAAAABJRU5ErkJggg=="
            )
        );
    }

    #[test]
    fn continues_searching_system_data_dirs_when_user_entry_is_missing() {
        let user_data = TempDirectory::new();
        let system_data = TempDirectory::new();
        std::fs::create_dir_all(system_data.0.join("applications")).unwrap();
        let icon_dir = system_data.0.join("icons/hicolor/48x48/apps");
        std::fs::create_dir_all(&icon_dir).unwrap();
        std::fs::write(
            system_data
                .0
                .join("applications/org.example.Editor.desktop"),
            "[Desktop Entry]\nIcon=editor-icon\n",
        )
        .unwrap();
        std::fs::write(
            icon_dir.join("editor-icon.png"),
            STANDARD
                .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScLttAAAAABJRU5ErkJggg==")
                .unwrap(),
        )
        .unwrap();

        let icon = resolve_icon_png_in(
            "org.example.Editor",
            &[user_data.0.clone(), system_data.0.clone()],
        );

        assert!(icon.is_some());
    }

    #[test]
    fn does_not_serve_an_icon_outside_the_xdg_data_directory() {
        let data_dir = TempDirectory::new();
        let external_dir = TempDirectory::new();
        std::fs::create_dir_all(data_dir.0.join("applications")).unwrap();
        let external_icon = external_dir.0.join("private.png");
        std::fs::write(
            &external_icon,
            STANDARD
                .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScLttAAAAABJRU5ErkJggg==")
                .unwrap(),
        )
        .unwrap();
        std::fs::write(
            data_dir.0.join("applications/org.example.Editor.desktop"),
            format!("[Desktop Entry]\nIcon={}\n", external_icon.display()),
        )
        .unwrap();

        assert!(
            resolve_icon_png_in("org.example.Editor", std::slice::from_ref(&data_dir.0)).is_none()
        );
    }

    #[test]
    fn rejects_application_ids_that_could_escape_the_applications_directory() {
        let data_dir = TempDirectory::new();
        assert!(resolve_icon_png_in("../outside", std::slice::from_ref(&data_dir.0)).is_none());
    }

    #[test]
    fn rejects_png_dimensions_above_the_android_decode_limit() {
        let mut png = STANDARD
            .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScLttAAAAABJRU5ErkJggg==")
            .unwrap();
        png[16..20].copy_from_slice(&513_u32.to_be_bytes());
        assert!(!valid_png_dimensions(&png));
    }

    #[test]
    fn attaches_local_icon_bytes_to_compositor_windows() {
        let data_dir = TempDirectory::new();
        std::fs::create_dir_all(data_dir.0.join("applications")).unwrap();
        let icon_dir = data_dir.0.join("icons/hicolor/48x48/apps");
        std::fs::create_dir_all(&icon_dir).unwrap();
        std::fs::write(
            data_dir.0.join("applications/org.example.Editor.desktop"),
            "[Desktop Entry]\nIcon=editor-icon\n",
        )
        .unwrap();
        std::fs::write(
            icon_dir.join("editor-icon.png"),
            STANDARD
                .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScLttAAAAABJRU5ErkJggg==")
                .unwrap(),
        )
        .unwrap();
        let cache = DesktopIconCache::with_data_dirs(vec![data_dir.0.clone()]);
        let mut windows = vec![Window {
            address: "0xabc".into(),
            title: "Editor".into(),
            app_id: "org.example.Editor".into(),
            workspace_id: None,
            monitor: None,
            floating: Some(false),
            pinned: None,
            icon_png_base64: None,
            fullscreen: false,
            focused: true,
        }];

        assign_window_icons(&mut windows, &cache);

        assert_eq!(
            windows[0].icon_png_base64.as_deref(),
            Some(
                "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScLttAAAAABJRU5ErkJggg=="
            )
        );
    }
}
