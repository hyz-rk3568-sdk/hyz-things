#!/usr/bin/env bash

set -euo pipefail

ACTION=${1:-deploy}
BINARY=${2:-}
ADB=${ADB:-adb}
ADB_SERIAL=${ADB_SERIAL:-}
CURL=${CURL:-curl}
REMOTE_DEV_DIR=/userdata/hyz-router/dev
REMOTE_RUN_DIR=/run/hyz-router-dev
REMOTE_TARGET=/usr/bin/hyz-router
INIT_SCRIPT=/etc/init.d/S81hyz-router

adb_args=()
if [[ -n "$ADB_SERIAL" ]]; then
    adb_args=(-s "$ADB_SERIAL")
fi

device_shell() {
    "$ADB" "${adb_args[@]}" shell "$1"
}

remote_action() {
    local name=$1
    local command=$2
    local marker="/run/hyz-router-dev-${name}-$$"
    local result

    device_shell "rm -f '$marker'; $command; rc=\$?; printf '%s\\n' \"\$rc\" > '$marker'; sync" || true
    result=$(device_shell "cat '$marker' 2>/dev/null || echo MISSING" | tr -d '\r\n')
    device_shell "rm -f '$marker'" >/dev/null 2>&1 || true
    if [[ "$result" != 0 ]]; then
        printf 'remote %s failed (status %s)\n' "$name" "$result" >&2
        return 1
    fi
}

mount_state() {
    device_shell "if grep -q ' $REMOTE_TARGET ' /proc/mounts; then echo mounted; else echo unmounted; fi" |
        tr -d '\r\n'
}

show_diagnostics() {
    device_shell "cat /run/hyz-router/shutdown.log 2>/dev/null || true; cat /run/hyz-router-init.log 2>/dev/null || true; ps -ef" >&2 || true
}

require_clean_stop() {
    local residual ownership

    residual=$(device_shell "for name in hyz-router mihomo tailscaled hostapd dnsmasq wpa_supplicant udhcpc; do pidof \"\$name\" 2>/dev/null || true; done" |
        tr -d '\r\n ')
    if [[ -n "$residual" ]]; then
        printf 'runtime processes remain after stop: %s\n' "$residual" >&2
        return 1
    fi

    ownership=$(device_shell "find /run/hyz-router/daemon.lock /run/hyz-network.lock /run/hyz-router/control.sock /run/hyz-mihomo/core.pid /run/hyz-mihomo/watch.pid /run/hyz-tailscale/tailscaled.pid /run/hyz-tailscale/tailscaled.sock 2>/dev/null || true" |
        tr -d '\r\n ')
    if [[ -n "$ownership" ]]; then
        printf 'runtime ownership remains after stop: %s\n' "$ownership" >&2
        return 1
    fi
}

stop_runtime() {
    if ! remote_action stop "$INIT_SCRIPT stop"; then
        show_diagnostics
        return 1
    fi
    if ! require_clean_stop; then
        show_diagnostics
        return 1
    fi
}

verify_ready() {
    local expected_sha=$1
    local actual_sha status_file readiness_error tailscale_ip ready deadline

    actual_sha=$(device_shell "sha256sum '$REMOTE_TARGET'" | tr -d '\r' | awk '{print $1}')
    if [[ "$actual_sha" != "$expected_sha" ]]; then
        printf 'running ELF SHA-256 mismatch: expected %s, got %s\n' "$expected_sha" "$actual_sha" >&2
        return 1
    fi

    status_file=$(mktemp)
    readiness_error=$(mktemp)
    trap 'rm -f "${status_file:-}" "${readiness_error:-}"' RETURN
    ready=false
    deadline=$((SECONDS + 180))
    while ((SECONDS < deadline)); do
        if device_shell "$REMOTE_TARGET status" 2>/dev/null | tr -d '\r' >"$status_file"; then
            if tailscale_ip=$(python3 - "$status_file" 2>"$readiness_error" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as stream:
    status = json.load(stream)
if status.get("state") != "ok":
    raise SystemExit("router status is not ok")
router = status.get("router", {})
proxy = status.get("proxy", {})
tailscale = status.get("tailscale", {})
if router.get("state") != "available":
    raise SystemExit("router state is unavailable")
proxy_data = proxy.get("data", {})
mihomo = proxy_data.get("mihomo", {})
legacy_proxy_ready = proxy_data.get("state") == "running"
core_required = mihomo.get("configured_required")
if proxy.get("state") != "available":
    raise SystemExit("proxy state is unavailable")
if core_required is True and mihomo.get("process") != "ready":
    raise SystemExit("required Mihomo Core is not running")
if core_required is None and not legacy_proxy_ready and mihomo.get("process") != "ready":
    raise SystemExit("Mihomo readiness is unavailable in the legacy status format")
lan_tun = proxy_data.get("lan_tun")
if isinstance(lan_tun, dict) and lan_tun.get("desired") is True and lan_tun.get("effective") != "ready":
    raise SystemExit("LAN TUN is not strictly ready")
if tailscale.get("state") != "available":
    raise SystemExit("Tailscale state is unavailable")
data = tailscale.get("data", {})
desired = data.get("desired_mode")
if desired == "lan_subnet_access":
    required = {
        "effective_mode": "lan_subnet_access",
        "backend_state": "running",
        "authenticated": True,
        "route_advertised": True,
        "local_firewall_ready": True,
    }
    for key, value in required.items():
        if data.get(key) != value:
            raise SystemExit(f"Tailscale {key} is not strictly ready")
print(data.get("ipv4") or "")
PY
            ); then
                ready=true
                break
            fi
        fi
        sleep 2
    done
    if [[ "$ready" != true ]]; then
        cat "$readiness_error" >&2
        return 1
    fi
    if ! device_shell "wget -q -T 3 -O /dev/null http://192.168.8.1:8080/api/v1/health" >/dev/null; then
        printf 'LAN management listener did not pass its health check\n' >&2
        return 1
    fi
    if [[ -n "$tailscale_ip" ]] && ! "$CURL" --fail --silent --show-error --max-time 15 \
        "http://$tailscale_ip:8080/api/v1/health" >/dev/null; then
        printf 'Tailscale management listener did not pass its health check\n' >&2
        return 1
    fi
    rm -f "$status_file" "$readiness_error"
    trap - RETURN
}

start_runtime() {
    local expected_sha=$1

    if ! remote_action start "$INIT_SCRIPT start"; then
        show_diagnostics
        return 1
    fi
    verify_ready "$expected_sha"
}

unmount_dev_binary() {
    if [[ "$(mount_state)" == mounted ]]; then
        remote_action unmount "umount '$REMOTE_TARGET'"
    fi
}

recover_firmware_after_failed_deploy() {
    local firmware_sha=$1

    printf 'development start failed; attempting only the verified graceful recovery path\n' >&2
    if ! stop_runtime; then
        printf 'recovery stopped: runtime ownership is not clean; the development mount was preserved\n' >&2
        return 1
    fi
    if ! unmount_dev_binary; then
        printf 'recovery stopped: development bind mount could not be removed\n' >&2
        return 1
    fi
    if ! start_runtime "$firmware_sha"; then
        printf 'firmware restart also failed; inspect the board without deleting ownership records\n' >&2
        return 1
    fi
    printf 'formal firmware ELF restored after failed development start\n' >&2
}

deploy() {
    local host_sha remote_staged firmware_sha remote_binary

    if [[ -z "$BINARY" || ! -f "$BINARY" ]]; then
        printf 'usage: %s deploy PATH_TO_HYZ_ROUTER\n' "$0" >&2
        return 2
    fi
    host_sha=$(sha256sum "$BINARY" | awk '{print $1}')
    remote_binary="$REMOTE_DEV_DIR/hyz-router.$host_sha"

    device_shell "install -d -m 0700 '$REMOTE_DEV_DIR'"
    "$ADB" "${adb_args[@]}" push "$BINARY" "$remote_binary"
    remote_staged=$(device_shell "sha256sum '$remote_binary'" | tr -d '\r' | awk '{print $1}')
    if [[ "$remote_staged" != "$host_sha" ]]; then
        printf 'staged ELF SHA-256 mismatch: expected %s, got %s\n' "$host_sha" "$remote_staged" >&2
        return 1
    fi

    stop_runtime
    unmount_dev_binary
    firmware_sha=$(device_shell "sha256sum '$REMOTE_TARGET'" | tr -d '\r' | awk '{print $1}')
    device_shell "printf '%s\\n' '$firmware_sha' > '$REMOTE_DEV_DIR/firmware.sha256'; chmod 0600 '$REMOTE_DEV_DIR/firmware.sha256'; install -d -m 0700 '$REMOTE_RUN_DIR'; cp '$remote_binary' '$REMOTE_RUN_DIR/hyz-router.next'; chmod 0755 '$REMOTE_RUN_DIR/hyz-router.next'; mv '$REMOTE_RUN_DIR/hyz-router.next' '$REMOTE_RUN_DIR/hyz-router'; sync"
    remote_action mount "mount -o bind '$REMOTE_RUN_DIR/hyz-router' '$REMOTE_TARGET'"

    if ! start_runtime "$host_sha"; then
        recover_firmware_after_failed_deploy "$firmware_sha" || true
        return 1
    fi
    printf 'development ELF active: %s\n' "$host_sha"
    printf 'a board reboot or make router-revert-dev restores the firmware ELF\n'
}

revert() {
    local firmware_sha actual_sha

    if [[ "$(mount_state)" != mounted ]]; then
        printf 'no development bind mount is active\n'
        return 0
    fi
    firmware_sha=$(device_shell "cat '$REMOTE_DEV_DIR/firmware.sha256' 2>/dev/null || true" | tr -d '\r\n')
    if [[ ! "$firmware_sha" =~ ^[0-9a-f]{64}$ ]]; then
        printf 'recorded firmware SHA-256 is missing or invalid; refusing automatic revert\n' >&2
        return 1
    fi

    stop_runtime
    unmount_dev_binary
    actual_sha=$(device_shell "sha256sum '$REMOTE_TARGET'" | tr -d '\r' | awk '{print $1}')
    if [[ "$actual_sha" != "$firmware_sha" ]]; then
        printf 'unmounted firmware SHA-256 mismatch: expected %s, got %s\n' "$firmware_sha" "$actual_sha" >&2
        return 1
    fi
    start_runtime "$firmware_sha"
    printf 'formal firmware ELF restored: %s\n' "$firmware_sha"
}

"$ADB" "${adb_args[@]}" wait-for-device
case "$ACTION" in
    deploy)
        deploy
        ;;
    revert)
        revert
        ;;
    *)
        printf 'usage: %s {deploy PATH_TO_HYZ_ROUTER|revert}\n' "$0" >&2
        exit 2
        ;;
esac
