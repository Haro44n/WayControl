//! WayControl user daemon and local administration CLI.

use axum_server::tls_rustls::RustlsConfig;
use qrcodegen::{QrCode, QrCodeEcc};
use std::{
    fs,
    io::{self, Write},
    net::{IpAddr, SocketAddr},
    os::unix::fs::{FileTypeExt, PermissionsExt},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UdpSocket, UnixListener, UnixStream},
};
use waycontrol_compositor::CompositorBackend;
use waycontrol_server::{
    AdminRequest, AdminResponse, AppState, HostSystemControls, SystemControlBackend,
    WaylandBackend, admin_command, ensure_server_certificate, lan_discovery_bind_address, router,
    run_lan_discovery, socket_address_from_env,
};

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("waycontrold: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref().unwrap_or("inspect") {
        "inspect" => inspect().await,
        "serve" => serve().await,
        "pairing-info" => match args.next().as_deref() {
            None => print_admin("pairing_info", None).await,
            Some("--qr") => print_pairing_qr().await,
            Some(_) => Err("usage: waycontrold pairing-info [--qr]".into()),
        },
        "pending" => print_admin("pending", None).await,
        "devices" => print_admin("devices", None).await,
        "approve" => confirm_admin_action("approve", args.next(), "Approve this device pairing? [y/N] ").await,
        "revoke" => confirm_admin_action("revoke", args.next(), "Revoke this trusted device? [y/N] ").await,
        command => Err(format!("unknown command '{command}'. Use inspect, serve, pairing-info [--qr], pending, devices, approve ID, or revoke ID").into()),
    }
}

async fn inspect() -> Result<(), Box<dyn std::error::Error>> {
    let uptime_seconds = fs::read_to_string("/proc/uptime")?
        .split_whitespace()
        .next()
        .unwrap_or("0")
        .parse::<f64>()?;
    let memory = fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|s| waycontrol_telemetry::parse_memory(&s).ok());
    let network = fs::read_to_string("/proc/net/dev")
        .ok()
        .and_then(|s| waycontrol_telemetry::parse_network_interfaces(&s).ok());
    let cpu_first = fs::read_to_string("/proc/stat")
        .ok()
        .and_then(|s| waycontrol_telemetry::parse_cpu_times(&s).ok());
    tokio::time::sleep(Duration::from_millis(120)).await;
    let cpu_second = fs::read_to_string("/proc/stat")
        .ok()
        .and_then(|s| waycontrol_telemetry::parse_cpu_times(&s).ok());
    let cpu = cpu_first
        .zip(cpu_second)
        .and_then(|(a, b)| waycontrol_telemetry::cpu_percent(a.total, b.total, a.idle, b.idle));
    let backend = WaylandBackend::detect_async().await;
    let compositor = match backend.workspaces().await {
        Ok(workspaces) => serde_json::json!({"available":true,"workspaces":workspaces}),
        Err(error) => serde_json::json!({"available":false,"error":error}),
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "computer_name":fs::read_to_string("/etc/hostname").unwrap_or_default().trim(),
            "uptime_seconds":uptime_seconds,
            "cpu_usage_percent":cpu,
            "memory":memory,
            "network_interfaces":network,
            "compositor":backend.info(),
            "capabilities":backend.capabilities(),
            "state":compositor
        }))?
    );
    Ok(())
}

async fn serve() -> Result<(), Box<dyn std::error::Error>> {
    let bind = socket_address_from_env()?;
    let data_dir = data_dir()?;
    let (cert_path, key_path, fingerprint) = ensure_server_certificate(&data_dir, bind)?;
    let compositor = Arc::new(WaylandBackend::detect_async().await);
    let system: Arc<dyn SystemControlBackend> = Arc::new(HostSystemControls);
    let state = AppState::new(compositor, system, &data_dir).await?;
    let admin_path = admin_socket_path()?;
    let listener = bind_admin_socket(&admin_path)?;
    eprintln!("WayControl HTTPS listener bound at {bind}; certificate SHA-256 {fingerprint}");
    eprintln!("Local administration socket: {}", admin_path.display());
    let tls = RustlsConfig::from_pem_file(&cert_path, &key_path).await?;
    let discovery = if let Some(discovery_bind) = lan_discovery_bind_address(bind) {
        match UdpSocket::bind(discovery_bind).await {
            Ok(socket) => {
                eprintln!("LAN discovery listener bound at {discovery_bind}");
                let hostname = fs::read_to_string("/etc/hostname")
                    .unwrap_or_else(|_| "WayControl desktop".into());
                Some((socket, hostname))
            }
            Err(error) => {
                eprintln!(
                    "LAN discovery disabled because {discovery_bind} could not be bound: {error}"
                );
                None
            }
        }
    } else {
        None
    };
    let web = axum_server::bind_rustls(bind, tls).serve(router(state.clone()).into_make_service());
    let discovery_task = async move {
        if let Some((socket, hostname)) = discovery {
            run_lan_discovery(socket, bind, &hostname).await?;
        }
        std::future::pending::<Result<(), std::io::Error>>().await
    };
    tokio::select! {
        result = web => result.map_err(Into::into),
        result = admin_loop(listener, state, bind.to_string(), fingerprint) => result,
        result = discovery_task => result.map_err(Into::into),
    }
}

async fn confirm_admin_action(
    action: &str,
    id: Option<String>,
    prompt: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(id) = id else {
        return Err("an id is required".into());
    };
    let inventory_action = if action == "approve" {
        "pending"
    } else {
        "devices"
    };
    let inventory = send_admin(AdminRequest {
        action: inventory_action.into(),
        id: None,
    })
    .await?;
    let label = if action == "approve" {
        inventory
            .pending
            .unwrap_or_default()
            .into_iter()
            .find(|item| item.request_id == id)
            .map(|item| item.device_name)
    } else {
        inventory
            .devices
            .unwrap_or_default()
            .into_iter()
            .find(|item| item.device_id == id)
            .map(|item| item.name)
    }
    .ok_or("the requested device is not pending/trusted")?;
    print!("{prompt}{label} ({}): ", id);
    io::stdout().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    if !matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
        println!("Cancelled.");
        return Ok(());
    }
    let response = send_admin(AdminRequest {
        action: action.into(),
        id: Some(id),
    })
    .await?;
    println!("{}", serde_json::to_string_pretty(&response)?);
    if response.ok {
        Ok(())
    } else {
        Err(response
            .message
            .unwrap_or_else(|| "local action failed".into())
            .into())
    }
}

async fn print_admin(action: &str, id: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    let response = send_admin(AdminRequest {
        action: action.into(),
        id,
    })
    .await?;
    println!("{}", serde_json::to_string_pretty(&response)?);
    if response.ok {
        Ok(())
    } else {
        Err(response
            .message
            .unwrap_or_else(|| "local action failed".into())
            .into())
    }
}

async fn print_pairing_qr() -> Result<(), Box<dyn std::error::Error>> {
    let response = send_admin(AdminRequest {
        action: "pairing_info".into(),
        id: None,
    })
    .await?;
    if !response.ok {
        return Err(response
            .message
            .unwrap_or_else(|| "could not obtain pairing details".into())
            .into());
    }
    let pairing = response.pairing.ok_or("pairing code is unavailable")?;
    let server_address = response
        .server_address
        .ok_or("server address is unavailable")?;
    let fingerprint = response
        .certificate_sha256
        .ok_or("server certificate fingerprint is unavailable")?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let expires_at = now.saturating_add(pairing.expires_in_seconds);
    let payload = pairing_qr_payload(
        &server_address,
        &fingerprint,
        &pairing.code,
        expires_at,
        now,
    )?;
    println!("Scan this QR code in WayControl on the same local network:");
    print!("{}", render_qr(&payload)?);
    println!("HTTPS address: https://{server_address}");
    println!("Certificate SHA-256: {fingerprint}");
    println!(
        "Pairing code expires in {} seconds.",
        pairing.expires_in_seconds
    );
    Ok(())
}

fn pairing_qr_payload(
    server_address: &str,
    fingerprint: &str,
    code: &str,
    expires_at_unix: u64,
    now_unix: u64,
) -> Result<String, String> {
    let address = server_address
        .parse::<SocketAddr>()
        .map_err(|_| "server address is not a valid IP endpoint".to_owned())?;
    let IpAddr::V4(ip) = address.ip() else {
        return Err("QR pairing currently requires an IPv4 LAN address".into());
    };
    if !(ip.is_private() || ip.is_link_local()) {
        return Err("QR pairing requires a private or link-local IPv4 address".into());
    }
    if fingerprint.len() != 64 || !fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("server certificate fingerprint is invalid".into());
    }
    if code.len() != 6 || !code.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("pairing code is invalid".into());
    }
    if expires_at_unix <= now_unix {
        return Err("pairing code has expired; request a fresh code".into());
    }
    serde_json::to_string(&serde_json::json!({
        "version": 1,
        "endpoint": format!("https://{address}"),
        "fingerprint": fingerprint.to_ascii_lowercase(),
        "code": code,
        "expires_at_unix": expires_at_unix,
    }))
    .map_err(|error| format!("could not encode pairing details: {error}"))
}

fn render_qr(payload: &str) -> Result<String, String> {
    let qr = QrCode::encode_text(payload, QrCodeEcc::Medium)
        .map_err(|error| format!("could not encode QR code: {error}"))?;
    let size = qr.size();
    let border = 4;
    let mut rendered = String::new();
    for y in -border..size + border {
        for x in -border..size + border {
            let dark = x >= 0 && y >= 0 && x < size && y < size && qr.get_module(x, y);
            rendered.push_str(if dark { "██" } else { "  " });
        }
        rendered.push('\n');
    }
    Ok(rendered)
}

async fn send_admin(request: AdminRequest) -> Result<AdminResponse, Box<dyn std::error::Error>> {
    let mut stream = UnixStream::connect(admin_socket_path()?).await?;
    let mut payload = serde_json::to_vec(&request)?;
    payload.push(b'\n');
    stream.write_all(&payload).await?;
    stream.shutdown().await?;
    let mut response = Vec::new();
    stream.take(16 * 1024).read_to_end(&mut response).await?;
    Ok(serde_json::from_slice(&response)?)
}

async fn admin_loop(
    listener: UnixListener,
    state: AppState,
    address: String,
    fingerprint: String,
) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        let (mut stream, _) = listener.accept().await?;
        let mut input = vec![0u8; 8192];
        let size = stream.read(&mut input).await?;
        if size == 0 {
            continue;
        }
        let response = match serde_json::from_slice::<AdminRequest>(&input[..size]) {
            Ok(request) => {
                let mut response = admin_command(&state, request).await;
                response.server_address = Some(address.clone());
                response.certificate_sha256 = Some(fingerprint.clone());
                response
            }
            Err(_) => AdminResponse {
                ok: false,
                pairing: None,
                pending: None,
                devices: None,
                message: Some("invalid local request".into()),
                server_address: None,
                certificate_sha256: None,
            },
        };
        let mut bytes = serde_json::to_vec(&response)?;
        bytes.push(b'\n');
        stream.write_all(&bytes).await?;
    }
}

fn bind_admin_socket(path: &std::path::Path) -> Result<UnixListener, Box<dyn std::error::Error>> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.file_type().is_socket() {
            fs::remove_file(path)?;
        } else {
            return Err(format!(
                "refusing to replace non-socket admin path {}",
                path.display()
            )
            .into());
        }
    }
    let listener = UnixListener::bind(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

fn data_dir() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .ok_or("neither XDG_CONFIG_HOME nor HOME is set")?;
    Ok(resolve_data_dir(&base)?)
}

fn resolve_data_dir(base: &std::path::Path) -> std::io::Result<PathBuf> {
    let current = base.join("waycontrol");
    let legacy = base.join("hyprcontrol");
    if !current.exists() && legacy.exists() {
        fs::rename(legacy, &current)?;
    }
    Ok(current)
}

fn admin_socket_path() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .ok_or("XDG_RUNTIME_DIR is required for the local administration socket")?;
    Ok(runtime.join("waycontrol-admin.sock"))
}

#[cfg(test)]
mod tests {
    use super::{pairing_qr_payload, render_qr, resolve_data_dir};
    use std::fs;

    #[test]
    fn migrates_existing_hyprcontrol_data_directory_without_losing_credentials() {
        let unique = format!(
            "waycontrol-migration-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let base = std::env::temp_dir().join(unique);
        let legacy = base.join("hyprcontrol");
        fs::create_dir_all(&legacy).unwrap();
        fs::write(legacy.join("trusted-devices.json"), b"[]").unwrap();

        let migrated = resolve_data_dir(&base).unwrap();

        assert_eq!(migrated, base.join("waycontrol"));
        assert_eq!(
            fs::read(migrated.join("trusted-devices.json")).unwrap(),
            b"[]"
        );
        assert!(!legacy.exists());
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn pairing_qr_payload_is_scoped_to_private_tls_and_expires() {
        let now = 1_800_000_000;
        let payload = pairing_qr_payload(
            "192.168.1.20:8765",
            &"a".repeat(64),
            "042381",
            now + 120,
            now,
        )
        .unwrap();
        let value: serde_json::Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(value["version"], 1);
        assert_eq!(value["endpoint"], "https://192.168.1.20:8765");
        assert_eq!(value["fingerprint"], "a".repeat(64));
        assert_eq!(value["code"], "042381");
        assert_eq!(value["expires_at_unix"], now + 120);
        assert_eq!(value.as_object().unwrap().len(), 5);
        assert!(value.get("token").is_none());
        assert!(
            pairing_qr_payload("127.0.0.1:8765", &"a".repeat(64), "042381", now + 120, now)
                .is_err()
        );
        assert!(
            pairing_qr_payload("8.8.8.8:8765", &"a".repeat(64), "042381", now + 120, now).is_err()
        );
        assert!(
            pairing_qr_payload(
                "192.168.1.20:8765",
                &"a".repeat(64),
                "12345x",
                now + 120,
                now
            )
            .is_err()
        );
        assert!(
            pairing_qr_payload("192.168.1.20:8765", &"a".repeat(64), "042381", now, now).is_err()
        );
    }

    #[test]
    fn pairing_qr_renderer_produces_a_square_symbol_with_a_quiet_zone() {
        let rendered = render_qr("waycontrol pairing payload").unwrap();
        let lines = rendered.lines().collect::<Vec<_>>();
        assert!(lines.len() >= 25);
        assert!(
            lines
                .iter()
                .all(|line| line.chars().count() == lines[0].chars().count())
        );
        assert!(
            lines
                .first()
                .unwrap()
                .chars()
                .all(|character| character == ' ')
        );
        assert!(
            lines
                .last()
                .unwrap()
                .chars()
                .all(|character| character == ' ')
        );
        assert!(render_qr(&"x".repeat(5000)).is_err());
    }
}
