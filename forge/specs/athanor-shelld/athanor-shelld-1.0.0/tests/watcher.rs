mod common;

use std::time::Duration;

use athanor_shelld::server::{WATCHER_NAME, WATCHER_PATH};
use athanor_shelld::watcher::{MAX_HOSTS, MAX_ITEMS, MAX_ITEMS_PER_OWNER};
use common::{Bus, APP_CGROUP};
use futures_util::StreamExt;
use zbus::{Connection, Proxy};

async fn watcher(conn: &Connection) -> Proxy<'static> {
    Proxy::new(
        conn,
        WATCHER_NAME,
        WATCHER_PATH,
        "org.kde.StatusNotifierWatcher",
    )
    .await
    .expect("proxy")
}

async fn items(proxy: &Proxy<'_>) -> Vec<String> {
    proxy
        .get_property("RegisteredStatusNotifierItems")
        .await
        .expect("property")
}

#[tokio::test]
async fn an_item_registered_by_name_leaves_with_its_owner() {
    let bus = Bus::start("sni-name");
    let _daemon = bus.daemon(APP_CGROUP).await;
    let observer = watcher(&bus.client().await).await;
    let mut registered = observer
        .receive_signal("StatusNotifierItemRegistered")
        .await
        .expect("subscribe");
    let mut unregistered = observer
        .receive_signal("StatusNotifierItemUnregistered")
        .await
        .expect("subscribe");
    let app = bus.client().await;
    app.request_name("org.kde.StatusNotifierItem-7-1")
        .await
        .expect("name");
    watcher(&app)
        .await
        .call::<_, _, ()>(
            "RegisterStatusNotifierItem",
            &("org.kde.StatusNotifierItem-7-1",),
        )
        .await
        .expect("register");
    let expected = "org.kde.StatusNotifierItem-7-1/StatusNotifierItem";
    let announced: String = registered
        .next()
        .await
        .expect("signal")
        .body()
        .deserialize()
        .expect("arg");
    assert_eq!(announced, expected);
    assert_eq!(items(&observer).await, [expected]);
    drop(app);
    let gone: String = tokio::time::timeout(Duration::from_secs(5), unregistered.next())
        .await
        .expect("in time")
        .expect("signal")
        .body()
        .deserialize()
        .expect("arg");
    assert_eq!(gone, expected);
    assert!(items(&observer).await.is_empty());
}

#[tokio::test]
async fn an_item_registered_by_object_path_belongs_to_its_sender() {
    let bus = Bus::start("sni-path");
    let _daemon = bus.daemon(APP_CGROUP).await;
    let app = bus.client().await;
    let unique = app.unique_name().expect("unique").to_string();
    let proxy = watcher(&app).await;
    proxy
        .call::<_, _, ()>(
            "RegisterStatusNotifierItem",
            &("/org/ayatana/NotificationItem/nm",),
        )
        .await
        .expect("register");
    proxy
        .call::<_, _, ()>(
            "RegisterStatusNotifierItem",
            &("/org/ayatana/NotificationItem/nm",),
        )
        .await
        .expect("again: ignored");
    assert_eq!(
        items(&proxy).await,
        [format!("{unique}/org/ayatana/NotificationItem/nm")]
    );
}

#[tokio::test]
async fn names_without_owner_and_garbage_are_refused() {
    let bus = Bus::start("sni-refuse");
    let _daemon = bus.daemon(APP_CGROUP).await;
    let proxy = watcher(&bus.client().await).await;
    for service in [
        "org.kde.StatusNotifierItem-nobody-1",
        "not a name",
        "/bad//path",
    ] {
        assert!(
            proxy
                .call::<_, _, ()>("RegisterStatusNotifierItem", &(service,))
                .await
                .is_err(),
            "{service}"
        );
    }
    assert!(items(&proxy).await.is_empty());
}

#[tokio::test]
async fn the_sixty_fifth_item_is_refused() {
    let bus = Bus::start("sni-cap");
    let _daemon = bus.daemon(APP_CGROUP).await;
    // Path-form items are owned by the connection that registers them (MAX_ITEMS_PER_OWNER),
    // so filling the global cap needs enough connections that no single one is refused first —
    // and each must stay open for the rest of the test, or its items leave with it.
    assert_eq!(
        MAX_ITEMS % MAX_ITEMS_PER_OWNER,
        0,
        "the split below assumes this"
    );
    let mut owners = Vec::new();
    let mut n = 0;
    for _ in 0..MAX_ITEMS / MAX_ITEMS_PER_OWNER {
        let proxy = watcher(&bus.client().await).await;
        for _ in 0..MAX_ITEMS_PER_OWNER {
            proxy
                .call::<_, _, ()>("RegisterStatusNotifierItem", &(format!("/item/{n}"),))
                .await
                .expect("register");
            n += 1;
        }
        owners.push(proxy);
    }
    let proxy = watcher(&bus.client().await).await;
    let err = proxy
        .call::<_, _, ()>("RegisterStatusNotifierItem", &("/item/extra",))
        .await
        .expect_err("full");
    assert!(err.to_string().contains("LimitsExceeded"), "{err}");
}

#[tokio::test]
async fn a_ninth_item_from_the_same_owner_is_refused() {
    let bus = Bus::start("sni-owner-cap");
    let _daemon = bus.daemon(APP_CGROUP).await;
    let proxy = watcher(&bus.client().await).await;
    for n in 0..MAX_ITEMS_PER_OWNER {
        proxy
            .call::<_, _, ()>("RegisterStatusNotifierItem", &(format!("/item/{n}"),))
            .await
            .expect("register");
    }
    let err = proxy
        .call::<_, _, ()>("RegisterStatusNotifierItem", &("/item/extra",))
        .await
        .expect_err("owner full");
    assert!(err.to_string().contains("LimitsExceeded"), "{err}");
}

#[tokio::test]
async fn the_host_is_announced_and_leaves_with_its_owner() {
    let bus = Bus::start("sni-host");
    let _daemon = bus.daemon(APP_CGROUP).await;
    let observer = watcher(&bus.client().await).await;
    let mut gone = observer
        .receive_signal("StatusNotifierHostUnregistered")
        .await
        .expect("subscribe");
    let host = bus.client().await;
    host.request_name("org.kde.StatusNotifierHost-9")
        .await
        .expect("name");
    watcher(&host)
        .await
        .call::<_, _, ()>(
            "RegisterStatusNotifierHost",
            &("org.kde.StatusNotifierHost-9",),
        )
        .await
        .expect("register");
    assert!(observer
        .get_property::<bool>("IsStatusNotifierHostRegistered")
        .await
        .expect("property"));
    assert_eq!(
        observer
            .get_property::<i32>("ProtocolVersion")
            .await
            .expect("property"),
        0
    );
    drop(host);
    tokio::time::timeout(Duration::from_secs(5), gone.next())
        .await
        .expect("in time")
        .expect("signal");
    assert!(!observer
        .get_property::<bool>("IsStatusNotifierHostRegistered")
        .await
        .expect("property"));
}

#[tokio::test]
async fn an_item_cannot_be_registered_under_a_name_the_caller_does_not_own() {
    let bus = Bus::start("sni-item-impersonate");
    let _daemon = bus.daemon(APP_CGROUP).await;
    let owner = bus.client().await;
    owner
        .request_name("org.kde.StatusNotifierItem-owned-1")
        .await
        .expect("name");
    let attacker = bus.client().await;
    let err = watcher(&attacker)
        .await
        .call::<_, _, ()>(
            "RegisterStatusNotifierItem",
            &("org.kde.StatusNotifierItem-owned-1",),
        )
        .await
        .expect_err("not the owner");
    assert!(err.to_string().contains("AccessDenied"), "{err}");
    let observer = watcher(&bus.client().await).await;
    assert!(items(&observer).await.is_empty());
}

#[tokio::test]
async fn a_host_cannot_be_registered_under_a_name_the_caller_does_not_own() {
    let bus = Bus::start("sni-host-impersonate");
    let _daemon = bus.daemon(APP_CGROUP).await;
    let owner = bus.client().await;
    owner
        .request_name("org.kde.StatusNotifierHost-owned")
        .await
        .expect("name");
    let attacker = bus.client().await;
    let err = watcher(&attacker)
        .await
        .call::<_, _, ()>(
            "RegisterStatusNotifierHost",
            &("org.kde.StatusNotifierHost-owned",),
        )
        .await
        .expect_err("not the owner");
    assert!(err.to_string().contains("AccessDenied"), "{err}");
    let observer = watcher(&bus.client().await).await;
    assert!(!observer
        .get_property::<bool>("IsStatusNotifierHostRegistered")
        .await
        .expect("property"));
}

#[tokio::test]
async fn an_owner_may_register_its_own_well_known_name() {
    let bus = Bus::start("sni-owned");
    let _daemon = bus.daemon(APP_CGROUP).await;
    let app = bus.client().await;
    app.request_name("org.kde.StatusNotifierItem-owner-2")
        .await
        .expect("item name");
    app.request_name("org.kde.StatusNotifierHost-owner-2")
        .await
        .expect("host name");
    let proxy = watcher(&app).await;
    proxy
        .call::<_, _, ()>(
            "RegisterStatusNotifierItem",
            &("org.kde.StatusNotifierItem-owner-2",),
        )
        .await
        .expect("register item");
    proxy
        .call::<_, _, ()>(
            "RegisterStatusNotifierHost",
            &("org.kde.StatusNotifierHost-owner-2",),
        )
        .await
        .expect("register host");
    assert_eq!(
        items(&proxy).await,
        ["org.kde.StatusNotifierItem-owner-2/StatusNotifierItem"]
    );
    assert!(proxy
        .get_property::<bool>("IsStatusNotifierHostRegistered")
        .await
        .expect("property"));
}

#[tokio::test]
async fn a_fifth_distinct_host_is_refused() {
    let bus = Bus::start("sni-host-cap");
    let _daemon = bus.daemon(APP_CGROUP).await;
    let mut hosts = Vec::new();
    for n in 0..MAX_HOSTS {
        let host = bus.client().await;
        let name = format!("org.kde.StatusNotifierHost-{n}");
        host.request_name(name.as_str()).await.expect("name");
        watcher(&host)
            .await
            .call::<_, _, ()>("RegisterStatusNotifierHost", &(name.as_str(),))
            .await
            .expect("register");
        hosts.push(host);
    }
    let extra = bus.client().await;
    extra
        .request_name("org.kde.StatusNotifierHost-extra")
        .await
        .expect("name");
    let err = watcher(&extra)
        .await
        .call::<_, _, ()>(
            "RegisterStatusNotifierHost",
            &("org.kde.StatusNotifierHost-extra",),
        )
        .await
        .expect_err("full");
    assert!(err.to_string().contains("LimitsExceeded"), "{err}");
    drop(hosts);
}
