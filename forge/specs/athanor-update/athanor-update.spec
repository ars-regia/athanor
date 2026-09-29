%global debug_package %{nil}
%global crate_dir forge/specs/%{name}/%{name}-%{version}
%global notify_dir forge/specs/%{name}/%{name}-notify-%{version}
%global sources forge/specs/%{name}/SOURCES
Name:           athanor-update
Version:        1.0.0
Release:        1%{?dist}
Summary:        Athanor system image updates and trust state

License:        MIT
URL:            https://github.com/hr-mes/athanor

BuildRequires:  rust cargo gcc systemd-rpm-macros
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
for unit in athanor-update-check.timer athanor-update-check.service athanor-update.service athanor-update-state.service athanor-update-migrate.service; do
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
/usr/lib/systemd/user/athanor-update-notify.service
/usr/lib/systemd/system-preset/80-athanor-update.preset
/usr/lib/systemd/user-preset/80-athanor-update.preset
/usr/lib/tmpfiles.d/athanor-update.conf
/usr/share/dbus-1/system.d/os.athanor.Update1.conf
/usr/share/dbus-1/system-services/os.athanor.Update1.service
/usr/share/polkit-1/actions/os.athanor.update.policy
%doc /usr/share/doc/athanor-update/RECOVERY.md

%changelog
* Thu Sep 24 2026 Athanor Forge <forge@athanor.os> - 1.0.0-1
- First release: update check timer, os.athanor.Update1 with Apply and GoBack, one-time
  migration to the signed reference, trust state file, notifier, signature policy templates
