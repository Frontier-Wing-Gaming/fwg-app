#!/bin/sh
# Runs the desktop end-to-end test in a throwaway Linux session.
#
#   e2e/run_linux.sh x11|sway|gnome|kde <path to fwg built with --features e2e> <output dir>
#
# ("wayland" is accepted as the old name for sway.) Starts a private D-Bus session with an
# unlocked GNOME Keyring, the AT-SPI accessibility bus (switched on, as a screen reader would),
# a 600x940 display, and the file chooser the desktop really uses, then runs e2e/driver.py:
#
#   x11    Xvfb, no window manager        file chooser: fake_portal.py answers by itself
#   sway   headless sway                  xdg-desktop-portal-gtk (GTK 3 dialog)
#   gnome  headless GNOME Shell (mutter)  xdg-desktop-portal-gnome (GTK 4 dialog)
#   kde    headless KWin                  xdg-desktop-portal-kde (Qt dialog)
#
# The real dialogs are driven over AT-SPI (e2e/dialog.py). FWG_PORTAL=fake|gtk|gnome|kde picks
# another one. Needs (Ubuntu 24.04 packages): dbus-x11 gnome-keyring at-spi2-core python3-pyatspi
# python3-dbus python3-gi python3-pil xdg-desktop-portal, and per session
#   x11:   xvfb xdotool xclip imagemagick
#   sway:  sway grim wtype wl-clipboard xdg-desktop-portal-gtk, e2e/wlpointer ($FWG_WLPOINTER)
#   gnome: gnome-shell xdg-desktop-portal-gnome pipewire xclip
#   kde:   kwin-wayland xdg-desktop-portal-kde qtwayland5 wl-clipboard binutils, e2e/wlpointer
set -eu

session=$1
[ "$session" = wayland ] && session=sway
app=$(realpath "$2")
out=$(realpath -m "$3")
here=$(dirname "$(realpath "$0")")
# The distribution's Python: pyatspi and python3-dbus are installed for it, not for pip ones.
PY=${PYTHON:-/usr/bin/python3}
W=600 H=940
mkdir -p "$out"

case "$session" in
  x11) default_portal=fake ;; sway) default_portal=gtk ;; gnome) default_portal=gnome ;;
  kde) default_portal=kde ;;
  *) echo "usage: $0 x11|sway|gnome|kde <app> <out>"; exit 2 ;;
esac
export FWG_PORTAL=${FWG_PORTAL:-$default_portal}

export XDG_RUNTIME_DIR=$(mktemp -d)
chmod 700 "$XDG_RUNTIME_DIR"
# A private config dir, so the portal configuration below is the only one.
export XDG_CONFIG_HOME="$XDG_RUNTIME_DIR/config"
mkdir -p "$XDG_CONFIG_HOME/xdg-desktop-portal"
export FWG_PORTAL_LOG="$out/portal.log"
export FWG_WLPOINTER=${FWG_WLPOINTER:-$here/wlpointer/target/release/wlpointer}
: > "$FWG_PORTAL_LOG"
pids=""
cleanup() { for p in $pids; do kill "$p" 2>/dev/null || true; done; [ -n "${DBUS_SESSION_BUS_PID:-}" ] && kill "$DBUS_SESSION_BUS_PID" 2>/dev/null; true; }
trap cleanup EXIT

eval "$(dbus-launch --sh-syntax)"
printf 'e2e' | gnome-keyring-daemon --unlock --components=secrets > /dev/null

wait_for_socket() {
  for _ in $(seq 100); do [ -S "$XDG_RUNTIME_DIR/$1" ] && return 0; sleep 0.2; done
  echo "$2 did not start:"; cat "$out/$2.log"; exit 1
}

unset DISPLAY WAYLAND_DISPLAY
case "$session" in
  x11)
    Xvfb :99 -screen 0 ${W}x${H}x24 > "$out/xvfb.log" 2>&1 &
    pids="$pids $!"
    export DISPLAY=:99
    sleep 2
    ;;
  sway)
    export XDG_CURRENT_DESKTOP=sway
    cat > "$XDG_RUNTIME_DIR/sway.conf" <<EOF
output HEADLESS-1 resolution ${W}x${H}
default_border none
xwayland disable
EOF
    WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_RENDERER=pixman \
      sway -c "$XDG_RUNTIME_DIR/sway.conf" > "$out/sway.log" 2>&1 &
    pids="$pids $!"
    wait_for_socket wayland-1 sway
    for _ in $(seq 50); do
      sock=$(ls "$XDG_RUNTIME_DIR"/sway-ipc.*.sock 2>/dev/null | head -1 || true)
      [ -n "$sock" ] && break
      sleep 0.2
    done
    export SWAYSOCK=$sock WAYLAND_DISPLAY=wayland-1
    # Headless sway has no keyboard; this one stays connected so the app is already attached to a
    # keyboard when the test types (each wtype call adds its own short-lived one).
    wtype -s 36000000 &
    pids="$pids $!"
    ;;
  gnome)
    export XDG_CURRENT_DESKTOP=GNOME XDG_SESSION_TYPE=wayland
    # Xwayland (for the clipboard, see backend_linux.py) needs this; it is normally made at boot.
    [ -d /tmp/.X11-unix ] || mkdir -m 1777 /tmp/.X11-unix
    # Mutter takes absolute pointer positions only against a screen cast, which needs PipeWire.
    pipewire > "$out/pipewire.log" 2>&1 &
    pids="$pids $!"
    gnome-shell --headless --wayland --virtual-monitor ${W}x${H} --wayland-display wayland-fwg \
      > "$out/gnome.log" 2>&1 &
    pids="$pids $!"
    wait_for_socket wayland-fwg gnome
    export WAYLAND_DISPLAY=wayland-fwg
    for _ in $(seq 50); do
      dbus-send --session --print-reply --dest=org.gnome.Mutter.RemoteDesktop \
        /org/gnome/Mutter/RemoteDesktop org.freedesktop.DBus.Peer.Ping > /dev/null 2>&1 && break
      sleep 0.2
    done
    export FWG_XWAYLAND_DISPLAY=$(sed -n 's/.*Using public X11 display \(:[0-9]*\).*/\1/p' "$out/gnome.log" | head -1)
    export FWG_XWAYLAND_AUTH=$(ls "$XDG_RUNTIME_DIR"/.mutter-Xwaylandauth.* 2>/dev/null | head -1 || true)
    # As in a real GNOME session: the app's clipboard library needs Xwayland on GNOME, since
    # mutter has no Wayland data-control protocol.
    export DISPLAY=$FWG_XWAYLAND_DISPLAY XAUTHORITY=$FWG_XWAYLAND_AUTH
    echo "Xwayland: DISPLAY=$DISPLAY XAUTHORITY=$XAUTHORITY"
    ;;
  kde)
    export XDG_CURRENT_DESKTOP=KDE XDG_SESSION_TYPE=wayland KDE_FULL_SESSION=true QT_QPA_PLATFORM=wayland
    # Lets wlpointer use fake input and the backend take screenshots without a .desktop grant.
    KWIN_WAYLAND_NO_PERMISSION_CHECKS=1 KWIN_SCREENSHOT_NO_PERMISSION_CHECKS=1 kwin_wayland --virtual --width $W --height $H \
      --no-lockscreen --socket wayland-fwg > "$out/kwin.log" 2>&1 &
    pids="$pids $!"
    wait_for_socket wayland-fwg kwin
    export WAYLAND_DISPLAY=wayland-fwg
    # KWin reads a client's whole executable when it connects (stalling every other client
    # meanwhile), and the app opens several connections: a 500 MB debug build never gets a window.
    if command -v strip > /dev/null; then
      strip --strip-debug -o "$XDG_RUNTIME_DIR/fwg" "$app" && app="$XDG_RUNTIME_DIR/fwg"
    fi
    ;;
esac

# The file chooser.
if [ "$FWG_PORTAL" = fake ]; then
  $PY "$here/fake_portal.py" "$out/fixture.jpg" "$FWG_PORTAL_LOG" > "$out/portal.out" 2>&1 &
  pids="$pids $!"
else
  printf '[preferred]\ndefault=%s\n' "$FWG_PORTAL" > "$XDG_CONFIG_HOME/xdg-desktop-portal/portals.conf"
  # D-Bus starts xdg-desktop-portal and the backend on first use, with this environment.
  dbus-update-activation-environment WAYLAND_DISPLAY XDG_CURRENT_DESKTOP XDG_SESSION_TYPE \
    XDG_RUNTIME_DIR XDG_CONFIG_HOME ${KDE_FULL_SESSION:+KDE_FULL_SESSION} ${QT_QPA_PLATFORM:+QT_QPA_PLATFORM}
  if [ "$FWG_PORTAL" = kde ]; then
    # Qt 5 only joins the accessibility bus if accessibility is switched on after it started
    # (when it's already on, Qt registers before it has connected and never retries).
    portal_kde=$(sed -n 's/^Exec=//p' /usr/share/dbus-1/services/org.freedesktop.impl.portal.desktop.kde.service)
    $portal_kde > "$out/portal-kde.log" 2>&1 &
    pids="$pids $!"
    for _ in $(seq 60); do
      dbus-send --session --print-reply --dest=org.freedesktop.impl.portal.desktop.kde \
        /org/freedesktop/portal/desktop org.freedesktop.DBus.Peer.Ping > /dev/null 2>&1 && break
      sleep 0.25
    done
  fi
fi

# Accessibility: start the AT-SPI bus and turn it on, which is what makes accesskit publish the
# tree; then wake the registry, which apps register with.
dbus-send --session --print-reply --dest=org.a11y.Bus /org/a11y/bus \
  org.freedesktop.DBus.Properties.Set string:org.a11y.Status string:IsEnabled variant:boolean:true > /dev/null
$PY -c "import pyatspi; pyatspi.Registry.getDesktop(0).childCount"

$PY "$here/driver.py" --app "$app" --out "$out" --session "$session"
