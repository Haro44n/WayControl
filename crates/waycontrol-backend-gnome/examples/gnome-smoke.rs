use std::time::{Duration, Instant};
use waycontrol_backend_gnome::GnomeAdapter;
use waycontrol_compositor::CompositorBackend;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(75);
    let adapter = loop {
        match GnomeAdapter::connect().await {
            Ok(adapter) => break adapter,
            Err(error) if Instant::now() < deadline => {
                eprintln!("waiting for nested GNOME bridge: {error}");
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            Err(error) => return Err(error.into()),
        }
    };
    let workspaces = adapter.workspaces().await?;
    if workspaces.is_empty() {
        return Err("nested GNOME Shell returned no workspaces".into());
    }
    let windows = adapter.windows().await?;
    println!(
        "GNOME bridge v1 ready: {} workspace(s), {} window(s)",
        workspaces.len(),
        windows.len()
    );
    Ok(())
}
