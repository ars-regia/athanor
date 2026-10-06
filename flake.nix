{
  description = "Athanor OS - Singularity Level 5 (Nix Hermetic Build Factory)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };
        rust-toolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" ];
          targets = [ "x86_64-unknown-linux-gnu" ];
        };
        
        security-tools = with pkgs; [ syft cosign ];
        # libclang: bindgen (libspa-sys di niri, aya) carica libclang.so da LIBCLANG_PATH.
        c-toolchain = with pkgs; [ gcc gnumake cmake mold llvmPackages_latest.llvm llvmPackages_latest.clang llvmPackages_latest.lld llvmPackages_latest.libclang ccache bpf-linker pahole elfutils ];
        rust-tools = with pkgs; [ rust-toolchain sccache clippy rustfmt cargo-deny cargo-vet cargo-fuzz ];
        # Toolset POSIX di base incluso: rpmbuild, %autosetup e i Makefile upstream
        # danno per scontati grep, diff, patch, gzip, file, which, m4, gettext.
        # dbus: the %check of athanor-update runs its bus tests on a private dbus-daemon.
        build-tools = with pkgs; [ rpm cpio dbus createrepo_c buildah skopeo jq git gnutar xz curl wget rsync flex bison bc zstd checkpolicy perl pkg-config meson ninja gobject-introspection glibc.bin autoconf automake libtool util-linux gnugrep diffutils patch which file gzip bzip2 unzip m4 gettext python3 go ];
        system-deps = with pkgs; [ zlib openssl policycoreutils spdlog systemd nodejs_22 nlohmann_json fmt speechd gnupg ipxe ncurses iproute2 fio gtk4 pango cairo gtk4-layer-shell glib libpulseaudio pkg-config ];
        # Header e .pc delle librerie di sistema, cioè i -devel di Fedora: in nixpkgs
        # stanno nei dev output. closePropagation segue propagatedBuildInputs come fa
        # stdenv, così le catene Requires dei .pc (gtk4 → pango → harfbuzz → …) sono
        # complete e pkg-config le risolve da /lib/pkgconfig (PKG_CONFIG_PATH in Env).
        # libdisplay-info alla versione della rootfs (Fedora 43: 0.2.0, soname .so.2):
        # nixpkgs ha la 0.4.0, che il crate libdisplay-info-sys 0.3 di niri rifiuta
        # (< 0.4.0) e che sulla rootfs non esisterebbe. Riusa il generic.nix di nixpkgs.
        libdisplay-info-target = pkgs.callPackage
          (import "${pkgs.path}/pkgs/by-name/li/libdisplay-info/generic.nix" {
            version = "0.2.0";
            hash = "sha256-6xmWBrPHghjok43eIDGeshpUEQTuwWLXNHg7CnBUt3Q=";
          }) { };
        system-libs = with pkgs; [
          zlib openssl curl spdlog systemd fmt speechd ncurses
          # polkit (forge/specs/polkit): its BuildRequires. meson, ninja, gobject-introspection and
          # glibc.bin (the ldd g-ir-scanner runs) are in build-tools.
          duktape linux-pam expat gobject-introspection dbus
          # libpulseaudio: athanor-bar audio (libpulse-binding) links libpulse-mainloop-glib.so.0 by soname
          gtk4 pango cairo graphene gdk-pixbuf gtk4-layer-shell glib libpulseaudio
          # compositors niri and cosmic-comp (smithay): wayland, pixman, gbm/drm/egl, xkbcommon, libseat, libinput, pipewire, libdisplay-info
          wayland wayland-protocols libgbm libdrm libglvnd libxkbcommon seatd libinput pipewire libdisplay-info-target pixman
          # athanor-launcher and athanor-preview-render: tinysparql (tracker-sys, the localsearch client),
          # poppler-glib (the PDF decoder), libseccomp and fontconfig (glycin)
          tinysparql poppler libseccomp fontconfig
        ];
        system-lib-closure = pkgs.lib.closePropagation (map pkgs.lib.getDev system-libs);
        system-dev = map pkgs.lib.getDev system-lib-closure;
        # Gli output con le .so (per openssl, curl, glib, pango non sono quello di default):
        # senza di loro la union /lib non ha libssl.so e un Makefile che fa `-lssl` fallisce.
        # Non lib.getLib: sugli output già specificati (openssl.dev) restituisce
        # l'output stesso, quindi la union resterebbe senza libssl.so.
        system-lib = map (d: d.lib or d.out or d) system-lib-closure;
      in
      {
        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            rust-toolchain just jq python3 pkg-config openssl
          ];
          shellHook = ''
            echo "========================================================"
            echo " ATHANOR OS: Nix Hermetic Build Environment Activated "
            echo "========================================================"
          '';
        };

        packages = rec {
          just-hermetic = pkgs.just;

          athanor-core = (pkgs.makeRustPlatform {
            cargo = rust-toolchain;
            rustc = rust-toolchain;
          }).buildRustPackage {
            pname = "athanor-core";
            version = "1.0.0";
            src = ./.;
            cargoLock = {
              lockFile = ./Cargo.lock;
            };
            nativeBuildInputs = with pkgs; [ pkg-config ];
            buildInputs = with pkgs; [ openssl glib gtk4 wayland wayland-protocols ];
            doCheck = false;
          };

          # Compatibilità FHS del builder: directory àncora e symlink verso la glibc e le
          # librerie di runtime di gcc di nixpkgs, come contenuto immutabile dell'immagine.
          # È una derivazione ordinaria, costruibile e ispezionabile da sola:
          #   nix build .#builder-fhs-compat
          builder-fhs-compat = pkgs.runCommand "athanor-builder-fhs-compat" { } ''
            mkdir -p $out/lib64 $out/lib/x86_64-linux-gnu $out/usr/lib $out/usr/lib64 $out/usr/lib/x86_64-linux-gnu

            for lib in ld-linux-x86-64.so.2 libc.so.6 libm.so.6 libpthread.so.0 libdl.so.2 librt.so.1; do
              ln -s ${pkgs.glibc}/lib/$lib $out/lib64/$lib
              ln -s ${pkgs.glibc}/lib/$lib $out/lib/x86_64-linux-gnu/$lib
            done

            for lib in libstdc++.so.6 libgcc_s.so.1; do
              ln -s ${pkgs.stdenv.cc.cc.lib}/lib/$lib $out/usr/lib64/$lib
              ln -s ${pkgs.stdenv.cc.cc.lib}/lib/$lib $out/lib/x86_64-linux-gnu/$lib
            done
            ln -s ${pkgs.stdenv.cc.cc.lib}/lib/libstdc++.so.6 $out/usr/lib/libstdc++.so.6
            ln -s ${pkgs.stdenv.cc.cc.lib}/lib/libgcc_s.so.1 $out/lib64/libgcc_s.so.1
          '';

          # containers/image policy for buildah and skopeo inside the builder: without
          # /etc/containers/policy.json every image operation is refused. nixpkgs' skopeo
          # carries the upstream default policy; it is installed under the name the
          # tools look up.
          builder-containers-policy = pkgs.runCommand "athanor-builder-containers-policy" { } ''
            install -D -m 0644 ${pkgs.skopeo.policy}/default-policy.json $out/etc/containers/policy.json
          '';

          # Macro RPM di systemd (%_unitdir, %systemd_post, …). In Fedora le fornisce
          # systemd-rpm-macros; l'rpm di nixpkgs non le ha, e rpmbuild lascerebbe
          # `%systemd_post` come testo letterale negli scriptlet. Rese dal template
          # ufficiale di systemd con i percorsi del sistema di destinazione (Fedora,
          # rootprefix /usr), non del builder: gli scriptlet girano sull'immagine finale.
          # Il file segue il layout di rpm (macros.d/) e viene fuso nella sua config dir
          # da builder-rpm-configdir, esposta con RPM_CONFIGDIR.
          builder-rpm-macros-systemd =
            let
              targetPaths = {
                LIBEXECDIR = "/usr/lib/systemd";
                SYSTEMD_UPDATE_HELPER_PATH = "/usr/lib/systemd/systemd-update-helper";
                SYSTEM_DATA_UNIT_DIR = "/usr/lib/systemd/system";
                USER_DATA_UNIT_DIR = "/usr/lib/systemd/user";
                SYSTEM_PRESET_DIR = "/usr/lib/systemd/system-preset";
                USER_PRESET_DIR = "/usr/lib/systemd/user-preset";
                SYSTEM_GENERATOR_DIR = "/usr/lib/systemd/system-generators";
                USER_GENERATOR_DIR = "/usr/lib/systemd/user-generators";
                SYSTEM_ENV_GENERATOR_DIR = "/usr/lib/systemd/system-environment-generators";
                USER_ENV_GENERATOR_DIR = "/usr/lib/systemd/user-environment-generators";
                SYSTEMD_CATALOG_DIR = "/usr/lib/systemd/catalog";
                UDEV_HWDB_DIR = "/usr/lib/udev/hwdb.d";
                UDEV_RULES_DIR = "/usr/lib/udev/rules.d";
                KERNEL_INSTALL_DIR = "/usr/lib/kernel/install.d";
                BINFMT_DIR = "/usr/lib/binfmt.d";
                SYSCTL_DIR = "/usr/lib/sysctl.d";
                SYSUSERS_DIR = "/usr/lib/sysusers.d";
                TMPFILES_DIR = "/usr/lib/tmpfiles.d";
                USER_TMPFILES_DIR = "/usr/share/user-tmpfiles.d";
                ENVIRONMENT_DIR = "/usr/lib/environment.d";
                MODULESLOAD_DIR = "/usr/lib/modules-load.d";
                MODPROBE_DIR = "/usr/lib/modprobe.d";
              };
              substitutions = pkgs.lib.concatStringsSep " "
                (pkgs.lib.mapAttrsToList (name: path: "-e 's|{{${name}}}|${path}|g'") targetPaths);
            in
            pkgs.runCommand "athanor-builder-rpm-macros-systemd" { } ''
              mkdir -p $out/macros.d
              sed ${substitutions} ${pkgs.systemd.src}/src/rpm/macros.systemd.in > $out/macros.d/macros.systemd
              if grep -q '{{' $out/macros.d/macros.systemd; then
                echo "macros.systemd: variabili del template non sostituite:" >&2
                grep -o '{{[A-Za-z_]*}}' $out/macros.d/macros.systemd | sort -u >&2
                exit 1
              fi
            '';

          # Percorsi del sistema di destinazione. L'rpm di nixpkgs è configurato con
          # prefix = il proprio store path, quindi %{_bindir}, %{_datadir}, %{_libexecdir}
          # e %{_localstatedir} finirebbero sotto /nix/store dentro i pacchetti (visto su
          # athanor-daemon-rs: dbus service e polkit policy installati lì). I pacchetti
          # sono per Fedora: prefix /usr, lib64, /var. Le macro derivate (_bindir, _libdir,
          # _mandir, …) discendono da queste nel file macros di rpm. __isa dà %{_isa} = (x86-64).
          builder-rpm-macros-target =
            let
              macros = {
                _prefix = "/usr";
                _exec_prefix = "%{_prefix}";
                _lib = "lib64";
                _localstatedir = "/var";
                _docdir = "%{_datadir}/doc";
                # Fedora's platform macros define %__isa; nixpkgs' rpm ships none, so
                # %{?_isa} expanded to nothing and rpm added no name(x86-64) provides.
                __isa = "x86-64";
              };
              body = pkgs.lib.concatStringsSep "
"
                (pkgs.lib.mapAttrsToList (name: value: "%${name} ${value}") macros);
            in
            pkgs.writeTextDir "macros.d/macros.athanor-target" (body + "
");

          # Config dir di rpm del builder: quella di nixpkgs più le macro di systemd e
          # dei percorsi di destinazione. rpm la trova tramite RPM_CONFIGDIR (config.Env
          # dell'immagine); l'rpm di nixpkgs non consulta /etc/rpm.
          builder-rpm-configdir = pkgs.symlinkJoin {
            name = "athanor-builder-rpm-configdir";
            paths = [ "${pkgs.rpm}/lib/rpm" builder-rpm-macros-systemd builder-rpm-macros-target ];
          };

          builderImage = pkgs.dockerTools.buildLayeredImage {
            name = "ghcr.io/hr-mes/athanor-builder";
            tag = "latest";
            contents = [ builder-fhs-compat builder-containers-policy pkgs.dockerTools.fakeNss pkgs.bashInteractive pkgs.coreutils pkgs.findutils pkgs.gnused pkgs.gawk pkgs.cacert pkgs.tzdata pkgs.shadow ] ++ security-tools ++ c-toolchain ++ rust-tools ++ build-tools ++ system-deps ++ system-dev ++ system-lib;
            config = {
              Cmd = [ "/bin/bash" ];
              Env = [
                "PATH=/bin:/usr/bin"
                "HOME=/root"
                # Bundle CA di pkgs.cacert: senza questa variabile curl, cargo e git
                # di nixpkgs non verificano alcun certificato TLS (idioma dockerTools).
                "SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
                "RPM_CONFIGDIR=${builder-rpm-configdir}"
                # I .pc dei dev output confluiscono qui dal symlinkJoin di contents;
                # il pkg-config di nixpkgs da solo guarda soltanto nel proprio prefisso.
                "PKG_CONFIG_PATH=/lib/pkgconfig:/share/pkgconfig"
                # Same union for g-ir-scanner: the .gir files of glib and friends (polkit).
                "XDG_DATA_DIRS=/share"
                # Makefile e cmake senza pkg-config (git, ananicy-cpp): header e librerie
                # dalle union /include e /lib, che i wrapper gcc/clang di nixpkgs onorano.
                # Gli header di glibc restano fuori da /include: su CPATH scavalcherebbero
                # gli stdint.h/stddef.h integrati di clang (ridefinizione di __INT64_C).
                "CPATH=/include"
                "LIBRARY_PATH=/lib"
                # Senza RUNPATH (sotto) i binari appena compilati trovano le librerie
                # della union solo così, per esempio nei %check o nei build script.
                "LD_LIBRARY_PATH=/lib"
                # bindgen (libspa-sys di niri, aya) usa libclang direttamente, senza il
                # wrapper: gli servono libclang.so e gli header di glibc, dopo quelli di clang.
                "LIBCLANG_PATH=${pkgs.llvmPackages_latest.libclang.lib}/lib"
                "BINDGEN_EXTRA_CLANG_ARGS=-idirafter ${pkgs.glibc.dev}/include"
                # I wrapper gcc/clang di nixpkgs aggiungono un DT_RUNPATH verso /nix/store per
                # ogni -L (glibc e libgcc compresi): nei pacchetti per Fedora è spazzatura.
                # La variabile con suffisso di target è quella letta da ld-wrapper.sh.
                "NIX_DONT_SET_RPATH_x86_64_unknown_linux_gnu=1"
                # Stesso wrapper: senza questa variabile l'interprete ELF (PT_INTERP) di ogni
                # binario è /nix/store/…-glibc/lib/ld-linux-x86-64.so.2, che sulla rootfs non
                # esiste. Il percorso FHS vale anche nel builder (symlink di builder-fhs-compat).
                "NIX_DYNAMIC_LINKER_x86_64_unknown_linux_gnu=/lib64/ld-linux-x86-64.so.2"
                "CC=clang"
                "CXX=clang++"
                "LD=ld.lld"
                "LLVM=1"
                "LLVM_IAS=1"
              ];
            };
            # Directory mutabili dell'immagine. nixpkgs esegue extraCommands nella radice del
            # layer di personalizzazione con percorsi relativi (manuale: `mkdir -m 1777 tmp`).
            # /root serve a rpmbuild (HOME=/root); /tmp e /var/tmp (%_tmppath) non esistono
            # in un'immagine buildLayeredImage se nessuno li crea.
            extraCommands = ''
              mkdir -m 0700 -p root
              mkdir -m 1777 -p tmp var/tmp
            '';
          };
        };
      }
    );
}
