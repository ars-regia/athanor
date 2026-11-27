//! Who the frontend is (doc_portal.md PT3): the owner of `org.freedesktop.portal.Desktop` that
//! is the main process of the image's `xdg-desktop-portal.service`, as
//! [`athanor_unit::bus_caller::admit`] checks it: through the pidfd the bus returns, the
//! verified user systemd places the caller in exactly that unit, whose main process it is,
//! whose one `ExecStart` runs `/usr/libexec/xdg-desktop-portal`, and whose `FragmentPath` and
//! drop-ins lie beneath `/usr/lib/systemd/user`. A process of the user that took the name, or
//! that runs the frontend from a unit it defined or overrode under its own configuration, is
//! refused (issue #1560).

use athanor_unit::bus_caller::{self, Questions, Service};

/// The frontend's well-known name.
pub const NAME: &str = "org.freedesktop.portal.Desktop";

/// The frontend's unit, as the image ships it, and the executable of its one `ExecStart`.
pub const FRONTEND: Service<'static> = Service {
    unit: "xdg-desktop-portal.service",
    exec: "/usr/libexec/xdg-desktop-portal",
};

/// `Ok` only when `sender`, while `owner` owns [`NAME`], is the main process of [`FRONTEND`];
/// the error says which check refused it. The caller bounds the whole check by a timeout.
pub async fn check(questions: &impl Questions, sender: &str, owner: &str) -> Result<(), String> {
    if sender != owner {
        return Err(format!("{sender} does not own {NAME}"));
    }
    bus_caller::admit(questions, sender, FRONTEND).await
}

#[cfg(test)]
mod tests {
    use zbus::Connection;

    use super::*;
    use crate::eis::tests::Fixture;
    use crate::testing::Bus;
    use athanor_unit::bus_caller::IMAGE_USER_UNITS;

    /// The user's configuration directory of units, where a process of the user may define or
    /// override any unit.
    const USER_UNITS: &str = "/home/user/.config/systemd/user";

    /// The frontend's connection on a private bus, and the fixture's answers for the image's
    /// `xdg-desktop-portal.service`, with the drop-in every user service of Fedora has.
    async fn frontend(bus: &Bus) -> (Connection, Fixture) {
        let frontend = bus.builder().name(NAME).build().await.expect("frontend");
        let mut fixture = Fixture::new(&frontend, &frontend).await;
        fixture.unit = FRONTEND.unit.into();
        fixture.exec = FRONTEND.exec.into();
        fixture.fragment = format!("{IMAGE_USER_UNITS}/{}", FRONTEND.unit);
        fixture.drop_ins = vec![format!(
            "{IMAGE_USER_UNITS}/service.d/10-timeout-abort.conf"
        )];
        (frontend, fixture)
    }

    fn name(conn: &Connection) -> String {
        conn.unique_name().expect("name").to_string()
    }

    #[tokio::test]
    async fn the_image_s_frontend_is_admitted() {
        let bus = Bus::start("frontend-image");
        let (frontend, fixture) = frontend(&bus).await;
        let sender = name(&frontend);
        check(&fixture, &sender, &sender).await.expect("admitted");
    }

    #[tokio::test]
    async fn a_caller_that_does_not_own_the_name_is_refused() {
        let bus = Bus::start("frontend-stranger");
        let (frontend, fixture) = frontend(&bus).await;
        let stranger = bus.builder().build().await.expect("stranger");
        let why = check(&fixture, &name(&stranger), &name(&frontend))
            .await
            .expect_err("refused");
        assert!(why.contains("does not own"), "{why}");
    }

    #[tokio::test]
    async fn a_frontend_defined_under_the_user_s_configuration_is_refused() {
        let bus = Bus::start("frontend-defined");
        let (frontend, mut fixture) = frontend(&bus).await;
        fixture.fragment = format!("{}/{}", USER_UNITS, FRONTEND.unit);
        let sender = name(&frontend);
        let why = check(&fixture, &sender, &sender)
            .await
            .expect_err("refused");
        assert!(why.contains("is defined by"), "{why}");
    }

    #[tokio::test]
    async fn a_frontend_overridden_under_the_user_s_configuration_is_refused() {
        let bus = Bus::start("frontend-overridden");
        let (frontend, mut fixture) = frontend(&bus).await;
        let drop_in = format!("{}/{}.d/override.conf", USER_UNITS, FRONTEND.unit);
        fixture.drop_ins.push(drop_in);
        let sender = name(&frontend);
        let why = check(&fixture, &sender, &sender)
            .await
            .expect_err("refused");
        assert!(why.contains("has the drop-in"), "{why}");
    }

    #[tokio::test]
    async fn another_unit_or_executable_is_refused() {
        let bus = Bus::start("frontend-other");
        let (frontend, fixture) = frontend(&bus).await;
        let sender = name(&frontend);
        let mut other_unit = Fixture::new(&frontend, &frontend).await;
        other_unit.unit = "evil.service".into();
        other_unit.fragment = fixture.fragment.clone();
        let mut other_exec = Fixture::new(&frontend, &frontend).await;
        other_exec.unit = FRONTEND.unit.into();
        other_exec.exec = "/usr/bin/sh".into();
        other_exec.fragment = fixture.fragment.clone();
        for refused in [other_unit, other_exec] {
            assert!(check(&refused, &sender, &sender).await.is_err());
        }
    }
}
