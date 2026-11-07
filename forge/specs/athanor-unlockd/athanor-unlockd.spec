%global debug_package %{nil}
%global sources forge/specs/%{name}/SOURCES
Name:           athanor-unlockd
Version:        1.0.0
Release:        1%{?dist}
Summary:        The session lock's PAM helper, run as root once per connection
License:        GPL-3.0-or-later

BuildRequires:  rust cargo gcc pkgconf-pkg-config pam-devel systemd-rpm-macros
Requires:       pam systemd fprintd-pam

%description
Runs the PAM transaction of the session lock (doc_lock_and_prompts.md, LP8), so that
pam_faillock can write its root-owned tally while the lock stays confined. systemd starts
one helper per connection on /run/athanor/unlockd.socket; the helper admits only the
uid's athanor-lock.service of an active Wayland session, runs the athanor-lock or
athanor-lock-fingerprint stack the client names, and answers SUCCESS or FAILURE in
polkit-agent-helper-1's line protocol. It never sets credentials or opens a session.

%prep

%build
%set_build_flags
cargo build --release --locked -p %{name}

%install
install -D -m 0755 target/release/athanor-unlockd %{buildroot}/usr/libexec/athanor-unlockd
install -D -m 0644 %{sources}/usr/lib/systemd/system/athanor-unlockd.socket \
    %{buildroot}/usr/lib/systemd/system/athanor-unlockd.socket
install -D -m 0644 %{sources}/usr/lib/systemd/system/athanor-unlockd@.service \
    %{buildroot}/usr/lib/systemd/system/athanor-unlockd@.service
install -D -m 0644 %{sources}/usr/lib/pam.d/athanor-lock %{buildroot}/usr/lib/pam.d/athanor-lock
install -D -m 0644 %{sources}/usr/lib/pam.d/athanor-lock-fingerprint \
    %{buildroot}/usr/lib/pam.d/athanor-lock-fingerprint

%post
%systemd_post athanor-unlockd.socket

%preun
%systemd_preun athanor-unlockd.socket

%postun
%systemd_postun athanor-unlockd.socket

%files
/usr/libexec/athanor-unlockd
/usr/lib/systemd/system/athanor-unlockd.socket
/usr/lib/systemd/system/athanor-unlockd@.service
/usr/lib/pam.d/athanor-lock
/usr/lib/pam.d/athanor-lock-fingerprint

%changelog
* Fri Nov 06 2026 Athanor Forge <forge@athanor.os> - 1.0.0-1
- First release (doc_lock_and_prompts.md, LP8, LP9): the PAM helper of the session lock,
  its socket and per-connection unit in polkit 128's helper sandbox, and the athanor-lock
  and athanor-lock-fingerprint stacks. No preset enables the socket yet.
