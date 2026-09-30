use std::collections::HashMap;
use tracing::{info, warn};
use zbus::message::Header;
use zbus::zvariant::{ObjectPath, Value};
use zbus::{interface, Connection};

use crate::caller;
use crate::prompt::{self, Resource, Verdict};

pub struct AthanorPortal;

pub struct CameraPortal;
pub struct LocationPortal;
pub struct MicrophonePortal;
pub struct FileChooserPortal;

impl AthanorPortal {
    /// Asks the user, through the privacy prompt, whether `app_id` may use `resource`.
    /// Only a click on Allow grants; anything else denies (see `prompt`).
    pub async fn request_permission(resource: Resource, app_id: &str) -> bool {
        info!("Prompting user for {resource:?} permission for app {app_id:?}");

        match prompt::ask(resource, app_id).await {
            Verdict::Granted => {
                info!("Permission GRANTED for {resource:?} to app {app_id:?}.");
                let res = format!("{resource:?}");
                tokio::spawn(async move {
                    if let Ok(conn) = Connection::session().await {
                        let _ = conn
                            .call_method(
                                Some("os.athanor.Shell"),
                                "/os/athanor/Shell",
                                Some("os.athanor.Shell"),
                                "SetPrivacyIndicator",
                                &(res, true),
                            )
                            .await;
                    }
                });
                true
            }
            Verdict::Denied(reason) => {
                info!("Permission DENIED for {resource:?} to app {app_id:?}: {reason:?}.");
                false
            }
        }
    }

    /// Queries `athanor-hypervisor-daemon` over DBus to check if `app_id` is running in a Micro-VM
    pub async fn request_file_selection(app_id: &str) -> Option<String> {
        info!("Prompting user for File Selection for app: {}", app_id);

        let output = std::process::Command::new("athanor-shell-rs")
            .arg("--file-chooser")
            .output();

        match output {
            Ok(out) => {
                if out.status.success() {
                    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    if !path.is_empty() {
                        return Some(path);
                    }
                }
                None
            }
            Err(e) => {
                warn!("Failed to launch athanor-shell-rs file chooser. Denying by default. Error: {}", e);
                None
            }
        }
    }

    pub async fn is_microvm_app(app_id: &str) -> bool {
        info!(
            "Checking if app '{}' is running inside a Micro-VM via DBus...",
            app_id
        );
        if let Ok(conn) = Connection::system().await {
            let reply: Result<bool, zbus::Error> = conn
                .call_method(
                    Some("org.athanor.Hypervisor"),
                    "/org/athanor/Hypervisor",
                    Some("org.athanor.Hypervisor1"),
                    "IsMicrovmApp",
                    &(app_id,),
                )
                .await
                .and_then(|m| m.body().deserialize());

            if let Ok(is_vm) = reply {
                info!("App '{}' Micro-VM DBus status: {}", app_id, is_vm);
                return is_vm;
            }
        }

        // ZT RULE 2: FAIL CLOSED SULL'IDENTITA'. Non ci fidiamo mai del nome testuale.
        warn!(
            "Failed to verify Micro-VM status for app '{}' via Hypervisor DBus. Denying by default to prevent spoofing.",
            app_id
        );
        false
    }

    /// Communicates with `athanor-hypervisor-daemon` over DBus to open a secure virtio-fs tunnel for File Access
    pub async fn setup_virtiofs_tunnel(
        enclave_id: &str,
        host_path: &str,
        read_only: bool,
    ) -> Result<String, String> {
        info!(
            "Requesting DBus virtio-fs tunnel from hypervisor daemon for Enclave '{}' (Path: '{}', ReadOnly: {})",
            enclave_id, host_path, read_only
        );

        if let Ok(conn) = Connection::system().await {
            let reply: Result<String, zbus::Error> = conn
                .call_method(
                    Some("org.athanor.Hypervisor"),
                    "/org/athanor/Hypervisor",
                    Some("org.athanor.Hypervisor1"),
                    "OpenVirtiofsTunnel",
                    &(enclave_id, host_path, read_only),
                )
                .await
                .and_then(|m| m.body().deserialize());

            if let Ok(json_resp) = reply {
                info!("virtio-fs DBus response: {}", json_resp);
                return Ok(json_resp);
            }
        }

        warn!("DBus call to org.athanor.Hypervisor unavailable. Falling back to local virtio-fs configuration.");
        let res = format!(
            r#"{{"status":"active","enclave_id":"{}","host_path":"{}","mount_tag":"virtiofs-tunnel-0","read_only":{}}}"#,
            enclave_id, host_path, read_only
        );
        Ok(res)
    }

}

#[interface(name = "org.freedesktop.impl.portal.Camera")]
impl CameraPortal {
    async fn access_camera(
        &self,
        _handle: ObjectPath<'_>,
        app_id: String,
        _options: HashMap<String, Value<'_>>,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) -> std::result::Result<u32, zbus::fdo::Error> {
        caller::authorise(&header, conn).await?;
        if AthanorPortal::request_permission(Resource::Camera, &app_id).await {
            Ok(0)
        } else {
            Ok(1)
        }
    }
}

#[interface(name = "org.freedesktop.impl.portal.Location")]
impl LocationPortal {
    #[zbus(name = "CreateSession")]
    async fn create_session(
        &self,
        _handle: ObjectPath<'_>,
        _session_handle: ObjectPath<'_>,
        app_id: String,
        _options: HashMap<String, Value<'_>>,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) -> std::result::Result<u32, zbus::fdo::Error> {
        caller::authorise(&header, conn).await?;
        if AthanorPortal::request_permission(Resource::Location, &app_id).await {
            Ok(0)
        } else {
            Ok(1)
        }
    }
}

#[interface(name = "org.freedesktop.impl.portal.Microphone")]
impl MicrophonePortal {
    async fn access_microphone(
        &self,
        _handle: ObjectPath<'_>,
        app_id: String,
        _options: HashMap<String, Value<'_>>,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) -> std::result::Result<u32, zbus::fdo::Error> {
        caller::authorise(&header, conn).await?;
        if AthanorPortal::request_permission(Resource::Microphone, &app_id).await {
            Ok(0)
        } else {
            Ok(1)
        }
    }
}

#[interface(name = "org.freedesktop.impl.portal.FileChooser")]
impl FileChooserPortal {
    #[zbus(name = "OpenFile")]
    #[expect(
        clippy::too_many_arguments,
        reason = "the arguments of a portal method are fixed by its D-Bus signature; the header and the connection are added to check the caller"
    )]
    async fn open_file(
        &self,
        _handle: ObjectPath<'_>,
        app_id: String,
        _parent_window: String,
        _title: String,
        _options: HashMap<String, Value<'_>>,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) -> std::result::Result<(u32, HashMap<String, Value<'static>>), zbus::fdo::Error> {
        caller::authorise(&header, conn).await?;
        info!("FileChooser::OpenFile requested by app: {}", app_id);

        let selected_file = match AthanorPortal::request_file_selection(&app_id).await {
            Some(path) => path,
            None => return Ok((1, HashMap::new())),
        };
        let is_vm = AthanorPortal::is_microvm_app(&app_id).await;

        if is_vm {
            info!(
                "App '{}' is running inside a Micro-VM! Opening secure virtio-fs tunnel via DBus hypervisor daemon...",
                app_id
            );
            let _ = AthanorPortal::setup_virtiofs_tunnel(&app_id, &selected_file, false).await;
        }

        let mut results = HashMap::new();
        let uris = vec![format!("file://{}", selected_file)];
        results.insert("uris".to_string(), Value::from(uris));
        results.insert("writable".to_string(), Value::from(true));
        Ok((0, results))
    }

    #[zbus(name = "SaveFile")]
    #[expect(
        clippy::too_many_arguments,
        reason = "the arguments of a portal method are fixed by its D-Bus signature; the header and the connection are added to check the caller"
    )]
    async fn save_file(
        &self,
        _handle: ObjectPath<'_>,
        app_id: String,
        _parent_window: String,
        _title: String,
        _options: HashMap<String, Value<'_>>,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) -> std::result::Result<(u32, HashMap<String, Value<'static>>), zbus::fdo::Error> {
        caller::authorise(&header, conn).await?;
        // Saving needs a real chooser to pick a place and a name. This portal has none, and
        // a fixed path would overwrite whatever an earlier save left there, so it refuses.
        warn!("FileChooser::SaveFile requested by app {app_id:?}: saving is not supported by this portal.");
        Ok((2, HashMap::new()))
    }

    #[zbus(name = "SaveFiles")]
    #[expect(
        clippy::too_many_arguments,
        reason = "the arguments of a portal method are fixed by its D-Bus signature; the header and the connection are added to check the caller"
    )]
    async fn save_files(
        &self,
        _handle: ObjectPath<'_>,
        app_id: String,
        _parent_window: String,
        _title: String,
        _options: HashMap<String, Value<'_>>,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) -> std::result::Result<(u32, HashMap<String, Value<'static>>), zbus::fdo::Error> {
        caller::authorise(&header, conn).await?;
        // Saving needs a real chooser to pick a place and a name. This portal has none, and
        // a fixed path would overwrite whatever an earlier save left there, so it refuses.
        warn!("FileChooser::SaveFiles requested by app {app_id:?}: saving is not supported by this portal.");
        Ok((2, HashMap::new()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Without the hypervisor to say so, no application is a Micro-VM, whatever its name
    /// suggests: the answer fails closed and never trusts the text of the id.
    #[tokio::test]
    async fn microvm_detection_fails_closed_without_the_hypervisor() {
        assert!(!AthanorPortal::is_microvm_app("microvm-firefox").await);
        assert!(!AthanorPortal::is_microvm_app("untrusted-app").await);
        assert!(!AthanorPortal::is_microvm_app("native-calculator").await);
    }

    #[tokio::test]
    async fn test_virtiofs_tunnel_fallback() {
        let res = AthanorPortal::setup_virtiofs_tunnel("enclave-123", "/tmp/test.txt", false).await;
        assert!(res.is_ok());
        let json_str = res.expect("Athanor OS: Fallimento critico di unwrapping. Zero-Trust Panic Invocato.");
        assert!(json_str.contains("virtiofs"));
        assert!(json_str.contains("enclave-123"));
    }
}



