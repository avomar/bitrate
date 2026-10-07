#!/usr/bin/env bash
set -e

MOCK_DIR="/tmp/bitrate_simulation_$$"
NET_DIR="$MOCK_DIR/sys/class/net"
PROC_DIR="$MOCK_DIR/proc/net"

cleanup() {
    rm -rf "$MOCK_DIR"
    if [ -n "$APPLET_PID" ]; then
        kill "$APPLET_PID" 2>/dev/null || true
    fi
}
trap cleanup EXIT INT TERM

mkdir -p "$NET_DIR" "$PROC_DIR"

set_iface() {
    local name="$1"
    local operstate="$2"
    local carrier="$3"
    local is_bridge="${4:-false}"

    local dir="$NET_DIR/$name"
    mkdir -p "$dir/statistics"
    echo "$operstate" > "$dir/operstate"
    echo "$carrier" > "$dir/carrier"
    echo "1000000" > "$dir/statistics/rx_bytes"
    echo "500000" > "$dir/statistics/tx_bytes"

    if [ "$is_bridge" = "true" ]; then
        mkdir -p "$dir/bridge"
    else
        rm -rf "$dir/bridge"
    fi
}

set_route() {
    local default_iface="$1"
    local metric="${2:-100}"

    cat <<EOF > "$PROC_DIR/route"
Iface	Destination	Gateway 	Flags	RefCnt	Use	Metric	Mask		MTU	Window	IRTT
$default_iface	00000000	0101A8C0	0003	0	0	$metric	00000000	0	0	0
EOF
}

echo "=========================================================="
echo "Bitrate Applet Network Simulation Harness"
echo "=========================================================="

if [ "$1" = "live" ]; then
    echo "Starting interactive live simulation..."
    echo "Mock sysfs location: $MOCK_DIR"
    echo

    # Initial state: Wi-Fi connected, Ethernet disconnected, Docker active
    set_iface "wlo1" "up" "1" "false"
    set_iface "eno1" "down" "0" "false"
    set_iface "docker0" "up" "1" "true"
    set_iface "br-e02939798c38" "up" "1" "true"
    set_iface "veth8bc1e04" "up" "1" "false"
    set_route "wlo1" 600

    echo "Building applet in debug mode..."
    cargo build

    echo "Launching applet with BITRATE_MOCK_SYSFS=$MOCK_DIR..."
    BITRATE_MOCK_SYSFS="$MOCK_DIR" ./target/debug/cosmic-ext-applet-bitrate &
    APPLET_PID=$!

    sleep 1

    while true; do
        echo
        echo "Select an event to simulate:"
        echo "  [1] Plug in Ethernet (eno1 up, route priority 100)"
        echo "  [2] Unplug Ethernet (eno1 down, route via wlo1)"
        echo "  [3] Disconnect Wi-Fi (wlo1 down)"
        echo "  [4] Reconnect Wi-Fi (wlo1 up)"
        echo "  [5] Add more Docker containers & bridges"
        echo "  [q] Quit simulation"
        read -r -p "Enter choice [1-5, q]: " choice
        case "$choice" in
            1)
                echo "-> Simulating: Ethernet cable plugged in..."
                set_iface "eno1" "up" "1" "false"
                set_route "eno1" 100
                echo "   eno1 is now UP (carrier=1, default route metric 100)."
                ;;
            2)
                echo "-> Simulating: Ethernet cable unplugged..."
                set_iface "eno1" "down" "0" "false"
                set_route "wlo1" 600
                echo "   eno1 is now DOWN (carrier=0, default route back to wlo1)."
                ;;
            3)
                echo "-> Simulating: Wi-Fi disconnected..."
                set_iface "wlo1" "down" "0" "false"
                echo "   wlo1 is now DOWN."
                ;;
            4)
                echo "-> Simulating: Wi-Fi connected..."
                set_iface "wlo1" "up" "1" "false"
                echo "   wlo1 is now UP."
                ;;
            5)
                echo "-> Simulating: Docker bridges and veths started..."
                set_iface "br-new123456" "up" "1" "true"
                set_iface "vethnew789" "up" "1" "false"
                echo "   Docker networks created (should be ignored by applet)."
                ;;
            q|Q)
                echo "Exiting..."
                break
                ;;
            *)
                echo "Unknown option."
                ;;
        esac
    done
else
    echo "Running automated real-life simulation test..."
    echo

    # 1. Initial State: Wi-Fi active, Ethernet unplugged, Docker running
    echo "[Step 1] Initial state: Wi-Fi active (wlo1), Ethernet unplugged (eno1), Docker running"
    set_iface "wlo1" "up" "1" "false"
    set_iface "eno1" "down" "0" "false"
    set_iface "docker0" "up" "1" "true"
    set_iface "br-e02939798c38" "up" "1" "true"
    set_iface "veth8bc1e04" "up" "1" "false"
    set_route "wlo1" 600

    BITRATE_MOCK_SYSFS="$MOCK_DIR" cargo test test_real_life_simulation_scenario -- --nocapture

    echo
    echo "All simulation scenarios passed successfully!"
    echo "Tip: Run './simulate.sh live' to interactively test network events with the UI."
fi
