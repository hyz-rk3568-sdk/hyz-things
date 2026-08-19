#!/bin/sh
# Lab-only dual-uplink reference. Run as root on the target with hyz-router stopped.
set -eu

PATH=/usr/sbin:/usr/bin:/sbin:/bin

RUNTIME_DIR=/run/hyz-dual-uplink
DEFAULT_UDHCPC_SCRIPT=/usr/share/udhcpc/default.script
WPA_CONFIG=/run/hyz-router/wpa_supplicant.rust.conf
WPA_PID_FILE=$RUNTIME_DIR/wpa_supplicant.pid
WPA_LOG=$RUNTIME_DIR/wpa_supplicant.log
DHCP_HOOK=$RUNTIME_DIR/udhcpc.script

usage() {
    printf '%s\n' "usage: $0 {reset|start-wifi|stop-wifi|start-dhcp|stop-dhcp|status|assert} [eth0|wlan0|none|ethernet|wifi|both]" >&2
    exit 2
}

die() {
    printf 'dual-uplink-reference: %s\n' "$*" >&2
    exit 1
}

require_root() {
    [ "$(id -u)" -eq 0 ] || die "must run as root"
}

require_router_stopped() {
    if pidof hyz-router >/dev/null 2>&1; then
        die "hyz-router is running; stop it before using the lab reference"
    fi
}

check_interface() {
    case "$1" in
        eth0|wlan0) ;;
        *) die "unsupported uplink: $1" ;;
    esac
}

metric_for() {
    case "$1" in
        eth0) printf '100\n' ;;
        wlan0) printf '600\n' ;;
        *) die "unsupported uplink: $1" ;;
    esac
}

pid_from_file() {
    file=$1
    [ -r "$file" ] || return 1
    pid=$(cat "$file")
    case "$pid" in
        ''|*[!0-9]*) return 1 ;;
    esac
    kill -0 "$pid" 2>/dev/null || return 1
    printf '%s\n' "$pid"
}

write_dhcp_hook() {
    mkdir -p "$RUNTIME_DIR"
    cat >"$DHCP_HOOK" <<'EOF'
#!/bin/sh
set -eu

PATH=/usr/sbin:/usr/bin:/sbin:/bin
DEFAULT_UDHCPC_SCRIPT=/usr/share/udhcpc/default.script
interface=${interface:-}
case "$interface" in
    eth0) metric=100 ;;
    wlan0) metric=600 ;;
    *) exit 1 ;;
esac

case "${1:-}" in
    bound|renew)
        "$DEFAULT_UDHCPC_SCRIPT" "$@"
        gateway=${router:-}
        gateway=${gateway%% *}
        [ -n "$gateway" ] || exit 1
        while /usr/sbin/ip -4 route del default dev "$interface" 2>/dev/null; do
            :
        done
        /usr/sbin/ip -4 route replace default via "$gateway" dev "$interface" metric "$metric"
        ;;
    deconfig)
        "$DEFAULT_UDHCPC_SCRIPT" "$@" || true
        while /usr/sbin/ip -4 route del default dev "$interface" 2>/dev/null; do
            :
        done
        ;;
    *)
        exec "$DEFAULT_UDHCPC_SCRIPT" "$@"
        ;;
esac
EOF
    chmod 0755 "$DHCP_HOOK"
}

udhcpc_pid_for_interface() {
    interface=$1
    for pid in $(pidof udhcpc 2>/dev/null || true); do
        [ -r "/proc/$pid/cmdline" ] || continue
        command_line=$(tr '\000' ' ' </proc/$pid/cmdline)
        case "$command_line" in
            *" -i $interface "*|*" --interface $interface "*)
                printf '%s\n' "$pid"
                return 0
                ;;
        esac
    done
    return 1
}

clear_global_addresses() {
    interface=$1
    ip -o -4 address show dev "$interface" 2>/dev/null |
        while read -r _ _ _ cidr _; do
            [ -n "${cidr:-}" ] || continue
            ip -4 address del "$cidr" dev "$interface" 2>/dev/null || true
        done
}

stop_dhcp() {
    interface=$1
    check_interface "$interface"
    write_dhcp_hook

    pid_file=$RUNTIME_DIR/$interface.udhcpc.pid
    pid=$(pid_from_file "$pid_file" 2>/dev/null || true)
    if [ -n "$pid" ]; then
        kill -TERM "$pid" 2>/dev/null || true
        deadline=20
        while kill -0 "$pid" 2>/dev/null && [ "$deadline" -gt 0 ]; do
            sleep 1
            deadline=$((deadline - 1))
        done
        if kill -0 "$pid" 2>/dev/null; then
            kill -KILL "$pid" 2>/dev/null || true
        fi
    fi
    rm -f "$pid_file"

    interface="$interface" "$DHCP_HOOK" deconfig || true
    metric=$(metric_for "$interface")
    ip -4 route del default dev "$interface" metric "$metric" 2>/dev/null || true
    clear_global_addresses "$interface"
}

start_dhcp() {
    interface=$1
    check_interface "$interface"
    require_router_stopped
    write_dhcp_hook
    mkdir -p "$RUNTIME_DIR"

    pid_file=$RUNTIME_DIR/$interface.udhcpc.pid
    if pid_from_file "$pid_file" >/dev/null 2>&1; then
        return 0
    fi
    if pid=$(udhcpc_pid_for_interface "$interface" 2>/dev/null); then
        die "an unowned udhcpc already uses $interface (pid $pid)"
    fi

    ip link set dev "$interface" up
    /usr/sbin/udhcpc -f -i "$interface" -t 3 -T 2 -A 2 -s "$DHCP_HOOK" \
        >"$RUNTIME_DIR/$interface.udhcpc.log" 2>&1 &
    pid=$!
    printf '%s\n' "$pid" >"$pid_file"
    sleep 1
    if ! kill -0 "$pid" 2>/dev/null; then
        cat "$RUNTIME_DIR/$interface.udhcpc.log" >&2 || true
        rm -f "$pid_file"
        die "udhcpc exited while starting on $interface"
    fi
}

start_wifi() {
    require_router_stopped
    [ -r "$WPA_CONFIG" ] || die "missing fixed Wi-Fi config: $WPA_CONFIG"
    mkdir -p "$RUNTIME_DIR"
    if pid_from_file "$WPA_PID_FILE" >/dev/null 2>&1; then
        return 0
    fi
    if pidof wpa_supplicant >/dev/null 2>&1; then
        die "an unowned wpa_supplicant is already running"
    fi

    ip link set dev wlan0 up
    /usr/sbin/wpa_supplicant -i wlan0 -D nl80211,wext -c "$WPA_CONFIG" \
        >"$WPA_LOG" 2>&1 &
    pid=$!
    printf '%s\n' "$pid" >"$WPA_PID_FILE"
    sleep 1
    if ! kill -0 "$pid" 2>/dev/null; then
        cat "$WPA_LOG" >&2 || true
        rm -f "$WPA_PID_FILE"
        die "wpa_supplicant exited while starting"
    fi
}

stop_wifi() {
    stop_dhcp wlan0
    pid=$(pid_from_file "$WPA_PID_FILE" 2>/dev/null || true)
    if [ -n "$pid" ]; then
        kill -TERM "$pid" 2>/dev/null || true
        deadline=10
        while kill -0 "$pid" 2>/dev/null && [ "$deadline" -gt 0 ]; do
            sleep 1
            deadline=$((deadline - 1))
        done
        if kill -0 "$pid" 2>/dev/null; then
            kill -KILL "$pid" 2>/dev/null || true
        fi
    fi
    rm -f "$WPA_PID_FILE"
}

state() {
    eth_ready=0
    wifi_ready=0
    if ip -4 address show dev eth0 2>/dev/null | grep -q ' inet '; then
        if ip -4 route show default 2>/dev/null | grep -q 'dev eth0 metric 100'; then
            eth_ready=1
        fi
    fi
    if ip -4 address show dev wlan0 2>/dev/null | grep -q ' inet '; then
        if ip -4 route show default 2>/dev/null | grep -q 'dev wlan0 metric 600'; then
            wifi_ready=1
        fi
    fi

    if [ "$eth_ready" -eq 1 ] && [ "$wifi_ready" -eq 1 ]; then
        state=both
        active=ethernet
    elif [ "$eth_ready" -eq 1 ]; then
        state=ethernet
        active=ethernet
    elif [ "$wifi_ready" -eq 1 ]; then
        state=wifi
        active=wifi
    else
        state=none
        active=none
    fi

    printf 'ETHERNET_READY=%s\n' "$eth_ready"
    printf 'WIFI_READY=%s\n' "$wifi_ready"
    printf 'STATE=%s\n' "$state"
    printf 'ACTIVE_UPLINK=%s\n' "$active"
    printf '%s\n' '--- addresses ---'
    ip -4 address show dev eth0 || true
    ip -4 address show dev wlan0 || true
    printf '%s\n' '--- default routes ---'
    ip -4 route show default || true
}

assert_state() {
    expected=$1
    case "$expected" in
        none|ethernet|wifi|both) ;;
        *) die "invalid expected state: $expected" ;;
    esac
    actual=$(state | awk -F= '$1 == "STATE" { print $2 }')
    [ "$actual" = "$expected" ] || die "expected state $expected, observed $actual"
    printf 'ASSERT_STATE=%s\n' "$actual"
}

main() {
    require_root
    command=${1:-}
    case "$command" in
        reset)
            require_router_stopped
            stop_dhcp eth0
            stop_wifi
            ;;
        start-wifi)
            start_wifi
            ;;
        stop-wifi)
            require_router_stopped
            stop_wifi
            ;;
        start-dhcp)
            [ "$#" -eq 2 ] || usage
            start_dhcp "$2"
            ;;
        stop-dhcp)
            [ "$#" -eq 2 ] || usage
            require_router_stopped
            stop_dhcp "$2"
            ;;
        status)
            state
            ;;
        assert)
            [ "$#" -eq 2 ] || usage
            assert_state "$2"
            ;;
        *)
            usage
            ;;
    esac
}

main "$@"
