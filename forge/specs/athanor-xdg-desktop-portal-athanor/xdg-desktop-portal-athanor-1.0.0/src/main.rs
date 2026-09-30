use anyhow::Result;
use std::future::pending;
use tracing::info;

mod caller;
mod portal;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    info!("Starting Athanor XDG Desktop Portal (Privacy & Hardware Indicators)");

    // Export D-Bus interface for XDG Desktop Portal
    let _conn = zbus::connection::Builder::session()?
        .name("org.freedesktop.impl.portal.desktop.athanor")?
        .serve_at("/org/freedesktop/portal/desktop", portal::FileChooserPortal)?
        .build()
        .await?;

    info!("D-Bus Interface 'org.freedesktop.impl.portal.desktop.athanor' registered.");

    // The portal daemon stays alive indefinitely on the session bus
    pending::<()>().await;
    
    Ok(())
}
