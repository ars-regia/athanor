%global debug_package %{nil}
%global crate_dir forge/specs/%{name}/%{name}-%{version}
%global notify_dir forge/specs/%{name}/%{name}-notify-%{version}
%global sources forge/specs/%{name}/SOURCES
Name:           athanor-update
Version:        1.0.0
Release:        9%{?dist}
Summary:        Athanor system image updates and trust state

License:        GPL-3.0-or-later
URL:            https://github.com/ars-regia/athanor

BuildRequires:  rust cargo gcc systemd-rpm-macros
BuildRequires:  dbus-daemon
Requires:       bootc skopeo ostree systemd polkit containers-common

%description
Checks for, verifies, downloads and applies Athanor system image updates, never without
the user's confirmation, and publishes the trust state the greeter, the shield and the
notifier read (docs/architecture/doc_update_trust.md). Ships the root binary
athanor-update with its timer, services, D-Bus and polkit policy, the user notifier
athanor-update-notify, and the templates and renderer of the container signature policy.

%prep
# Built in place from the workspace checkout: nothing to unpack.

%build
%set_build_flags
cargo build --release --locked -p %{name} -p %{name}-notify

%check
cargo test --release --locked -p athanor-trust-state -p %{name} -p %{name}-notify
python3 -B -m unittest discover -s forge/specs/%{name}/tests

%install
install -D -m 0755 target/release/athanor-update %{buildroot}/usr/bin/athanor-update
install -D -m 0755 target/release/athanor-update-notify %{buildroot}/usr/bin/athanor-update-notify
install -D -m 0755 %{sources}/usr/libexec/athanor-update/render-policy %{buildroot}/usr/libexec/athanor-update/render-policy
for template in policy.json.in attachments-policy.json.in athanor.yaml.in; do
    install -D -m 0644 %{sources}/usr/share/athanor/containers/templates/$template %{buildroot}/usr/share/athanor/containers/templates/$template
done
for unit in athanor-update-check.timer athanor-update-check.service athanor-update.service athanor-update-state.service athanor-update-migrate.service athanor-update-migrate.timer; do
    install -D -m 0644 %{sources}/usr/lib/systemd/system/$unit %{buildroot}/usr/lib/systemd/system/$unit
done
install -D -m 0644 %{sources}/usr/lib/systemd/user/athanor-update-notify.service %{buildroot}/usr/lib/systemd/user/athanor-update-notify.service
install -D -m 0644 %{sources}/usr/lib/systemd/system-preset/80-athanor-update.preset %{buildroot}/usr/lib/systemd/system-preset/80-athanor-update.preset
install -D -m 0644 %{sources}/usr/lib/systemd/user-preset/80-athanor-update.preset %{buildroot}/usr/lib/systemd/user-preset/80-athanor-update.preset
install -D -m 0644 %{sources}/usr/lib/tmpfiles.d/athanor-update.conf %{buildroot}/usr/lib/tmpfiles.d/athanor-update.conf
install -D -m 0644 %{sources}/usr/share/dbus-1/system.d/os.athanor.Update1.conf %{buildroot}/usr/share/dbus-1/system.d/os.athanor.Update1.conf
install -D -m 0644 %{sources}/usr/share/dbus-1/system-services/os.athanor.Update1.service %{buildroot}/usr/share/dbus-1/system-services/os.athanor.Update1.service
install -D -m 0644 %{sources}/usr/share/polkit-1/actions/os.athanor.update.policy %{buildroot}/usr/share/polkit-1/actions/os.athanor.update.policy
install -D -m 0644 forge/specs/%{name}/RECOVERY.md %{buildroot}/usr/share/doc/athanor-update/RECOVERY.md

%files
/usr/bin/athanor-update
/usr/bin/athanor-update-notify
/usr/libexec/athanor-update/render-policy
/usr/share/athanor/containers/templates/policy.json.in
/usr/share/athanor/containers/templates/attachments-policy.json.in
/usr/share/athanor/containers/templates/athanor.yaml.in
/usr/lib/systemd/system/athanor-update-check.timer
/usr/lib/systemd/system/athanor-update-check.service
/usr/lib/systemd/system/athanor-update.service
/usr/lib/systemd/system/athanor-update-state.service
/usr/lib/systemd/system/athanor-update-migrate.service
/usr/lib/systemd/system/athanor-update-migrate.timer
/usr/lib/systemd/user/athanor-update-notify.service
/usr/lib/systemd/system-preset/80-athanor-update.preset
/usr/lib/systemd/user-preset/80-athanor-update.preset
/usr/lib/tmpfiles.d/athanor-update.conf
/usr/share/dbus-1/system.d/os.athanor.Update1.conf
/usr/share/dbus-1/system-services/os.athanor.Update1.service
/usr/share/polkit-1/actions/os.athanor.update.policy
%doc /usr/share/doc/athanor-update/RECOVERY.md

%changelog
* Thu Oct 08 2026 Athanor Forge <forge@athanor.os> - 1.0.0-9
- `athanor-update migrate` moves a machine that follows the project's previous owner (listed
  in `/usr/share/athanor/containers/moved-from`, which render-policy writes from
  `--moved-from` at the image build), an image the policy in force no longer names, to
  the same image under the owner the policy pins, on the tag or digest it follows, with
  `bootc switch --enforce-container-sigpolicy`. The image the machine already runs supplies
  the policy, so the switch is verified even when the machine reached that image through an
  unverified reference. Any other owner, a fork included, stays `reference-out-of-scope`.
  Until the new deployment boots, the state reads `verified.reason = owner-moved`; an image
  or tag the new owner has not published (`successor-absent`) or a queued rollback reads
  `owner-moved-waiting`, and the migration timer retries.
- The migration decides from the deployments, not from the stamp alone: a switch that staged
  nothing, or not the target with the policy enforced, fails; a staged target waits for the
  restart (`restart-pending`); the stamp is written once the signed reference has booted, so
  a deployment that does not boot is switched once more, and then its digest is held. A
  stamp is ignored on an image of the previous owner, and neither athanor-update-migrate.service
  nor its timer has the stamp as a condition; with the stamp and no previous owner listed,
  the unit ends without asking bootc.
- The migration never stages a held digest: after `GoBack()` or a greenboot rollback of the
  moved deployment the machine stays on the previous owner, `move-held` is written and the
  state reads `verified.reason = owner-moved-held`, until the new owner publishes a newer
  build.
- A verified machine on a run-number tag (digits only) or a digest publishes
  `verified.reason = pinned-build`: one build, which receives no updates. Nothing moves it.
  Any other tag follows newer builds.
- An image the registry does not hold at all (`name unknown`, or ghcr.io's 403 on the pull
  token or `denied`) is a wait like a missing tag, not a failure every five minutes.

* Thu Oct 08 2026 Athanor Forge <forge@athanor.os> - 1.0.0-8
- A machine that has migrated but boots a reference that does not enforce the policy, with
  no enforcing deployment staged, publishes `verified.reason = origin-not-enforcing` instead
  of `media`: it does not verify its updates and the check asks the registry for none. The
  state is reported, not repaired; the migration does not run again.

* Wed Oct 07 2026 Athanor Forge <forge@athanor.os> - 1.0.0-7
- A queued rollback (`rollbackQueued` in `bootc status`: greenboot after a failed health
  check, or `bootc rollback`) holds the booted digest at every check and downloads nothing.
  `GoBack()` on such a boot holds and reboots without a second `bootc rollback`, which
  would have swapped back to the deployment being left. `Apply()` refuses and the migration
  waits (`rollback-queued`), since bootc discards the staged deployment on a rollback and a
  new one would replace the return.

* Wed Oct 07 2026 Athanor Forge <forge@athanor.os> - 1.0.0-6
- `athanor-update migrate`: a channel without a manifest on the registry (`:stable` is
  published later, A2-4) is a wait, not a failure. The unit exits 0, no longer restarts every
  five minutes, and the state reads `verified.reason = channel-absent`.
  athanor-update-migrate.timer retries every six hours until the stamp exists; the stamp
  clears `channel-absent`.

* Sun Oct 04 2026 Athanor Forge <forge@athanor.os> - 1.0.0-5
- `athanor-update go-back`: the console client of GoBack(), for an administrator at a text
  console (`sudo athanor-update go-back`). It calls the service the notifier calls and
  reports what the service answers; run as any other user it says to use sudo.

* Thu Oct 01 2026 Athanor Forge <forge@athanor.os> - 1.0.0-4
- A deployment bootc reports incompatible (packages layered, removed or replaced with
  rpm-ostree) is described from rpm-ostree and published as not verified, reason
  local-changes, instead of failing the check; the migration waits on it.

* Wed Sep 30 2026 Athanor Forge <forge@athanor.os> - 1.0.0-3
- UT11: a downloaded update is announced once per user, not once per session.

* Wed Sep 30 2026 Athanor Forge <forge@athanor.os> - 1.0.0-2
- os.athanor.Update1.State(): the checked trust state, open to every user without
  polkit, for sandboxed readers whose user namespace cannot see root as the file's
  owner; a missing, refused or unreadable state is a named error.

* Thu Sep 24 2026 Athanor Forge <forge@athanor.os> - 1.0.0-1
- First release: update check timer, os.athanor.Update1 with Apply and GoBack, one-time
  migration to the signed reference, trust state file, notifier, signature policy templates
