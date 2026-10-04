mod common;

use std::collections::HashMap;
use std::time::Duration;

use athanor_shelld::clock::{bus_changes, system_changes};
use common::Bus;
use tokio::sync::mpsc;
use zvariant::Value;

async fn heard(rx: &mut mpsc::Receiver<()>) -> bool {
    tokio::time::timeout(Duration::from_secs(3), rx.recv()).await == Ok(Some(()))
}

async fn silent(rx: &mut mpsc::Receiver<()>) -> bool {
    tokio::time::timeout(Duration::from_millis(400), rx.recv())
        .await
        .is_err()
}

/// logind and timedated as fakes on a private bus: only a resume and a timezone change count.
#[tokio::test]
async fn a_resume_and_a_new_timezone_are_heard_and_nothing_else_is() {
    let bus = Bus::start("clock");
    let system = bus.client().await;
    system
        .request_name("org.freedesktop.login1")
        .await
        .expect("login1");
    system
        .request_name("org.freedesktop.timedate1")
        .await
        .expect("timedate1");
    let (tx, mut rx) = mpsc::channel(8);
    let task = tokio::spawn(bus_changes(bus.client().await, tx));
    tokio::time::sleep(Duration::from_millis(300)).await; // the matches are installed

    let login = ("/org/freedesktop/login1", "org.freedesktop.login1.Manager");
    system
        .emit_signal(None::<&str>, login.0, login.1, "PrepareForSleep", &(true,))
        .await
        .expect("going down");
    assert!(silent(&mut rx).await, "going to sleep is not a change yet");
    system
        .emit_signal(None::<&str>, login.0, login.1, "PrepareForSleep", &(false,))
        .await
        .expect("resume");
    assert!(heard(&mut rx).await, "the resume");

    let props = (
        "/org/freedesktop/timedate1",
        "org.freedesktop.DBus.Properties",
    );
    let changed = |key: &str| HashMap::from([(key.to_owned(), Value::from("Europe/Rome"))]);
    system
        .emit_signal(
            None::<&str>,
            props.0,
            props.1,
            "PropertiesChanged",
            &(
                "org.freedesktop.timedate1",
                changed("NTP"),
                Vec::<String>::new(),
            ),
        )
        .await
        .expect("another property");
    assert!(silent(&mut rx).await, "NTP is not the timezone");
    system
        .emit_signal(
            None::<&str>,
            props.0,
            props.1,
            "PropertiesChanged",
            &(
                "org.freedesktop.timedate1",
                changed("Timezone"),
                Vec::<String>::new(),
            ),
        )
        .await
        .expect("timezone");
    assert!(heard(&mut rx).await, "the timezone");
    task.abort();
}

/// No system bus: the function returns, so the caller's timer alone remains, and it does not spin.
#[tokio::test]
async fn an_absent_system_bus_ends_the_bus_half_quietly() {
    let (tx, _rx) = mpsc::channel(1);
    std::env::set_var(
        "DBUS_SYSTEM_BUS_ADDRESS",
        "unix:path=/nonexistent/athanor-no-bus",
    );
    let done = tokio::time::timeout(Duration::from_secs(3), system_changes(tx)).await;
    assert!(done.is_ok(), "returned instead of retrying forever");
}
