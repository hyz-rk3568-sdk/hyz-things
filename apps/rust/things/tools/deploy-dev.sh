#!/usr/bin/env bash

set -euo pipefail

ACTION=${1:-deploy}
BINARY=${2:-}
ADB=${ADB:-adb}
ADB_SERIAL=${ADB_SERIAL:-}
CURL=${CURL:-curl}
REMOTE_DEV_DIR=/userdata/hyz-router/dev
REMOTE_TARGET=/usr/bin/hyz-router
REMOTE_INIT_SCRIPT=/etc/init.d/S80hyz-router-dev
REMOTE_FIRMWARE_SHA=$REMOTE_DEV_DIR/firmware.sha256
ADB_WAIT_SECONDS=${ADB_WAIT_SECONDS:-360}
READY_WAIT_SECONDS=${READY_WAIT_SECONDS:-180}

adb_args=()
if [[ -n "$ADB_SERIAL" ]]; then
    adb_args=(-s "$ADB_SERIAL")
fi

device_shell() {
    $ADB "${adb_args[@]}" shell "$1"
}

valid_sha() {
    [[ "$1" =~ ^[0-9a-f]{64}$ ]]
}

wait_for_adb() {
    local deadline=$((SECONDS + ADB_WAIT_SECONDS))

    while ((SECONDS < deadline)); do
        if $ADB "${adb_args[@]}" get-state >/dev/null 2>&1; then
            return 0
        fi
        sleep 2
    done
    printf 'ADB did not reconnect within %s seconds\n' "$ADB_WAIT_SECONDS" >&2
    return 1
}

reboot_board() {
    local previous_boot_id current_boot_id deadline

    previous_boot_id=$(device_shell 'cat /proc/sys/kernel/random/boot_id' | tr -d '\r\n')
    device_shell 'sync'
    $ADB "${adb_args[@]}" reboot >/dev/null
    deadline=$((SECONDS + ADB_WAIT_SECONDS))
    while ((SECONDS < deadline)); do
        if $ADB "${adb_args[@]}" get-state >/dev/null 2>&1; then
            current_boot_id=$(device_shell 'cat /proc/sys/kernel/random/boot_id' 2>/dev/null | tr -d '\r\n')
            if [[ -n "$current_boot_id" && "$current_boot_id" != "$previous_boot_id" ]]; then
                return 0
            fi
        fi
        sleep 2
    done
    printf 'board did not complete a new boot within %s seconds\n' "$ADB_WAIT_SECONDS" >&2
    return 1
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
    deadline=$((SECONDS + READY_WAIT_SECONDS))
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
    if ! device_shell 'wget -q -T 3 -O /dev/null http://192.168.8.1:8080/api/v1/health' >/dev/null; then
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

read_firmware_sha() {
    device_shell "cat '$REMOTE_FIRMWARE_SHA' 2>/dev/null || true" | tr -d '\r\n'
}

ensure_firmware_sha() {
    local firmware_sha

    firmware_sha=$(read_firmware_sha)
    if valid_sha "$firmware_sha"; then
        printf '%s\n' "$firmware_sha"
        return 0
    fi
    if device_shell "test -e '$REMOTE_INIT_SCRIPT'" >/dev/null 2>&1; then
        printf 'firmware SHA-256 is missing while a development boot override exists; refusing to guess the underlying ELF\n' >&2
        return 1
    fi
    firmware_sha=$(device_shell "sha256sum '$REMOTE_TARGET'" | tr -d '\r' | awk '{print $1}')
    if ! valid_sha "$firmware_sha"; then
        printf 'could not determine the firmware ELF SHA-256\n' >&2
        return 1
    fi
    device_shell "install -d -m 0700 '$REMOTE_DEV_DIR'; printf '%s\\n' '$firmware_sha' > '$REMOTE_FIRMWARE_SHA.tmp'; chmod 0600 '$REMOTE_FIRMWARE_SHA.tmp'; mv '$REMOTE_FIRMWARE_SHA.tmp' '$REMOTE_FIRMWARE_SHA'; sync"
    printf '%s\n' "$firmware_sha"
}

write_init_script() {
    local host_sha=$1
    local remote_binary=$2
    local local_script local_script_sha remote_script remote_script_sha

    local_script=$(mktemp)
    trap 'rm -f "${local_script:-}"' RETURN
    cat >"$local_script" <<EOF
#!/bin/sh

SOURCE=$remote_binary
EXPECTED_SHA=$host_sha
TARGET=$REMOTE_TARGET
RUNTIME_DIR=/run/hyz-router-dev
LOG=/run/hyz-router-dev-mount.log

case "\${1:-}" in
    start)
        umask 077
        install -d -m 0700 "\$RUNTIME_DIR" || exit 1
        actual=\$(sha256sum "\$SOURCE" 2>/dev/null | cut -d' ' -f1)
        if [ "\$actual" != "\$EXPECTED_SHA" ]; then
            echo "source SHA-256 mismatch" >"\$LOG"
            exit 1
        fi
        cp "\$SOURCE" "\$RUNTIME_DIR/hyz-router.next" || exit 1
        chmod 0755 "\$RUNTIME_DIR/hyz-router.next" || exit 1
        mv "\$RUNTIME_DIR/hyz-router.next" "\$RUNTIME_DIR/hyz-router" || exit 1
        mount -o bind "\$RUNTIME_DIR/hyz-router" "\$TARGET" || exit 1
        actual=\$(sha256sum "\$TARGET" | cut -d' ' -f1)
        if [ "\$actual" != "\$EXPECTED_SHA" ]; then
            echo "mounted SHA-256 mismatch" >"\$LOG"
            umount "\$TARGET" || true
            exit 1
        fi
        printf 'development ELF active: %s\\n' "\$EXPECTED_SHA" >"\$LOG"
        ;;
    stop) ;;
    *) echo "Usage: \$0 {start|stop}" >&2; exit 1 ;;
esac
EOF
    chmod 0755 "$local_script"
    local_script_sha=$(sha256sum "$local_script" | awk '{print $1}')
    remote_script="$REMOTE_DEV_DIR/S80hyz-router-dev.$host_sha"
    $ADB "${adb_args[@]}" push "$local_script" "$remote_script"
    remote_script_sha=$(device_shell "sha256sum '$remote_script'" | tr -d '\r' | awk '{print $1}')
    if [[ "$remote_script_sha" != "$local_script_sha" ]]; then
        printf 'staged init script SHA-256 mismatch: expected %s, got %s\n' "$local_script_sha" "$remote_script_sha" >&2
        return 1
    fi
    device_shell "install -m 0755 '$remote_script' '$REMOTE_INIT_SCRIPT.next'; mv '$REMOTE_INIT_SCRIPT.next' '$REMOTE_INIT_SCRIPT'; sync"
    rm -f "$local_script"
    trap - RETURN
}

deploy() {
    local host_sha remote_binary remote_staged firmware_sha

    if [[ -z "$BINARY" || ! -f "$BINARY" ]]; then
        printf 'usage: %s deploy PATH_TO_HYZ_ROUTER\n' "$0" >&2
        return 2
    fi
    host_sha=$(sha256sum "$BINARY" | awk '{print $1}')
    if ! valid_sha "$host_sha"; then
        printf 'host ELF SHA-256 is invalid\n' >&2
        return 1
    fi
    remote_binary="$REMOTE_DEV_DIR/hyz-router.$host_sha"

    firmware_sha=$(ensure_firmware_sha)
    device_shell "install -d -m 0700 '$REMOTE_DEV_DIR'"
    $ADB "${adb_args[@]}" push "$BINARY" "$remote_binary.tmp"
    remote_staged=$(device_shell "sha256sum '$remote_binary.tmp'" | tr -d '\r' | awk '{print $1}')
    if [[ "$remote_staged" != "$host_sha" ]]; then
        printf 'staged ELF SHA-256 mismatch: expected %s, got %s\n' "$host_sha" "$remote_staged" >&2
        return 1
    fi
    device_shell "chmod 0755 '$remote_binary.tmp'; mv '$remote_binary.tmp' '$remote_binary'; sync"
    write_init_script "$host_sha" "$remote_binary"

    printf 'development ELF staged: %s\n' "$host_sha"
    printf 'firmware ELF preserved: %s\n' "$firmware_sha"
    printf 'rebooting to activate the checksum-locked development boot override\n'
    reboot_board
    verify_ready "$host_sha"
    printf 'development ELF active after reboot: %s\n' "$host_sha"
}

revert() {
    local firmware_sha

    firmware_sha=$(read_firmware_sha)
    if ! valid_sha "$firmware_sha"; then
        printf 'recorded firmware SHA-256 is missing or invalid; refusing automatic revert\n' >&2
        return 1
    fi
    if ! device_shell "test -e '$REMOTE_INIT_SCRIPT'" >/dev/null 2>&1; then
        verify_ready "$firmware_sha"
        printf 'no development boot override is installed; firmware ELF is active: %s\n' "$firmware_sha"
        return 0
    fi

    device_shell "rm -f '$REMOTE_INIT_SCRIPT'; sync"
    printf 'development boot override removed; rebooting to restore the firmware ELF\n'
    reboot_board
    verify_ready "$firmware_sha"
    printf 'firmware ELF restored after reboot: %s\n' "$firmware_sha"
}

wait_for_adb
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
