mod common;

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use athanor_shelld::sender::{Admitted, Caller};
use athanor_shelld::server::{NOTIFICATIONS_NAME, NOTIFICATIONS_PATH, PRIVATE_PATH};
use athanor_shelld::store::CAPACITY;
use athanor_shelld::wire::WireNotification;
use common::{Bus, APP_CGROUP, BAR_CGROUP, SESSION_CGROUP};
use futures_util::StreamExt;
use std::fs;
use zbus::{Connection, Proxy};
use zvariant::Value;

async fn public(conn: &Connection) -> Proxy<'static> {
    Proxy::new(
        conn,
        NOTIFICATIONS_NAME,
        NOTIFICATIONS_PATH,
        "org.freedesktop.Notifications",
    )
    .await
    .expect("proxy")
}

async fn private(conn: &Connection) -> Proxy<'static> {
    Proxy::new(
        conn,
        NOTIFICATIONS_NAME,
        PRIVATE_PATH,
        "os.athanor.Notifications1",
    )
    .await
    .expect("proxy")
}

async fn notify(
    proxy: &Proxy<'_>,
    replaces: u32,
    summary: &str,
    body: &str,
    actions: &[&str],
    hints: HashMap<&str, Value<'_>>,
) -> u32 {
    proxy
        .call(
            "Notify",
            &("app", replaces, "", summary, body, actions, hints, -1i32),
        )
        .await
        .expect("Notify")
}

/// The daemon, then the bar, the control center and an application, connected before it.
async fn units(bus: &Bus, cgroup: &str) -> (Connection, Connection, Connection, Connection) {
    let (bar, center, app) = (bus.client().await, bus.client().await, bus.client().await);
    let daemon = bus
        .daemon_for(
            cgroup,
            &[(&bar, Caller::Bar), (&center, Caller::ControlCenter)],
        )
        .await;
    (daemon, bar, center, app)
}

async fn admitted<B>(conn: &Connection, method: &str, body: &B) -> bool
where
    B: serde::Serialize + zvariant::DynamicType,
{
    match private(conn).await.call_method(method, body).await {
        Ok(_) => true,
        Err(err) => !err.to_string().contains("AccessDenied"),
    }
}

#[tokio::test]
async fn each_method_admits_the_units_of_the_table() {
    let bus = Bus::start("admit");
    let (_daemon, bar, center, app) = units(&bus, APP_CGROUP).await;
    macro_rules! row {
        ($method:literal, $body:expr, $bar:literal, $center:literal) => {
            assert_eq!(
                admitted(&bar, $method, &$body).await,
                $bar,
                "bar {}",
                $method
            );
            assert_eq!(
                admitted(&center, $method, &$body).await,
                $center,
                "center {}",
                $method
            );
            assert!(
                !admitted(&app, $method, &$body).await,
                "application {}",
                $method
            );
        };
    }
    row!("List", (), true, true);
    row!("History", (), false, true);
    row!("Close", (1u32, 2u32), true, true);
    row!("InvokeAction", (1u32, "default", ""), true, true);
    row!("Reply", (1u32, "hi"), true, true);
    row!("MarkRead", (vec![1u32],), true, true);
    row!("ClearAll", (), false, true);
    row!("ClearGroup", ("",), false, true);
    row!("DoNotDisturb", (), true, true);
    row!("SetDoNotDisturb", (false,), true, true);
    row!("SetDoNotDisturbUntil", (false, 0i64), true, true);
    row!("Rules", ("",), false, true);
    row!("SetRule", ("", "popups", "true"), false, true);
    row!("Settings", (), true, true);
    row!("SetSetting", ("sound", "true"), false, false);
    row!("ReportFullscreen", (true, false), true, false);
}

#[tokio::test]
async fn signals_reach_only_admitted_units() {
    let bus = Bus::start("unicast");
    let (_daemon, bar, center, app) = units(&bus, APP_CGROUP).await;
    let mut admitted_streams = Vec::new();
    for conn in [&bar, &center] {
        let proxy = private(conn).await;
        admitted_streams.push(proxy.receive_signal("Added").await.expect("subscribe"));
        let _: Vec<WireNotification> = proxy.call("List", &()).await.expect("List");
    }
    let mut to_app = private(&app)
        .await
        .receive_signal("Added")
        .await
        .expect("subscribe");
    let id = notify(&public(&app).await, 0, "s", "", &[], HashMap::new()).await;
    for stream in &mut admitted_streams {
        let (wire,): (WireNotification,) = stream
            .next()
            .await
            .expect("signal")
            .body()
            .deserialize()
            .expect("wire");
        assert_eq!(wire.id, id);
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(300), to_app.next())
            .await
            .is_err(),
        "a unicast signal never reaches an unadmitted connection"
    );
}

#[tokio::test]
async fn the_control_center_may_mute_but_not_grant_bypass() {
    let bus = Bus::start("mute");
    let (_daemon, bar, center, _app) = units(&bus, APP_CGROUP).await;
    let bar_proxy = private(&bar).await;
    let mut changed = bar_proxy
        .receive_signal("RulesChanged")
        .await
        .expect("subscribe");
    let _: Vec<WireNotification> = bar_proxy.call("List", &()).await.expect("List");
    let center_proxy = private(&center).await;
    center_proxy
        .call::<_, _, ()>("SetRule", &("org.example.Chat", "allowed", "false"))
        .await
        .expect("mute");
    let (app,): (String,) = changed
        .next()
        .await
        .expect("signal")
        .body()
        .deserialize()
        .expect("app");
    assert_eq!(app, "org.example.Chat");
    let err = center_proxy
        .call::<_, _, ()>("SetRule", &("org.example.Chat", "bypass_dnd", "true"))
        .await
        .expect_err("refused");
    assert!(err.to_string().contains("AccessDenied"), "{err}");
    let rules: HashMap<String, String> = center_proxy
        .call("Rules", &("org.example.Chat",))
        .await
        .expect("Rules");
    assert_eq!(rules.get("allowed").map(String::as_str), Some("false"));
    assert_eq!(rules.get("bypass_dnd").map(String::as_str), Some("false"));
}

#[tokio::test]
async fn the_history_survives_a_restart_of_the_daemon() {
    let bus = Bus::start("restart");
    let (_daemon, _bar, center, app) = units(&bus, APP_CGROUP).await;
    let sender = public(&app).await;
    let kept = notify(&sender, 0, "kept", "", &["default", "Open"], HashMap::new()).await;
    notify(
        &sender,
        0,
        "gone",
        "",
        &[],
        HashMap::from([("transient", Value::from(true))]),
    )
    .await;
    private(&center)
        .await
        .call::<_, _, ()>("MarkRead", &(vec![kept],))
        .await
        .expect("MarkRead");
    let file = bus.dir.join("state/notifications.json");
    common::wait_for_file(&file, "\"read\":true").await;

    // A second bus and daemon on a copy of the file: a restart, and a new bus as after a reboot.
    let second = Bus::start("restart-second");
    fs::create_dir_all(second.dir.join("state")).expect("mkdir");
    fs::copy(&file, second.dir.join("state/notifications.json")).expect("copy");
    let (_daemon2, _bar2, center2, app2) = units(&second, APP_CGROUP).await;
    let history: Vec<WireNotification> = private(&center2)
        .await
        .call("History", &())
        .await
        .expect("History");
    let seen: Vec<_> = history
        .iter()
        .map(|n| (n.id, n.summary.as_str(), n.read, n.actions_available))
        .collect();
    assert_eq!(
        seen,
        [(kept, "kept", true, false)],
        "on another bus the actions are unavailable"
    );
    let next = notify(&public(&app2).await, 0, "next", "", &[], HashMap::new()).await;
    assert!(next > kept);
}

#[tokio::test]
async fn a_spoofed_desktop_entry_lands_in_other_and_does_not_pass_dnd() {
    let bus = Bus::start("spoof");
    fs::create_dir_all(bus.dir.join("config/apps")).expect("mkdir");
    fs::write(
        bus.dir.join("config/apps/org.example.Chat.conf"),
        "bypass_dnd=true\n",
    )
    .expect("rule");
    let (_daemon, bar, _center, app) = units(&bus, SESSION_CGROUP).await;
    let bar_proxy = private(&bar).await;
    let mut added = bar_proxy.receive_signal("Added").await.expect("subscribe");
    let _: Vec<WireNotification> = bar_proxy.call("List", &()).await.expect("List");
    bar_proxy
        .call::<_, _, ()>("SetDoNotDisturb", &(true,))
        .await
        .expect("dnd");
    let hints = HashMap::from([("desktop-entry", Value::from("org.example.Chat"))]);
    notify(&public(&app).await, 0, "s", "", &[], hints).await;
    let (wire,): (WireNotification,) = added
        .next()
        .await
        .expect("signal")
        .body()
        .deserialize()
        .expect("wire");
    assert_eq!(
        (
            wire.app_id.as_str(),
            wire.desktop_entry.as_str(),
            wire.popup
        ),
        ("", "org.example.Chat", false)
    );
}

#[tokio::test]
async fn do_not_disturb_changed_reaches_both_units_with_its_reason() {
    let bus = Bus::start("dnd-changed");
    let (_daemon, bar, center, _app) = units(&bus, APP_CGROUP).await;
    let mut streams = Vec::new();
    for conn in [&bar, &center] {
        let proxy = private(conn).await;
        streams.push(
            proxy
                .receive_signal("DoNotDisturbChanged")
                .await
                .expect("subscribe"),
        );
        let _: Vec<WireNotification> = proxy.call("List", &()).await.expect("List");
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs() as i64;
    private(&center)
        .await
        .call::<_, _, ()>("SetDoNotDisturbUntil", &(true, now + 3600))
        .await
        .expect("set");
    for stream in &mut streams {
        let got: (bool, String, i64) = stream
            .next()
            .await
            .expect("signal")
            .body()
            .deserialize()
            .expect("args");
        assert_eq!(got, (true, "manual".to_owned(), now + 3600));
    }
}

#[tokio::test]
async fn capabilities_and_server_information_are_the_specifications() {
    let bus = Bus::start("caps");
    let _daemon = bus.daemon(APP_CGROUP).await;
    let proxy = public(&bus.client().await).await;
    let caps: Vec<String> = proxy.call("GetCapabilities", &()).await.expect("caps");
    assert_eq!(
        caps,
        [
            "actions",
            "body",
            "body-hyperlinks",
            "body-markup",
            "icon-static",
            "inline-reply",
            "persistence",
            "sound"
        ]
    );
    let info: (String, String, String, String) =
        proxy.call("GetServerInformation", &()).await.expect("info");
    assert_eq!(
        (info.0.as_str(), info.3.as_str()),
        ("athanor-shelld", "1.2")
    );
}

#[tokio::test]
async fn a_replace_keeps_the_id_and_an_unknown_one_gets_a_new_id() {
    let bus = Bus::start("ids");
    let _daemon = bus.daemon(APP_CGROUP).await;
    let proxy = public(&bus.client().await).await;
    let first = notify(&proxy, 0, "a", "", &[], HashMap::new()).await;
    assert_eq!(
        notify(&proxy, first, "a2", "", &[], HashMap::new()).await,
        first
    );
    let other = notify(&proxy, 9999, "b", "", &[], HashMap::new()).await;
    assert!(other != 9999 && other != first);
}

#[tokio::test]
async fn close_notification_announces_reason_3_and_a_second_close_fails() {
    let bus = Bus::start("close");
    let _daemon = bus.daemon(APP_CGROUP).await;
    let client = bus.client().await;
    let proxy = public(&client).await;
    let mut closed = proxy
        .receive_signal("NotificationClosed")
        .await
        .expect("subscribe");
    let id = notify(&proxy, 0, "a", "", &[], HashMap::new()).await;
    proxy
        .call::<_, _, ()>("CloseNotification", &(id,))
        .await
        .expect("close");
    let (got_id, reason): (u32, u32) = closed
        .next()
        .await
        .expect("signal")
        .body()
        .deserialize()
        .expect("args");
    assert_eq!((got_id, reason), (id, 3));
    assert!(proxy
        .call::<_, _, ()>("CloseNotification", &(id,))
        .await
        .is_err());
}

#[tokio::test]
async fn the_private_interface_refuses_a_process_outside_the_bar() {
    let bus = Bus::start("refuse");
    let _daemon = bus.daemon(APP_CGROUP).await;
    let proxy = private(&bus.client().await).await;
    let err = proxy
        .call::<_, _, Vec<WireNotification>>("List", &())
        .await
        .expect_err("refused");
    assert!(err.to_string().contains("AccessDenied"), "{err}");
    for (method, result) in [
        (
            "SetDoNotDisturb",
            proxy.call::<_, _, ()>("SetDoNotDisturb", &(true,)).await,
        ),
        (
            "Close",
            proxy.call::<_, _, ()>("Close", &(1u32, 2u32)).await,
        ),
        (
            "InvokeAction",
            proxy
                .call::<_, _, ()>("InvokeAction", &(1u32, "default", ""))
                .await,
        ),
    ] {
        assert!(
            result
                .expect_err(method)
                .to_string()
                .contains("AccessDenied"),
            "{method}"
        );
    }
}

#[tokio::test]
async fn the_bar_lists_clean_text_and_hears_added_replaced_closed() {
    let bus = Bus::start("bar");
    let _daemon = bus.daemon(BAR_CGROUP).await;
    let client = bus.client().await;
    let (public, private) = (public(&client).await, private(&client).await);
    // The bar lists on start (BR1): only after that is its unique name the private signals'
    // destination.
    private
        .call::<_, _, Vec<WireNotification>>("List", &())
        .await
        .expect("List");
    let mut added = private.receive_signal("Added").await.expect("subscribe");
    let mut replaced = private.receive_signal("Replaced").await.expect("subscribe");
    let mut closed = private.receive_signal("Closed").await.expect("subscribe");
    let id = notify(
        &public,
        0,
        "two\nlines",
        "<b>hi</b>\u{202E}x\u{0007}",
        &[],
        HashMap::new(),
    )
    .await;
    let first: WireNotification = added
        .next()
        .await
        .expect("Added")
        .body()
        .deserialize()
        .expect("wire");
    assert_eq!(
        (first.id, first.summary.as_str(), first.body.as_str()),
        (id, "twolines", "hix")
    );
    assert_eq!(first.body_spans[0], ("hi".to_owned(), 1, String::new()));
    notify(&public, id, "again", "", &[], HashMap::new()).await;
    let again: WireNotification = replaced
        .next()
        .await
        .expect("Replaced")
        .body()
        .deserialize()
        .expect("wire");
    assert_eq!((again.id, again.summary.as_str()), (id, "again"));
    let listed: Vec<WireNotification> = private.call("List", &()).await.expect("List");
    let (dnd, ..): (bool, String, i64, Vec<String>) = private
        .call("DoNotDisturb", &())
        .await
        .expect("DoNotDisturb");
    assert!(!dnd);
    assert_eq!(listed.iter().map(|n| n.id).collect::<Vec<_>>(), [id]);
    private
        .call::<_, _, ()>("Close", &(id, 2u32))
        .await
        .expect("Close");
    let (closed_id, reason): (u32, u32) = closed
        .next()
        .await
        .expect("Closed")
        .body()
        .deserialize()
        .expect("args");
    assert_eq!((closed_id, reason), (id, 2));
    assert!(
        private
            .call::<_, _, ()>("Close", &(id, 3u32))
            .await
            .is_err(),
        "reason 3 belongs to the application"
    );
}

#[tokio::test]
async fn private_signals_reach_only_the_bar_that_listed() {
    let bus = Bus::start("unicast-eavesdrop");
    let _daemon = bus.daemon(BAR_CGROUP).await;
    let client = bus.client().await;
    let (public, private) = (public(&client).await, private(&client).await);
    private
        .call::<_, _, Vec<WireNotification>>("List", &())
        .await
        .expect("List");
    let mut added = private.receive_signal("Added").await.expect("subscribe");

    // A second connection with a broad match rule for the private interface, but never
    // admitted (it never called List): without unicast it would see every notification's
    // content, exactly what BR1's private interface must never hand out.
    let bystander = bus.client().await;
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface("os.athanor.Notifications1")
        .expect("interface")
        .build();
    let mut eavesdrop = zbus::MessageStream::for_match_rule(rule, &bystander, None)
        .await
        .expect("match rule");

    let id = notify(&public, 0, "secret", "body", &[], HashMap::new()).await;
    let heard: WireNotification = added
        .next()
        .await
        .expect("Added")
        .body()
        .deserialize()
        .expect("wire");
    assert_eq!(heard.id, id, "the bar still hears it");

    let nothing = tokio::time::timeout(Duration::from_millis(300), eavesdrop.next()).await;
    assert!(
        nothing.is_err(),
        "a connection outside the bar received a private signal"
    );
}

#[tokio::test]
async fn no_private_signal_is_sent_before_any_bar_has_listed() {
    let bus = Bus::start("unlisted");
    let _daemon = bus.daemon(BAR_CGROUP).await;
    let client = bus.client().await;
    let (public, private) = (public(&client).await, private(&client).await);
    // No List call yet: the daemon has no destination for the private interface's signals,
    // so it must emit none at all, fail closed rather than broadcast.
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface("os.athanor.Notifications1")
        .expect("interface")
        .build();
    let mut eavesdrop = zbus::MessageStream::for_match_rule(rule, &client, None)
        .await
        .expect("match rule");

    notify(&public, 0, "s", "b", &[], HashMap::new()).await;

    let nothing = tokio::time::timeout(Duration::from_millis(300), eavesdrop.next()).await;
    assert!(
        nothing.is_err(),
        "a private signal was sent while no bar had listed"
    );
    // The notification itself is unaffected: it is still held.
    private
        .call::<_, _, Vec<WireNotification>>("List", &())
        .await
        .expect("List");
}

#[tokio::test]
async fn an_action_sends_the_token_first_then_closes_unless_resident() {
    let bus = Bus::start("action");
    let _daemon = bus.daemon(BAR_CGROUP).await;
    let client = bus.client().await;
    let (public, private) = (public(&client).await, private(&client).await);
    // A broadcast signal reaches only a connection with a match rule for it.
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .path(NOTIFICATIONS_PATH)
        .expect("path")
        .build();
    let mut stream = zbus::MessageStream::for_match_rule(rule, &client, None)
        .await
        .expect("match rule");
    let id = notify(&public, 0, "a", "", &["default", "Open"], HashMap::new()).await;
    assert!(
        private
            .call::<_, _, ()>("InvokeAction", &(id, "nope", "tok"))
            .await
            .is_err(),
        "unknown key"
    );
    private
        .call::<_, _, ()>("InvokeAction", &(id, "default", "token-1"))
        .await
        .expect("invoke");
    // Collect the signals of the public object in arrival order.
    let mut order = Vec::new();
    while order.len() < 3 {
        let message = stream.next().await.expect("message").expect("ok");
        let header = message.header();
        if header.path().map(|p| p.as_str()) == Some(NOTIFICATIONS_PATH) {
            if let Some(member) = header.member() {
                order.push(member.to_string());
            }
        }
    }
    assert_eq!(
        order,
        ["ActivationToken", "ActionInvoked", "NotificationClosed"]
    );

    let resident = notify(
        &public,
        0,
        "r",
        "",
        &["default", "Open"],
        HashMap::from([("resident", Value::Bool(true))]),
    )
    .await;
    private
        .call::<_, _, ()>("InvokeAction", &(resident, "default", ""))
        .await
        .expect("invoke");
    let listed: Vec<WireNotification> = private.call("List", &()).await.expect("List");
    assert!(
        listed.iter().any(|n| n.id == resident),
        "a resident notification stays"
    );
}

#[tokio::test]
async fn do_not_disturb_persists_and_ends_popups_but_critical() {
    let bus = Bus::start("dnd");
    {
        let _daemon = bus.daemon(BAR_CGROUP).await;
        let private = private(&bus.client().await).await;
        private
            .call::<_, _, ()>("SetDoNotDisturb", &(true,))
            .await
            .expect("set");
    }
    assert!(bus.dir.join("state/do-not-disturb").exists());
    let again = Bus::start("dnd-again");
    std::fs::create_dir_all(again.dir.join("state")).expect("mkdir");
    std::fs::copy(
        bus.dir.join("state/do-not-disturb"),
        again.dir.join("state/do-not-disturb"),
    )
    .expect("copy");
    let _daemon = again.daemon(BAR_CGROUP).await;
    let client = again.client().await;
    let (public, private) = (public(&client).await, private(&client).await);
    let normal = notify(&public, 0, "n", "", &[], HashMap::new()).await;
    let critical = notify(
        &public,
        0,
        "c",
        "",
        &[],
        HashMap::from([("urgency", Value::U8(2))]),
    )
    .await;
    let listed: Vec<WireNotification> = private.call("List", &()).await.expect("List");
    let (dnd, reason, ..): (bool, String, i64, Vec<String>) = private
        .call("DoNotDisturb", &())
        .await
        .expect("DoNotDisturb");
    assert_eq!((dnd, reason.as_str()), (true, "manual"));
    let left = |id| {
        listed
            .iter()
            .find(|n| n.id == id)
            .expect("listed")
            .popup_ms_left
    };
    assert_eq!((left(normal), left(critical)), (0, u32::MAX));
}

#[tokio::test]
async fn the_list_keeps_the_newest_five_hundred_and_says_so() {
    let bus = Bus::start("capacity");
    let _daemon = bus.daemon(BAR_CGROUP).await;
    let client = bus.client().await;
    let (public, private) = (public(&client).await, private(&client).await);
    let mut closed = public
        .receive_signal("NotificationClosed")
        .await
        .expect("subscribe");
    let first = notify(&public, 0, "0", "", &[], HashMap::new()).await;
    for n in 1..=CAPACITY {
        notify(&public, 0, &n.to_string(), "", &[], HashMap::new()).await;
    }
    let (id, reason): (u32, u32) = closed
        .next()
        .await
        .expect("signal")
        .body()
        .deserialize()
        .expect("args");
    assert_eq!((id, reason), (first, 1));
    let listed: Vec<WireNotification> = private.call("List", &()).await.expect("List");
    assert_eq!(listed.len(), CAPACITY);
}

#[tokio::test]
async fn images_are_scaled_and_bad_ones_dropped_without_failing_the_call() {
    let bus = Bus::start("images");
    let _daemon = bus.daemon(BAR_CGROUP).await;
    let client = bus.client().await;
    let (public, private) = (public(&client).await, private(&client).await);
    let good = Value::new((
        200i32,
        100i32,
        800i32,
        true,
        8i32,
        4i32,
        vec![255u8; 200 * 100 * 4],
    ));
    let bad = Value::new((
        100_000i32,
        1i32,
        400_000i32,
        true,
        8i32,
        4i32,
        vec![0u8; 16],
    ));
    let a = notify(
        &public,
        0,
        "good",
        "",
        &[],
        HashMap::from([("image-data", good)]),
    )
    .await;
    let b = notify(
        &public,
        0,
        "bad",
        "",
        &[],
        HashMap::from([
            ("image-data", bad),
            ("image-path", Value::from("https://x/y.png")),
        ]),
    )
    .await;
    let c = notify(
        &public,
        0,
        "huge",
        "",
        &[],
        HashMap::from([("x-huge", Value::new(vec![0u8; 256 * 1024]))]),
    )
    .await;
    let listed: Vec<WireNotification> = private.call("List", &()).await.expect("List");
    let get = |id| listed.iter().find(|n| n.id == id).expect("listed");
    assert_eq!((get(a).image_width, get(a).image_height), (96, 48));
    assert_eq!(
        (
            get(b).image_width,
            get(b).icon_file.as_str(),
            get(b).icon_name.as_str()
        ),
        (0, "", "")
    );
    assert!(listed.iter().any(|n| n.id == c));
}

#[tokio::test]
async fn a_second_daemon_on_the_same_bus_fails_to_start() {
    let bus = Bus::start("taken");
    let _daemon = bus.daemon(APP_CGROUP).await;
    let second = athanor_shelld::server::start(
        bus.builder(),
        athanor_shelld::server::Config {
            state_dir: bus.dir.join("state"),
            config_dir: bus.dir.join("config"),
            proc_root: bus.dir.join("proc"),
            admitted: Admitted::from_proc_root(bus.dir.join("proc")),
            player: athanor_shelld::sound::Player::default(),
        },
    )
    .await;
    assert!(
        second.is_err(),
        "the name is taken: the start must fail, not queue"
    );
}

#[tokio::test]
async fn ending_do_not_disturb_posts_a_summary_whose_default_action_opens_the_center() {
    let bus = Bus::start("summary");
    let (daemon, bar, _center, app) = units(&bus, APP_CGROUP).await;
    let bar_proxy = private(&bar).await;
    let mut added = bar_proxy.receive_signal("Added").await.expect("subscribe");
    let _: Vec<WireNotification> = bar_proxy.call("List", &()).await.expect("List");
    bar_proxy
        .call::<_, _, ()>("SetDoNotDisturb", &(true,))
        .await
        .expect("on");
    notify(&public(&app).await, 0, "hidden", "", &[], HashMap::new()).await;
    let (hidden,): (WireNotification,) = added
        .next()
        .await
        .expect("signal")
        .body()
        .deserialize()
        .expect("wire");
    assert!(!hidden.popup);
    // The control center's name, taken by a test double that records the call.
    struct Center(tokio::sync::mpsc::UnboundedSender<String>);
    #[zbus::interface(name = "os.athanor.ControlCenter1")]
    impl Center {
        fn show(&self, page: &str) {
            self.0.send(page.to_owned()).ok();
        }
    }
    let (tx, mut pages) = tokio::sync::mpsc::unbounded_channel();
    let _center = zbus::connection::Builder::address(bus.address.as_str())
        .expect("address")
        .serve_at("/os/athanor/ControlCenter1", Center(tx))
        .expect("serve")
        .name("os.athanor.ControlCenter1")
        .expect("name")
        .build()
        .await
        .expect("center");
    bar_proxy
        .call::<_, _, ()>("SetDoNotDisturb", &(false,))
        .await
        .expect("off");
    let (summary,): (WireNotification,) = added
        .next()
        .await
        .expect("signal")
        .body()
        .deserialize()
        .expect("wire");
    assert_eq!(
        summary.summary,
        "1 notification while do not disturb was on"
    );
    assert!(summary.transient && summary.popup);
    assert_eq!(summary.actions[0].0, "default");
    bar_proxy
        .call::<_, _, ()>("InvokeAction", &(summary.id, "default", ""))
        .await
        .expect("invoke");
    assert_eq!(pages.recv().await.as_deref(), Some("notifications"));
    drop(daemon);
}

#[tokio::test]
async fn the_summary_that_fills_the_store_closes_the_notification_it_evicts() {
    let bus = Bus::start("summary-evicts");
    let (_daemon, bar, _center, app) = units(&bus, APP_CGROUP).await;
    let bar_proxy = private(&bar).await;
    let mut closed = bar_proxy.receive_signal("Closed").await.expect("subscribe");
    let _: Vec<WireNotification> = bar_proxy.call("List", &()).await.expect("List");
    bar_proxy
        .call::<_, _, ()>("SetDoNotDisturb", &(true,))
        .await
        .expect("on");
    let public = public(&app).await;
    let first = notify(&public, 0, "oldest", "", &[], HashMap::new()).await;
    for n in 1..CAPACITY {
        notify(&public, 0, &n.to_string(), "", &[], HashMap::new()).await;
    }
    bar_proxy
        .call::<_, _, ()>("SetDoNotDisturb", &(false,))
        .await
        .expect("off");
    let (id, reason): (u32, u32) = closed
        .next()
        .await
        .expect("signal")
        .body()
        .deserialize()
        .expect("args");
    assert_eq!(
        (id, reason),
        (first, 1),
        "the summary evicted the oldest, and said so"
    );
}

/// A theme with the two sounds of NC7 and a stand-in for `pw-play` that logs its arguments.
fn sound_rig(bus: &Bus) -> (PathBuf, athanor_shelld::sound::Player) {
    use std::os::unix::fs::PermissionsExt;
    let stereo = bus.dir.join("data/sounds/freedesktop/stereo");
    std::fs::create_dir_all(&stereo).expect("theme");
    for name in ["message-new-instant", "dialog-warning", "bell"] {
        std::fs::write(stereo.join(format!("{name}.oga")), "").expect("sound");
    }
    let log = bus.dir.join("played");
    let program = bus.dir.join("fake-pw-play");
    std::fs::write(
        &program,
        format!("#!/bin/sh\necho \"$@\" >> {}\n", log.display()),
    )
    .expect("program");
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    (
        log,
        athanor_shelld::sound::Player::with_program(vec![bus.dir.join("data")], program),
    )
}

async fn played(log: &std::path::Path, lines: usize) -> String {
    for _ in 0..50 {
        let text = std::fs::read_to_string(log).unwrap_or_default();
        if text.lines().count() >= lines {
            return text;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    std::fs::read_to_string(log).unwrap_or_default()
}

#[tokio::test]
async fn a_notification_plays_the_themes_sound_and_never_the_sound_file_it_names() {
    let bus = Bus::start("sound");
    let (log, player) = sound_rig(&bus);
    let _daemon = bus.daemon_sounding(APP_CGROUP, player).await;
    let proxy = public(&bus.client().await).await;
    notify(
        &proxy,
        0,
        "s",
        "",
        &[],
        HashMap::from([("sound-file", Value::from("/etc/hostname"))]),
    )
    .await;
    let text = played(&log, 1).await;
    assert_eq!(
        text.trim(),
        format!(
            "--media-role Notification -- {}",
            bus.dir
                .join("data/sounds/freedesktop/stereo/message-new-instant.oga")
                .display()
        )
    );
    // The hinted name when the theme has it, the urgency's own when it does not.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    notify(
        &proxy,
        0,
        "s",
        "",
        &[],
        HashMap::from([("sound-name", Value::from("bell"))]),
    )
    .await;
    assert!(played(&log, 2)
        .await
        .lines()
        .nth(1)
        .is_some_and(|l| l.ends_with("bell.oga")));
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    notify(
        &proxy,
        0,
        "s",
        "",
        &[],
        HashMap::from([("suppress-sound", Value::Bool(true))]),
    )
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    assert_eq!(
        played(&log, 3).await.lines().count(),
        2,
        "suppress-sound is honoured"
    );
}

fn assert_invalid_args(result: zbus::Result<()>, why: &str) {
    match result {
        Err(zbus::Error::MethodError(name, _, _)) => {
            assert_eq!(name.as_str(), "org.freedesktop.DBus.Error.InvalidArgs", "{why}");
        }
        other => panic!("{why}: expected InvalidArgs, got {other:?}"),
    }
}

#[tokio::test]
async fn a_reply_reaches_the_sending_application_only_and_is_never_kept() {
    let bus = Bus::start("reply");
    let (_daemon, _bar, center, app) = units(&bus, APP_CGROUP).await;
    let third = bus.client().await;
    let (app_public, center_private) = (public(&app).await, private(&center).await);
    let mut mine = app_public
        .receive_signal("NotificationReplied")
        .await
        .expect("subscribe");
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .path(NOTIFICATIONS_PATH)
        .expect("path")
        .build();
    let mut eavesdropper = zbus::MessageStream::for_match_rule(rule, &third, None)
        .await
        .expect("match rule");
    let plain = notify(&app_public, 0, "plain", "", &[], HashMap::new()).await;
    assert_invalid_args(
        center_private.call::<_, _, ()>("Reply", &(plain, "x")).await,
        "no inline-reply action declared",
    );
    assert_invalid_args(
        center_private.call::<_, _, ()>("Reply", &(9999u32, "x")).await,
        "no such notification",
    );
    let id = notify(
        &app_public,
        0,
        "chat",
        "",
        &["inline-reply", "Answer"],
        HashMap::from([("resident", Value::Bool(true))]),
    )
    .await;
    let secret = "zq-reply-text-9f3";
    center_private
        .call::<_, _, ()>("Reply", &(id, secret))
        .await
        .expect("Reply");
    let (got, text): (u32, String) = mine
        .next()
        .await
        .expect("NotificationReplied")
        .body()
        .deserialize()
        .expect("args");
    assert_eq!((got, text.as_str()), (id, secret));
    // A resident notification stays; a closing one is announced to everyone, so the
    // eavesdropper hears nothing before it.
    let history = bus.dir.join("state/notifications.json");
    let listed: Vec<WireNotification> = center_private.call("List", &()).await.expect("List");
    assert!(listed.iter().any(|n| n.id == id), "a resident notification stays after a reply");
    let notice = notify(&app_public, 0, "t", "", &[], HashMap::new()).await;
    center_private
        .call::<_, _, ()>("Close", &(notice, 2u32))
        .await
        .expect("Close");
    let first = eavesdropper.next().await.expect("message").expect("ok");
    assert_ne!(
        first.header().member().map(|m| m.to_string()).as_deref(),
        Some("NotificationReplied"),
        "the reply is targeted"
    );
    tokio::time::sleep(Duration::from_millis(2500)).await;
    assert!(!fs::read_to_string(&history).expect("history").contains(secret));
}

#[tokio::test]
async fn a_replace_that_changes_only_the_value_does_not_pop_up_or_play() {
    let bus = Bus::start("progress");
    let (log, player) = sound_rig(&bus);
    let _daemon = bus.daemon_sounding(BAR_CGROUP, player).await;
    let client = bus.client().await;
    let (public, private) = (public(&client).await, private(&client).await);
    private
        .call::<_, _, Vec<WireNotification>>("List", &())
        .await
        .expect("List");
    let mut added = private.receive_signal("Added").await.expect("subscribe");
    let mut replaced = private.receive_signal("Replaced").await.expect("subscribe");
    let with = |value: i32| HashMap::from([("value", Value::I32(value))]);
    let id = notify(&public, 0, "copy", "", &[], with(10)).await;
    let first: WireNotification = added.next().await.expect("Added").body().deserialize().expect("wire");
    assert_eq!((first.value, first.popup), (10, true));
    assert_eq!(played(&log, 1).await.lines().count(), 1);

    notify(&public, id, "copy", "", &[], with(40)).await;
    let update: WireNotification = replaced.next().await.expect("Replaced").body().deserialize().expect("wire");
    assert_eq!((update.id, update.value, update.popup), (id, 40, false));
    assert!(update.popup_ms_left > 0, "the popup that shows keeps its time");

    notify(&public, id, "copy", "", &[], with(500)).await;
    let ignored: WireNotification = replaced.next().await.expect("Replaced").body().deserialize().expect("wire");
    assert_eq!(ignored.value, -1, "a value outside 0 to 100 is no value");

    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(played(&log, 1).await.lines().count(), 1, "the update played nothing");
    tokio::time::sleep(Duration::from_millis(300)).await;
    notify(&public, id, "copy done", "", &[], with(100)).await;
    let done: WireNotification = replaced.next().await.expect("Replaced").body().deserialize().expect("wire");
    assert!(done.popup, "a changed text pops up again");
    assert_eq!(played(&log, 2).await.lines().count(), 2);
}

#[tokio::test]
async fn a_reply_closes_a_notification_that_is_not_resident_and_fails_when_the_sender_left() {
    let bus = Bus::start("reply-outcomes");
    let (_daemon, _bar, center, app) = units(&bus, APP_CGROUP).await;
    let (app_public, center_private) = (public(&app).await, private(&center).await);
    let mut closed = app_public
        .receive_signal("NotificationClosed")
        .await
        .expect("subscribe");
    let id = notify(&app_public, 0, "chat", "", &["inline-reply", "Answer"], HashMap::new()).await;
    center_private
        .call::<_, _, ()>("Reply", &(id, "hi"))
        .await
        .expect("Reply");
    let (got, reason): (u32, u32) = closed
        .next()
        .await
        .expect("NotificationClosed")
        .body()
        .deserialize()
        .expect("args");
    assert_eq!((got, reason), (id, 2), "dismissed by the reply");

    let leaver = bus.client().await;
    let kept = notify(
        &public(&leaver).await,
        0,
        "gone",
        "",
        &["inline-reply", "Answer"],
        HashMap::from([("resident", Value::Bool(true))]),
    )
    .await;
    drop(leaver);
    // The daemon forgets the sender on NameOwnerChanged: ask until it has.
    let mut outcome = Ok(());
    for _ in 0..50 {
        outcome = center_private.call::<_, _, ()>("Reply", &(kept, "late")).await;
        if outcome.is_err() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_invalid_args(outcome, "the sender is gone");
}

#[tokio::test]
async fn only_the_sending_application_replaces_a_notification() {
    let bus = Bus::start("replace-owner");
    let (_daemon, _bar, _center, app) = units(&bus, APP_CGROUP).await;
    let second = bus.client().await;
    let (a, b) = (public(&app).await, public(&second).await);
    let id = notify(&a, 0, "mine", "", &["inline-reply", "Answer"], HashMap::new()).await;
    assert_eq!(
        notify(&b, id, "mine again", "", &[], HashMap::new()).await,
        id,
        "the same application from a new connection still replaces"
    );
    // Unidentified senders are told apart by their connection: B cannot take A's row.
    let other = Bus::start("replace-owner-other");
    let (_daemon, _bar, center, app) = units(&other, SESSION_CGROUP).await;
    let second = other.client().await;
    let (a, b) = (public(&app).await, public(&second).await);
    let mine = notify(&a, 0, "mine", "", &["inline-reply", "Answer"], HashMap::new()).await;
    let theirs = notify(&b, mine, "taken", "", &[], HashMap::new()).await;
    assert_ne!(theirs, mine);
    let listed: Vec<WireNotification> = private(&center).await.call("List", &()).await.expect("List");
    let held = listed.iter().find(|n| n.id == mine).expect("A's row stays");
    assert_eq!((held.summary.as_str(), held.reply), ("mine", true));
}

#[tokio::test]
async fn a_value_only_replace_keeps_the_read_state() {
    let bus = Bus::start("progress-read");
    let (_daemon, _bar, center, app) = units(&bus, APP_CGROUP).await;
    let (public, private) = (public(&app).await, private(&center).await);
    private
        .call::<_, _, Vec<WireNotification>>("List", &())
        .await
        .expect("List");
    let mut replaced = private.receive_signal("Replaced").await.expect("subscribe");
    let with = |value: i32| HashMap::from([("value", Value::I32(value))]);
    let id = notify(&public, 0, "copy", "", &[], with(10)).await;
    private.call::<_, _, ()>("MarkRead", &(vec![id],)).await.expect("MarkRead");
    notify(&public, id, "copy", "", &[], with(20)).await;
    let update: WireNotification = replaced.next().await.expect("Replaced").body().deserialize().expect("wire");
    assert_eq!((update.value, update.read), (20, true));
}

#[tokio::test]
async fn a_reply_goes_to_the_connection_that_sent_the_last_progress_update() {
    let bus = Bus::start("progress-sender");
    let (_daemon, _bar, center, app) = units(&bus, APP_CGROUP).await;
    let second = bus.client().await;
    let (first_public, second_public) = (public(&app).await, public(&second).await);
    let mut heard = second_public
        .receive_signal("NotificationReplied")
        .await
        .expect("subscribe");
    let with = |value: i32| HashMap::from([("value", Value::I32(value))]);
    let hints = |value: i32| {
        let mut hints = with(value);
        hints.insert("resident", Value::Bool(true));
        hints
    };
    let id = notify(&first_public, 0, "copy", "", &["inline-reply", "Answer"], hints(10)).await;
    // The same application, from a new process, reports the progress.
    notify(&second_public, id, "copy", "", &["inline-reply", "Answer"], hints(50)).await;
    drop(app);
    private(&center)
        .await
        .call::<_, _, ()>("Reply", &(id, "to the new process"))
        .await
        .expect("Reply");
    let (got, text): (u32, String) = heard
        .next()
        .await
        .expect("NotificationReplied")
        .body()
        .deserialize()
        .expect("args");
    assert_eq!((got, text.as_str()), (id, "to the new process"));
}

#[tokio::test]
async fn the_twenty_first_notification_of_a_burst_is_listed_without_a_popup() {
    let bus = Bus::start("rate");
    let (daemon, bar, center, app) = units(&bus, APP_CGROUP).await;
    let _daemon = daemon;
    let (public, private) = (public(&app).await, private(&bar).await);
    private
        .call::<_, _, Vec<WireNotification>>("List", &())
        .await
        .expect("List");
    let mut added = private.receive_signal("Added").await.expect("subscribe");
    for n in 0..21 {
        notify(&public, 0, &format!("burst {n}"), "", &[], HashMap::new()).await;
    }
    let mut popups = Vec::new();
    for _ in 0..21 {
        let wire: WireNotification = added.next().await.expect("Added").body().deserialize().expect("wire");
        popups.push(wire.popup);
    }
    assert!(popups[..20].iter().all(|p| *p), "the first twenty pop up");
    assert!(!popups[20], "the twenty-first does not");
    let history: Vec<WireNotification> = self::private(&center)
        .await
        .call("History", &())
        .await
        .expect("History");
    assert_eq!(history.len(), 21, "all of them are listed");
}

#[tokio::test]
async fn progress_updates_do_not_count_against_the_limit() {
    let bus = Bus::start("rate-progress");
    let (daemon, bar, _center, app) = units(&bus, APP_CGROUP).await;
    let _daemon = daemon;
    let (public, private) = (public(&app).await, private(&bar).await);
    private
        .call::<_, _, Vec<WireNotification>>("List", &())
        .await
        .expect("List");
    let mut added = private.receive_signal("Added").await.expect("subscribe");
    let with = |value: i32| HashMap::from([("value", Value::I32(value))]);
    let id = notify(&public, 0, "copy", "", &[], with(0)).await;
    for value in 1..=30 {
        notify(&public, id, "copy", "", &[], with(value)).await;
    }
    notify(&public, 0, "next", "", &[], HashMap::new()).await;
    let mut last = true;
    for _ in 0..2 {
        let wire: WireNotification = added.next().await.expect("Added").body().deserialize().expect("wire");
        last = wire.popup;
    }
    assert!(last, "thirty progress updates left the budget alone");
}

#[tokio::test]
async fn another_sender_cannot_close_a_notification_and_learns_nothing() {
    let bus = Bus::start("close-owner");
    let (_daemon, _bar, center, app) = units(&bus, SESSION_CGROUP).await;
    let second = bus.client().await;
    let (a, b) = (public(&app).await, public(&second).await);
    let mine = notify(&a, 0, "mine", "", &[], HashMap::new()).await;
    let denied = b
        .call::<_, _, ()>("CloseNotification", &(mine,))
        .await
        .expect_err("not B's to close");
    let unknown = b
        .call::<_, _, ()>("CloseNotification", &(mine + 100,))
        .await
        .expect_err("unknown id");
    assert_eq!(
        denied.to_string(),
        unknown.to_string().replace(&(mine + 100).to_string(), &mine.to_string()),
        "a held id answers as an unknown one"
    );
    let listed: Vec<WireNotification> = private(&center).await.call("List", &()).await.expect("List");
    assert_eq!(listed.iter().filter(|n| n.id == mine).count(), 1, "the row stays");
    let file = bus.dir.join("state/notifications.json");
    common::wait_for_file(&file, "\"mine\"").await;
    assert!(fs::read_to_string(&file).expect("history").contains("\"mine\""));
    a.call::<_, _, ()>("CloseNotification", &(mine,))
        .await
        .expect("A closes its own");
}

