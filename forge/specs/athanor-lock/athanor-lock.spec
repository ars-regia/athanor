%global debug_package %{nil}
Name:           athanor-lock
Version:        1.0.0
Release:        1%{?athanor_content}%{?dist}
Summary:        The Athanor session lock
License:        GPL-3.0-or-later

BuildRequires:  rust cargo gcc

%description
The session lock of doc_lock_and_prompts.md (LP2), written in Athanor's own toolkit, Sal, and
run as a guard of the session that restarts without limit (LP10). This release carries the
crate, the user unit and the package of LP18 step 2 alone: the program locks nothing yet, says
so and exits with a failure. The unit is installed and not enabled; LP18 step 3 adds its preset.

%prep

%build
%set_build_flags
cargo build --release --locked -p %{name}

%install
install -D -m 0755 target/release/athanor-lock %{buildroot}/usr/bin/athanor-lock
install -D -m 0644 forge/specs/athanor-lock/athanor-lock-1.0.0/data/athanor-lock.service \
    %{buildroot}/usr/lib/systemd/user/athanor-lock.service

%files
/usr/bin/athanor-lock
/usr/lib/systemd/user/athanor-lock.service

%changelog
* Thu Nov 19 2026 Athanor Forge <forge@athanor.os> - 1.0.0-1
- First release (doc_lock_and_prompts.md, LP18 step 2, task 1): the crate, the guard user unit
  with the confinement of LP15, and the package. The program locks nothing yet and exits; no
  preset enables the unit.
