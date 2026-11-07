#!/bin/sh
# Runs the desktop end-to-end test in a throwaway Linux session.
#
#   e2e/run_linux.sh x11|wayland <path to fwg built with --features e2e> <output dir>
#
# Starts a private D-Bus session with an unlocked GNOME Keyring, the AT-SPI accessibility bus
# (switched on, as a screen reader would), the fake file chooser portal, and either Xvfb (x11) or
# a headless sway compositor (wayland), then runs e2e/driver.py. Needs: dbus-x11, gnome-keyring,
# at-spi2-core, python3-pyatspi, python3-dbus, python3-gi, python3-pil, and for x11 xvfb, xdotool,
# xclip, imagemagick; for wayland sway, grim, wtype, wl-clipboard and e2e/wlpointer (built from
# e2e/wlpointer, path in $FWG_WLPOINTER).
set -eu

session=$1
app=$(realpath "$2")
out=$(realpath -m "$3")
here=$(dirname "$(realpath "$0")")
# The distribution's Python: pyatspi and python3-dbus are installed for it, not for pip ones.
PY=${PYTHON:-/usr/bin/python3}
mkdir -p "$out"

export XDG_RUNTIME_DIR=$(mktemp -d)
chmod 700 "$XDG_RUNTIME_DIR"
export FWG_PORTAL_LOG="$out/portal.log"
: > "$FWG_PORTAL_LOG"
pids=""
cleanup() { for p in $pids; do kill "$p" 2>/dev/null || true; done; [ -n "${DBUS_SESSION_BUS_PID:-}" ] && kill "$DBUS_SESSION_BUS_PID" 2>/dev/null; true; }
trap cleanup EXIT

eval "$(dbus-launch --sh-syntax)"
printf 'e2e' | gnome-keyring-daemon --unlock --components=secrets > /dev/null

# Accessibility: start the AT-SPI bus and turn it on, which is what makes accesskit publish the tree.
dbus-send --session --print-reply --dest=org.a11y.Bus /org/a11y/bus \
  org.freedesktop.DBus.Properties.Set string:org.a11y.Status string:IsEnabled variant:boolean:true > /dev/null

$PY "$here/fake_portal.py" "$out/fixture.jpg" "$FWG_PORTAL_LOG" > "$out/portal.out" 2>&1 &
pids="$pids $!"

case "$session" in
  x11)
    Xvfb :99 -screen 0 600x940x24 > "$out/xvfb.log" 2>&1 &
    pids="$pids $!"
    export DISPLAY=:99
    unset WAYLAND_DISPLAY
    sleep 2
    ;;
  wayland)
    unset DISPLAY
    cat > "$XDG_RUNTIME_DIR/sway.conf" <<'EOF'
output HEADLESS-1 resolution 600x940
default_border none
xwayland disable
EOF
    WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_RENDERER=pixman \
      sway -c "$XDG_RUNTIME_DIR/sway.conf" > "$out/sway.log" 2>&1 &
    pids="$pids $!"
    for _ in $(seq 50); do
      sock=$(ls "$XDG_RUNTIME_DIR"/sway-ipc.*.sock 2>/dev/null | head -1 || true)
      [ -n "$sock" ] && [ -S "$XDG_RUNTIME_DIR/wayland-1" ] && break
      sleep 0.2
    done
    [ -n "$sock" ] || { echo "sway did not start:"; cat "$out/sway.log"; exit 1; }
    export SWAYSOCK=$sock WAYLAND_DISPLAY=wayland-1
    export FWG_WLPOINTER=${FWG_WLPOINTER:-$here/wlpointer/target/release/wlpointer}
    # Headless sway has no keyboard; this one stays connected so the app is already attached to a
    # keyboard when the test types (each wtype call adds its own short-lived one).
    wtype -s 36000000 &
    pids="$pids $!"
    ;;
  *) echo "usage: $0 x11|wayland <app> <out>"; exit 2 ;;
esac

$PY "$here/driver.py" --app "$app" --out "$out" --session "$session"
