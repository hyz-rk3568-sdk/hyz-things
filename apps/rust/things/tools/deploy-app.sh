#!/usr/bin/env bash

# Per-application hot-push tool for the headless hyz-things board.
#
# Every application (router, things, camera) has its own init service and
# daemon binary. Pushing camera or things stops and restarts only that
# service; the stable router core is never restarted by another application's
# push. Pushing the router is the only action that restarts it.
#
# The wire contracts are versioned (hyz-contract). Servers accept their own
# and the previous protocol version; clients require an exact match. The
# persistent registry at /userdata/hyz-things/apps/registry.json records the
# protocol versions each deployed application speaks, and a push is refused
# before any service is stopped when it would break an installed peer.
# Applications installed from firmware have no registry entry and are assumed
# to speak the current source tree's protocol versions.
#
# Usage:
#   deploy-app.sh deploy NAME PATH_TO_BINARY
#   deploy-app.sh revert NAME
#   deploy-app.sh check NAME
#
# NAME is one of: router things camera

set -euo pipefail

ACTION=${1:-}
APP_NAME=${2:-}
BINARY=${3:-}
ADB=${ADB:-adb}
ADB_SERIAL=${ADB_SERIAL:-}
CONTRACT_DIR=${CONTRACT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../../contract" && pwd)}
THINGS_DIR=${THINGS_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}

REMOTE_APPS_DIR=/userdata/hyz-things/apps
REMOTE_REGISTRY=$REMOTE_APPS_DIR/registry.json
REMOTE_RUN_DIR=/run/hyz-things/apps
ROUTER_READY_MARKER=/run/hyz-router/ready
CAMERA_CONTROL_SOCKET=/run/hyz-camera/control.sock
CAMERA_OWNER_FILE=/run/hyz-camera/daemon.owner
THINGS_HEALTH_URL=http://192.168.8.1:8080/api/v1/health
ADB_WAIT_SECONDS=${ADB_WAIT_SECONDS:-360}
READY_WAIT_SECONDS=${READY_WAIT_SECONDS:-180}

# Fixed per-application registry: name -> init script and daemon binary.
declare -A INIT_SCRIPT=(
    [router]=/etc/init.d/S81hyz-router
    [things]=/etc/init.d/S83hyz-things
    [camera]=/etc/init.d/S82hyz-camera
)
declare -A DAEMON=(
    [router]=/usr/bin/hyz-router
    [things]=/usr/bin/hyz-things
    [camera]=/usr/bin/hyz-camera
)

ROUTER_WAS_READY=false
REGISTRY_FILE=

adb_args=()
if [[ -n "$ADB_SERIAL" ]]; then
    adb_args=(-s "$ADB_SERIAL")
fi

device_shell() {
    # 远端命令可能带 device_shell_rc 的回传标记；输出型调用不需要它。
    # grep 无匹配行时返回 1，pipefail 下会误报整个命令失败：输出型调用
    # 不依赖退出码，成败判断一律走 device_shell_rc。
    "$ADB" "${adb_args[@]}" shell "$1" | tr -d '\r' | grep -v '^__HYZ_RC__=' || true
}

# RK adbd 不把远端 shell 的退出码回传给宿主，`adb shell` 总是返回 0。
# 需要远端成败判断的地方显式回传退出码并解析，避免部署工具对失败"盲跑"
# （例如 init 脚本 stop/start 失败仍继续换二进制）。
device_shell_rc() {
    local output rc
    output=$("$ADB" "${adb_args[@]}" shell "$1; printf '\\n__HYZ_RC__=%d\\n' \$?" | tr -d '\r')
    rc=$(printf '%s\n' "$output" | sed -n 's/.*__HYZ_RC__=\([0-9][0-9]*\).*/\1/p' | tail -1)
    [[ -n "$rc" && "$rc" == "0" ]]
}

valid_sha() {
    [[ "$1" =~ ^[0-9a-f]{64}$ ]]
}

valid_app() {
    [[ -n "${INIT_SCRIPT[$APP_NAME]:-}" && -n "${DAEMON[$APP_NAME]:-}" ]]
}

usage() {
    printf 'usage: %s {deploy NAME PATH_TO_BINARY|revert NAME|check NAME}\n' "$0" >&2
    printf '       NAME is one of: router things camera\n' >&2
}

wait_for_adb() {
    local deadline=$((SECONDS + ADB_WAIT_SECONDS))

    while ((SECONDS < deadline)); do
        if "$ADB" "${adb_args[@]}" get-state >/dev/null 2>&1; then
            return 0
        fi
        sleep 2
    done
    printf 'ADB did not reconnect within %s seconds\n' "$ADB_WAIT_SECONDS" >&2
    return 1
}

read_registry() {
    REGISTRY_FILE=$(mktemp)
    device_shell "cat '$REMOTE_REGISTRY' 2>/dev/null || true" | tr -d '\r' >"$REGISTRY_FILE"
    if ! python3 -m json.tool "$REGISTRY_FILE" >/dev/null 2>&1; then
        printf 'no valid device registry yet (%s); assuming a fresh board\n' "$REMOTE_REGISTRY" >&2
        printf '{}\n' >"$REGISTRY_FILE"
    fi
}

registry_entry_value() { # peer protocol
    python3 - "$REGISTRY_FILE" "$1" "$2" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as stream:
    registry = json.load(stream)
entry = registry.get("apps", {}).get(sys.argv[2], {}).get("current") or {}
versions = entry.get("protocol_versions") or {}
print(versions.get(sys.argv[3], ""))
PY
}

registry_previous_sha() { # peer
    python3 - "$REGISTRY_FILE" "$1" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as stream:
    registry = json.load(stream)
previous = registry.get("apps", {}).get(sys.argv[2], {}).get("previous") or {}
print(previous.get("sha256", ""))
PY
}

protocol_version_of() { # app protocol
    local app=$1 protocol=$2 file pattern version
    case "$app:$protocol" in
        things:camera)
            file="$THINGS_DIR/src/adapters/outbound/camera.rs"
            pattern='^const CAMERA_CONTROL_PROTOCOL_VERSION: u16 = '
            ;;
        things:router|router:router)
            file="$CONTRACT_DIR/src/router.rs"
            pattern='^pub const PROTOCOL_VERSION: u16 = '
            ;;
        camera:camera)
            file="$CONTRACT_DIR/src/camera.rs"
            pattern='^pub const CONTROL_PROTOCOL_VERSION: u16 = '
            ;;
        *)
            printf 'unknown protocol edge %s:%s\n' "$app" "$protocol" >&2
            return 1
            ;;
    esac
    if [[ ! -f "$file" ]]; then
        printf 'cannot find %s\n' "$file" >&2
        return 1
    fi
    version=$(grep -m1 "$pattern" "$file" | sed -E 's/.*= ([0-9]+);.*/\1/')
    if ! [[ "$version" =~ ^[0-9]+$ ]]; then
        printf 'cannot read %s protocol version from %s\n' "$protocol" "$file" >&2
        return 1
    fi
    printf '%s\n' "$version"
}

pushed_protocol_versions() { # app -> "protocol version" lines
    case "$1" in
        router)
            printf 'router %s\n' "$(protocol_version_of router router)"
            ;;
        things)
            printf 'router %s\ncamera %s\n' \
                "$(protocol_version_of things router)" \
                "$(protocol_version_of things camera)"
            ;;
        camera)
            printf 'camera %s\n' "$(protocol_version_of camera camera)"
            ;;
    esac
}

previous_protocol_versions() { # -> "protocol version" lines recorded for the previous binary
    python3 - "$REGISTRY_FILE" "$APP_NAME" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as stream:
    registry = json.load(stream)
previous = registry.get("apps", {}).get(sys.argv[2], {}).get("previous") or {}
versions = previous.get("protocol_versions") or {}
for protocol, version in sorted(versions.items()):
    print(f"{protocol} {version}")
PY
}

compat_edges() { # pushed app -> "protocol peer" lines
    case "$1" in
        router) printf 'router things\n' ;;
        things) printf 'router router\ncamera camera\n' ;;
        camera) printf 'camera things\n' ;;
    esac
}

installed_peer_version() { # peer protocol
    local peer=$1 protocol=$2 version
    version=$(registry_entry_value "$peer" "$protocol")
    if [[ -n "$version" ]]; then
        printf '%s\n' "$version"
        return 0
    fi
    if device_shell_rc "test -x '${DAEMON[$peer]}'"; then
        version=$(protocol_version_of "$peer" "$protocol")
        printf 'registry has no %s entry; assuming installed firmware %s speaks %s protocol %s from source\n' \
            "$peer" "$peer" "$protocol" "$version" >&2
        printf '%s\n' "$version"
        return 0
    fi
    return 1
}

check_compatibility() { # versions_file
    local file=$1 protocol peer pushed_version installed_version
    while read -r protocol peer; do
        pushed_version=$(awk -v protocol="$protocol" '$1 == protocol {print $2}' "$file")
        if [[ -z "$pushed_version" ]]; then
            printf 'cannot determine the pushed %s protocol version\n' "$protocol" >&2
            return 1
        fi
        if ! installed_version=$(installed_peer_version "$peer" "$protocol"); then
            if [[ "$peer" == "router" ]]; then
                printf '%s depends on the router control protocol but no router is installed\n' "$APP_NAME" >&2
                return 1
            fi
            printf '%s is not installed; skipping the %s protocol check\n' "$peer" "$protocol" >&2
            continue
        fi
        if [[ "$pushed_version" != "$installed_version" ]]; then
            printf 'protocol %s mismatch: pushed %s speaks %s, installed %s speaks %s\n' \
                "$protocol" "$APP_NAME" "$pushed_version" "$peer" "$installed_version" >&2
            printf 'the wire contract requires equal versions; deploy a matching %s first (servers accept their own and the previous version)\n' \
                "$peer" >&2
            return 1
        fi
    done < <(compat_edges "$APP_NAME")
    return 0
}

verify_ready() {
    local probe deadline
    case "$APP_NAME" in
        router) probe="[ -s '$ROUTER_READY_MARKER' ]" ;;
        things) probe="/usr/bin/wget -q -T 3 -O /dev/null '$THINGS_HEALTH_URL'" ;;
        camera) probe="[ -S '$CAMERA_CONTROL_SOCKET' ] && [ -f '$CAMERA_OWNER_FILE' ]" ;;
    esac
    deadline=$((SECONDS + READY_WAIT_SECONDS))
    while ((SECONDS < deadline)); do
        if device_shell_rc "$probe"; then
            return 0
        fi
        sleep 2
    done
    printf '%s did not become ready within %s seconds\n' "$APP_NAME" "$READY_WAIT_SECONDS" >&2
    return 1
}

assert_router_untouched() {
    if ! device_shell_rc "test -s '$ROUTER_READY_MARKER'"; then
        printf '%s push disturbed the running router: %s is gone\n' "$APP_NAME" "$ROUTER_READY_MARKER" >&2
        return 1
    fi
}

swap_binary() { # staged_on_device expected_sha
    local init_script=${INIT_SCRIPT[$APP_NAME]} target=${DAEMON[$APP_NAME]} target_next
    target_next="$target.next"
    # Only this application's service is stopped and restarted; the init
    # scripts of the other applications are never invoked.
    if ! device_shell_rc "'$init_script' stop"; then
        printf '%s init script refused to stop; binary left unchanged\n' "$APP_NAME" >&2
        return 1
    fi
    if ! device_shell_rc "install -m 0755 '$1' '$target_next' && test \"\$(sha256sum '$target_next' | cut -d' ' -f1)\" = '$2' && mv '$target_next' '$target' && sync"; then
        printf 'failed to atomically install %s\n' "$target" >&2
        return 1
    fi
    if ! device_shell_rc "'$init_script' start"; then
        printf '%s failed to start; run %s revert %s to roll back\n' "$APP_NAME" "$0" "$APP_NAME" >&2
        return 1
    fi
    verify_ready
    if [[ "$ROUTER_WAS_READY" == true ]]; then
        assert_router_untouched
    fi
}

write_registry() { # sha
    local entry_json registry_json host_sha remote_sha tmp versions_file
    versions_file=$(mktemp)
    pushed_protocol_versions "$APP_NAME" >"$versions_file"
    # 版本行经临时文件传给 python：heredoc（脚本体）和进程替换会同时占用
    # stdin，后者会被覆盖，导致 registry 的 protocol_versions 写空。
    entry_json=$(python3 - "$APP_NAME" "${DAEMON[$APP_NAME]}" "${INIT_SCRIPT[$APP_NAME]}" "$1" "$versions_file" <<'PY'
import json
import sys

name, binary, init_script, sha, versions_file = sys.argv[1:6]
versions = {}
with open(versions_file, encoding="utf-8") as stream:
    for line in stream:
        if not line.strip():
            continue
        protocol, version = line.split()
        versions[protocol] = int(version)
print(json.dumps({
    "binary": binary,
    "init_script": init_script,
    "sha256": sha,
    "protocol_versions": versions,
}, sort_keys=True))
PY
)
    rm -f "$versions_file"
    registry_json=$(python3 - "$REGISTRY_FILE" "$APP_NAME" "$entry_json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as stream:
    registry = json.load(stream)
apps = registry.setdefault("apps", {})
old = apps.get(sys.argv[2], {}).get("current")
entry = json.loads(sys.argv[3])
apps[sys.argv[2]] = {
    "current": entry,
    "previous": None if old is None else {k: v for k, v in old.items() if k != "previous"},
}
print(json.dumps(registry, sort_keys=True))
PY
)
    tmp=$(mktemp)
    printf '%s\n' "$registry_json" >"$tmp"
    host_sha=$(sha256sum "$tmp" | awk '{print $1}')
    "$ADB" "${adb_args[@]}" push "$tmp" "$REMOTE_REGISTRY.tmp" >/dev/null
    remote_sha=$(device_shell "sha256sum '$REMOTE_REGISTRY.tmp'" | tr -d '\r' | awk '{print $1}')
    if [[ "$remote_sha" != "$host_sha" ]]; then
        printf 'registry push SHA-256 mismatch: expected %s, got %s\n' "$host_sha" "$remote_sha" >&2
        rm -f "$tmp"
        return 1
    fi
    device_shell "install -d -m 0700 '$REMOTE_APPS_DIR'; mv '$REMOTE_REGISTRY.tmp' '$REMOTE_REGISTRY'; sync"
    device_shell "install -d -m 0700 '$REMOTE_RUN_DIR'"
    "$ADB" "${adb_args[@]}" push "$tmp" "$REMOTE_RUN_DIR/$APP_NAME.json" >/dev/null
    rm -f "$tmp"
}

deploy() {
    local host_sha remote_staged remote_binary versions_file
    if ! valid_app; then
        usage
        return 2
    fi
    if [[ -z "$BINARY" || ! -f "$BINARY" ]]; then
        printf 'usage: %s deploy %s PATH_TO_BINARY\n' "$0" "$APP_NAME" >&2
        return 2
    fi
    host_sha=$(sha256sum "$BINARY" | awk '{print $1}')
    if ! valid_sha "$host_sha"; then
        printf 'host ELF SHA-256 is invalid\n' >&2
        return 1
    fi
    read_registry
    versions_file=$(mktemp)
    pushed_protocol_versions "$APP_NAME" >"$versions_file"
    if ! check_compatibility "$versions_file"; then
        rm -f "$versions_file"
        return 1
    fi
    rm -f "$versions_file"

    remote_binary="$REMOTE_APPS_DIR/$APP_NAME/$host_sha"
    if [[ "$APP_NAME" != "router" ]] && device_shell_rc "test -s '$ROUTER_READY_MARKER'"; then
        ROUTER_WAS_READY=true
    fi

    device_shell "install -d -m 0700 '$REMOTE_APPS_DIR/$APP_NAME'"
    "$ADB" "${adb_args[@]}" push "$BINARY" "$remote_binary.tmp" >/dev/null
    remote_staged=$(device_shell "sha256sum '$remote_binary.tmp'" | tr -d '\r' | awk '{print $1}')
    if [[ "$remote_staged" != "$host_sha" ]]; then
        printf 'staged ELF SHA-256 mismatch: expected %s, got %s\n' "$host_sha" "$remote_staged" >&2
        return 1
    fi
    device_shell "chmod 0755 '$remote_binary.tmp'; mv '$remote_binary.tmp' '$remote_binary'; sync"
    printf '%s staged: %s\n' "$APP_NAME" "$host_sha"

    swap_binary "$remote_binary" "$host_sha"
    write_registry "$host_sha"
    printf '%s active: %s\n' "$APP_NAME" "$host_sha"
}

revert() {
    local previous_sha remote_previous versions_file
    if ! valid_app; then
        usage
        return 2
    fi
    read_registry
    previous_sha=$(registry_previous_sha "$APP_NAME")
    if [[ -z "$previous_sha" ]]; then
        printf 'no previous deployment of %s is recorded; nothing to revert to\n' "$APP_NAME" >&2
        return 1
    fi
    versions_file=$(mktemp)
    previous_protocol_versions >"$versions_file"
    if ! check_compatibility "$versions_file"; then
        printf 'the reverted %s is protocol-incompatible with its peers; not downgrading\n' "$APP_NAME" >&2
        rm -f "$versions_file"
        return 1
    fi
    rm -f "$versions_file"
    remote_previous="$REMOTE_APPS_DIR/$APP_NAME/$previous_sha"
    if ! device_shell_rc "test -x '$remote_previous'"; then
        printf 'previous %s binary is not present on the device: %s\n' "$APP_NAME" "$remote_previous" >&2
        return 1
    fi
    if [[ "$APP_NAME" != "router" ]] && device_shell_rc "test -s '$ROUTER_READY_MARKER'"; then
        ROUTER_WAS_READY=true
    fi
    swap_binary "$remote_previous" "$previous_sha"
    write_registry "$previous_sha"
    printf '%s reverted to: %s\n' "$APP_NAME" "$previous_sha"
}

check() {
    local versions_file
    if ! valid_app; then
        usage
        return 2
    fi
    read_registry
    versions_file=$(mktemp)
    pushed_protocol_versions "$APP_NAME" >"$versions_file"
    if ! check_compatibility "$versions_file"; then
        rm -f "$versions_file"
        return 1
    fi
    rm -f "$versions_file"
    printf '%s deployment is protocol-compatible\n' "$APP_NAME"
}

wait_for_adb
case "$ACTION" in
    deploy)
        deploy
        ;;
    revert)
        revert
        ;;
    check)
        check
        ;;
    *)
        usage
        exit 2
        ;;
esac
