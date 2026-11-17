//! What the tests share: a private dbus-daemon with its own configuration, so the tests do not
//! depend on where the host keeps `session.conf`, and the members of an introspection XML, as
//! the backend's Settings test reads them (`settings.rs`).

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::{env, fs, process};

pub struct Bus {
    daemon: Child,
    address: String,
    dir: PathBuf,
}

const CONFIG: &str = r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:tmpdir=/tmp</listen>
  <auth>EXTERNAL</auth>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
"#;

impl Bus {
    pub fn start(name: &str) -> Bus {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let dir = env::temp_dir().join(format!("athanor-portal-{name}-{}-{n}", process::id()));
        fs::create_dir_all(&dir).expect("mkdir");
        let config = dir.join("bus.conf");
        fs::write(&config, CONFIG).expect("config");
        let mut daemon = Command::new("dbus-daemon")
            .arg(format!("--config-file={}", config.display()))
            .args(["--nofork", "--nopidfile", "--print-address"])
            .arg(format!("--address=unix:path={}", dir.join("bus").display()))
            .stdout(Stdio::piped())
            .spawn()
            .expect("dbus-daemon");
        let mut address = String::new();
        BufReader::new(daemon.stdout.take().expect("stdout"))
            .read_line(&mut address)
            .expect("address");
        assert!(!address.trim().is_empty(), "dbus-daemon printed no address");
        Bus {
            daemon,
            address: address.trim().to_owned(),
            dir,
        }
    }

    pub fn address(&self) -> &str {
        &self.address
    }

    pub fn builder(&self) -> zbus::connection::Builder<'static> {
        zbus::connection::Builder::address(self.address.as_str())
    }
}

impl Drop for Bus {
    fn drop(&mut self) {
        // Test teardown: a daemon that already exited, or a directory already gone, is fine.
        self.daemon.kill().ok();
        self.daemon.wait().ok();
        fs::remove_dir_all(&self.dir).ok();
    }
}

pub type Member = (String, String, Vec<(String, String)>);

/// `(kind, name, [(type, direction or access)])` of each member in introspection XML.
/// Signal arguments are compared without a direction: upstream writes "out", zbus nothing.
pub fn members(xml: &str) -> std::collections::BTreeSet<Member> {
    let mut text = xml.to_owned();
    while let Some(start) = text.find("<!--") {
        let end = text[start..]
            .find("-->")
            .map_or(text.len(), |at| start + at + 3);
        text.replace_range(start..end, "");
    }
    let attr = |tag: &str, key: &str| {
        let needle = format!(" {key}=\"");
        tag.find(&needle)
            .map(|at| &tag[at + needle.len()..])
            .and_then(|rest| rest.split('"').next())
            .unwrap_or_default()
            .to_owned()
    };
    let mut found = std::collections::BTreeSet::new();
    let mut open: Option<Member> = None;
    for tag in text.split('<').skip(1).filter_map(|t| t.split('>').next()) {
        match tag.split_whitespace().next().unwrap_or_default() {
            kind @ ("method" | "signal") => {
                open = Some((kind.to_owned(), attr(tag, "name"), Vec::new()));
                if tag.ends_with('/') {
                    found.extend(open.take());
                }
            }
            "/method" | "/signal" => found.extend(open.take()),
            "arg" => {
                if let Some((kind, _, args)) = open.as_mut() {
                    let direction = match kind.as_str() {
                        "signal" => String::new(),
                        _ => attr(tag, "direction"),
                    };
                    args.push((attr(tag, "type"), direction));
                }
            }
            "property" => {
                found.insert((
                    "property".to_owned(),
                    attr(tag, "name"),
                    vec![(attr(tag, "type"), attr(tag, "access"))],
                ));
            }
            _ => {}
        }
    }
    found
}
