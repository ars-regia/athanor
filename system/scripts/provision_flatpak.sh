#!/bin/bash
set -euo pipefail

if [ "${0}" != "/usr/libexec/athanor-flatpak-provisioner" ]; then
    mkdir -p /usr/libexec
fi

MANIFEST="/usr/share/athanor/packages.json"
if [ ! -f "$MANIFEST" ]; then
    MANIFEST="/etc/athanor/packages.json"
fi

if [ ! -f "$MANIFEST" ]; then
    echo "[Athanor Flatpak] No package manifest found at $MANIFEST, skipping."
    exit 0
fi

if ! command -v flatpak &>/dev/null; then
    echo "[Athanor Flatpak] Flatpak binary not found, skipping."
    exit 0
fi

echo "[Athanor Flatpak] Configuring Flathub remote..."
flatpak remote-add --if-not-exists --system flathub https://dl.flathub.org/repo/flathub.flatpakrepo

FLATPAKS=$(jq -r '.flatpaks[]?' "$MANIFEST")

if [ -z "$FLATPAKS" ]; then
    echo "[Athanor Flatpak] No flatpaks configured in manifest."
    exit 0
fi

for app in $FLATPAKS; do
    echo "[Athanor Flatpak] Provisioning $app..."
    if ! flatpak info "$app" &>/dev/null; then
        flatpak install --system -y --noninteractive flathub "$app" || {
            echo "[Athanor Flatpak] CRITICAL ERROR: Failed to install $app"
            exit 1
        }
    else
        echo "[Athanor Flatpak] $app is already installed."
    fi
done


echo "[Athanor Flatpak] Enforcing Zero-Trust Global Overrides (Martial Law)..."
# 1. Distruzione totale del protocollo legacy X11 (Previene i keylogger globali)
flatpak override --system --nosocket=x11
flatpak override --system --nosocket=fallback-x11

# 2. Isolamento del Filesystem (Forza le app a usare gli XDG Portals per leggere/scrivere file)
flatpak override --system --nofilesystem=host
flatpak override --system --nofilesystem=home

# 3. Forzatura del Backend Nativo Wayland
flatpak override --system --env=GDK_BACKEND=wayland
flatpak override --system --env=QT_QPA_PLATFORM=wayland

# 4. Prevenzione dell'evasione Flatpak-in-Flatpak
flatpak override --system --no-talk-name=org.freedesktop.Flatpak

echo "[Athanor Flatpak] Provisioning Completato. Sandbox sigillata."
