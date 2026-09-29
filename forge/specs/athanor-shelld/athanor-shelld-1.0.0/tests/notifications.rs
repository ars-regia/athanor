mod common;

use std::collections::HashMap;
use std::time::Duration;

use athanor_shelld::server::{NOTIFICATIONS_NAME, NOTIFICATIONS_PATH, PRIVATE_PATH};
use athanor_shelld::store::CAPACITY;
use athanor_shelld::wire::WireNotification;
use common::{Bus, APP_CGROUP, BAR_CGROUP};
use futures_util::StreamExt;
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

#[tokio::test]
async fn capabilities_and_server_information_are_the_specifications() {
    let bus = Bus::start("caps");
    let _daemon = bus.daemon(APP_CGROUP).await;
    let proxy = public(&bus.client().await).await;
    let caps: Vec<String> = proxy.call("GetCapabilities", &()).await.expect("caps");
    assert_eq!(caps, ["actions", "body", "icon-static", "persistence"]);
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
        .call::<_, _, (bool, Vec<WireNotification>)>("List", &())
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
        .call::<_, _, (bool, Vec<WireNotification>)>("List", &())
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
        (id, "twolines", "<b>hi</b>x")
    );
    notify(&public, id, "again", "", &[], HashMap::new()).await;
    let again: WireNotification = replaced
        .next()
        .await
        .expect("Replaced")
        .body()
        .deserialize()
        .expect("wire");
    assert_eq!((again.id, again.summary.as_str()), (id, "again"));
    let (dnd, listed): (bool, Vec<WireNotification>) =
        private.call("List", &()).await.expect("List");
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
    let bus = Bus::start("unicast");
    let _daemon = bus.daemon(BAR_CGROUP).await;
    let client = bus.client().await;
    let (public, private) = (public(&client).await, private(&client).await);
    private
        .call::<_, _, (bool, Vec<WireNotification>)>("List", &())
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
        .call::<_, _, (bool, Vec<WireNotification>)>("List", &())
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
    let (_, listed): (bool, Vec<WireNotification>) = private.call("List", &()).await.expect("List");
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
    let (dnd, listed): (bool, Vec<WireNotification>) =
        private.call("List", &()).await.expect("List");
    assert!(dnd);
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
async fn the_list_keeps_the_newest_hundred_and_says_so() {
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
    let (_, listed): (bool, Vec<WireNotification>) = private.call("List", &()).await.expect("List");
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
    let (_, listed): (bool, Vec<WireNotification>) = private.call("List", &()).await.expect("List");
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
            bar: athanor_shelld::sender::BarUnit::from_proc(),
        },
    )
    .await;
    assert!(
        second.is_err(),
        "the name is taken: the start must fail, not queue"
    );
}
