#!/bin/bash
# Live verification run for t_44e24832 on a private session bus.
#
# Usage: run_live.sh <theme> <W> <H>
#
# 1. private dbus session bus (no other session services on it)
# 2. scripted Session1 peer (orbis-fake-session-peer) — fixture power limits
#    incl. NVIDIA Dynamic Boost; no hardware access anywhere
# 3. production binary target/release/orbis-control against that bus,
#    sandboxed XDG_CONFIG_HOME/XDG_STATE_HOME, X11 backend
# 4. dashboard capture (nav measurement), click Производительность, capture
#    the performance section (power limits card incl. Dynamic Boost row)
#
# Never touches the user's own live orbis-control instance: the new window is
# identified by window-id diff, and everything is torn down by recorded PIDs.
set -u

# X access for xdotool/import in agent shells that lack a session environment.
export DISPLAY="${DISPLAY:-:0}"
export XAUTHORITY="${XAUTHORITY:-/run/user/1000/xauth_pVsGMV}"

THEME="${1:?theme}"
W="${2:?width}"
H="${3:?height}"
ROOT="/home/artt/Orbis-control-implementation"
TOOLS="$ROOT/docs/verification/ui-d-t44e24832/tools"
FAKE_PEER="$TOOLS/fake-peer/target/release/orbis-fake-session-peer"
BIN="$ROOT/target/release/orbis-control"
OUT="$ROOT/docs/verification/ui-d-t44e24832/live2/${THEME}-${W}x${H}"
TAG="${THEME}-${W}x${H}"

mkdir -p "$OUT"
RUNDIR="$OUT/run"
rm -rf "$RUNDIR"
mkdir -p "$RUNDIR/config-home/orbis-control" "$RUNDIR/state-home/orbis-control"
# dbus unix sockets are limited to ~107 bytes; keep the socket path short.
BUS_SOCK="/tmp/orbis-t44e-${THEME}-${W}x${H}.sock"
rm -f "$BUS_SOCK"

cat > "$RUNDIR/config-home/orbis-control/preferences.toml" <<EOF
schema_version = 1

[appearance]
theme = "$THEME"

[window]
close_action = "hide_to_tray"
start_minimized = false
remember_position = false
EOF

cleanup() {
    set +e
    [ -n "${APP_PID:-}" ] && kill "$APP_PID" 2>/dev/null
    [ -n "${PEER_PID:-}" ] && kill "$PEER_PID" 2>/dev/null
    [ -n "${BUS_PID:-}" ] && kill "$BUS_PID" 2>/dev/null
    wait 2>/dev/null
    set -e
}
trap cleanup EXIT INT TERM

# 1. private session bus (--print-address/--print-pid take fd numbers)
dbus-daemon --session \
    --address="unix:path=$BUS_SOCK" \
    --fork --print-address=3 --print-pid=4 \
    3> "$RUNDIR/bus-address" 4> "$RUNDIR/bus-pid"
BUS_PID=$(cat "$RUNDIR/bus-pid")
BUS_ADDR=$(cat "$RUNDIR/bus-address")
echo "[$TAG] bus $BUS_ADDR (pid $BUS_PID)"

# 2. scripted Session1 peer
"$FAKE_PEER" "$RUNDIR/bus-address" > "$OUT/fake-peer.log" 2>&1 &
PEER_PID=$!
for _ in $(seq 1 60); do
    grep -q "Session1 up" "$OUT/fake-peer.log" && break
    sleep 0.25
done
if ! grep -q "Session1 up" "$OUT/fake-peer.log"; then
    echo "[$TAG] FAIL: peer did not come up" >&2
    cat "$OUT/fake-peer.log" >&2
    exit 3
fi
echo "[$TAG] peer up (pid $PEER_PID)"

# fixture truth from the wire, measured not assumed
gdbus call --address "$BUS_ADDR" \
    --dest io.github.orbiscontrol.Session \
    --object-path /io/github/orbiscontrol/Session \
    --method io.github.orbiscontrol.Session1.PowerLimits \
    > "$OUT/fixture-powerlimits.txt" 2>&1
echo "[$TAG] fixture wire: $(cat "$OUT/fixture-powerlimits.txt")"

# 3. production binary against the private bus
xdotool search --class orbis-control > "$RUNDIR/win-before.txt" 2>/dev/null || true
env -u WAYLAND_DISPLAY \
    DISPLAY="${DISPLAY:-:0}" \
    XAUTHORITY="${XAUTHORITY:-/run/user/1000/xauth_pVsGMV}" \
    DBUS_SESSION_BUS_ADDRESS="$BUS_ADDR" \
    DBUS_SYSTEM_BUS_ADDRESS="${DBUS_SYSTEM_BUS_ADDRESS:-unix:path=/run/user/1000/bus}" \
    XDG_CONFIG_HOME="$RUNDIR/config-home" \
    XDG_STATE_HOME="$RUNDIR/state-home" \
    "$BIN" > "$OUT/app.log" 2>&1 &
APP_PID=$!

WIN=""
for _ in $(seq 1 80); do
    sleep 0.25
    xdotool search --class orbis-control > "$RUNDIR/win-after.txt" 2>/dev/null || true
    WIN=$(comm -13 <(sort "$RUNDIR/win-before.txt") <(sort "$RUNDIR/win-after.txt") | head -1)
    [ -n "$WIN" ] && break
done
if [ -z "$WIN" ]; then
    echo "[$TAG] FAIL: no new orbis-control window" >&2
    tail -30 "$OUT/app.log" >&2
    exit 4
fi
echo "[$TAG] app pid $APP_PID window $WIN"
xdotool getwindowgeometry "$WIN"

# 4. geometry + dashboard capture
xdotool windowmove "$WIN" 40 40
xdotool windowsize "$WIN" "$W" "$H"
sleep 1.2
xdotool getwindowgeometry "$WIN" > "$OUT/geometry-dashboard.txt"
import -window "$WIN" "$OUT/cap-dashboard-$TAG.png"
echo "[$TAG] dashboard captured"

# performance section (power limits card)
xdotool mousemove --window "$WIN" 117 165 click 1
sleep 1.5
import -window "$WIN" "$OUT/cap-perf-$TAG.png"
echo "[$TAG] perf section captured"

# GPU limits rows (Dynamic Boost / GPU temp target) sit below the fold at
# min size; scroll the section area down and capture again.
xdotool mousemove --window "$WIN" 700 450
for _ in 1 2 3 4 5; do xdotool click 5; sleep 0.15; done
sleep 1.0
import -window "$WIN" "$OUT/cap-perf-scrolled-$TAG.png"
echo "[$TAG] perf scrolled captured"

# settle log tail for the power-limit chain
sleep 1
grep -iE "power.limit|panic|ERROR" "$OUT/app.log" | tail -20 > "$OUT/power-limit-log.txt" || true

cleanup
trap - EXIT INT TERM
echo "[$TAG] done"
