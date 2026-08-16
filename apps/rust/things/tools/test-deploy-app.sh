#!/usr/bin/env bash

# Host-only tests for deploy-app.sh. No device, no ADB: a fake adb stub
# serves a fixture registry and records every shell command so the tests can
# prove that a camera/things push never invokes the router init script, that
# the protocol-compatibility gate refuses a push before any service is
# stopped, and that the compatibility matrix follows the wire contracts.

set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
DEPLOY="$SCRIPT_DIR/deploy-app.sh"
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

FAKE_ADB="$WORK/adb"
FAKE_REGISTRY="$WORK/registry.json"
FAKE_DEVICE_ROOT="$WORK/device"
FAKE_LOG="$WORK/adb.log"
mkdir -p "$FAKE_DEVICE_ROOT"

SHA=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa

mkdir -p "$WORK/contract/src" "$WORK/things/src/adapters/outbound"
write_constants() {
    cat >"$WORK/contract/src/router.rs" <<'EOF'
pub const PROTOCOL_VERSION: u16 = 10;
EOF
    cat >"$WORK/contract/src/camera.rs" <<'EOF'
pub const CONTROL_PROTOCOL_VERSION: u16 = 2;
EOF
    cat >"$WORK/things/src/adapters/outbound/camera.rs" <<'EOF'
const CAMERA_CONTROL_PROTOCOL_VERSION: u16 = 2;
EOF
}

cat >"$FAKE_REGISTRY" <<EOF
{"apps": {"things": {"current": {"binary": "/usr/bin/hyz-things", "init_script": "/etc/init.d/S83hyz-things", "sha256": "$SHA", "protocol_versions": {"camera": 2, "router": 10}}}, "router": {"current": {"binary": "/usr/bin/hyz-router", "init_script": "/etc/init.d/S81hyz-router", "sha256": "$SHA", "protocol_versions": {"router": 10}}}, "camera": {"current": {"binary": "/usr/bin/hyz-camera", "init_script": "/etc/init.d/S82hyz-camera", "sha256": "$SHA", "protocol_versions": {"camera": 2}}}}}
EOF

cat >"$FAKE_ADB" <<'EOF'
#!/usr/bin/env bash
# Fake adb: serves the fixture registry, emulates file push and sha256sum,
# and records every command so the harness can assert service isolation.
log() {
    printf '%s\n' "$*" >>"$FAKE_LOG"
}

case "${1:-}" in
    get-state)
        exit 0
        ;;
    shell)
        cmd="${*:2}"
        case "$cmd" in
            *cat*registry.json*)
                cat "$FAKE_REGISTRY"
                ;;
            *install\ -d*)
                log "shell $cmd"
                ;;
            *stop*)
                log "shell $cmd"
                ;;
            *start*)
                log "shell $cmd"
                ;;
            *install\ -m*)
                log "shell $cmd"
                ;;
            *chmod*)
                log "shell $cmd"
                ;;
            *mv\ '*'*)
                src=$(printf '%s' "$cmd" | sed -E "s/.*mv '([^']+)' '([^']+)'.*/\1/")
                dst=$(printf '%s' "$cmd" | sed -E "s/.*mv '([^']+)' '([^']+)'.*/\2/")
                mkdir -p "$(dirname "$FAKE_DEVICE_ROOT$dst")"
                mv "$FAKE_DEVICE_ROOT$src" "$FAKE_DEVICE_ROOT$dst"
                log "shell $cmd"
                ;;
            *sync*)
                log "shell $cmd"
                ;;
            *sha256sum*)
                path=$(printf '%s' "$cmd" | sed -E "s/.*sha256sum '([^']+)'.*/\1/")
                if [[ -f "$FAKE_DEVICE_ROOT$path" ]]; then
                    sha256sum "$FAKE_DEVICE_ROOT$path" | awk '{print $1}'
                else
                    printf 'fake device file missing: %s\n' "$path" >&2
                    exit 1
                fi
                ;;
            *wget*)
                log "shell $cmd"
                ;;
            *test\ -x*)
                if [[ -n "${FAKE_NO_CAMERA:-}" && "$cmd" == *hyz-camera* ]]; then
                    exit 1
                fi
                exit 0
                ;;
            *test\ -s*)
                exit 0
                ;;
            *control.sock*)
                log "shell $cmd"
                ;;
            */run/hyz-router/ready*)
                log "shell $cmd"
                ;;
            *)
                printf 'unhandled fake adb shell: %s\n' "$cmd" >&2
                exit 1
                ;;
        esac
        ;;
    push)
        mkdir -p "$(dirname "$FAKE_DEVICE_ROOT$3")"
        cp "$2" "$FAKE_DEVICE_ROOT$3"
        log "push $3"
        ;;
    *)
        printf 'unhandled fake adb: %s\n' "$*" >&2
        exit 1
        ;;
esac
EOF
chmod 0755 "$FAKE_ADB"

run_deploy() { # expect args...
    local expect=$1
    shift
    if [[ "$expect" == ok ]]; then
        if ! ADB="$FAKE_ADB" FAKE_REGISTRY="$FAKE_REGISTRY" FAKE_DEVICE_ROOT="$FAKE_DEVICE_ROOT" \
            FAKE_LOG="$FAKE_LOG" FAKE_NO_CAMERA="${FAKE_NO_CAMERA:-}" \
            CONTRACT_DIR="$WORK/contract" THINGS_DIR="$WORK/things" \
            bash "$DEPLOY" "$@" >/dev/null 2>&1; then
            printf 'FAIL: expected success: %s\n' "$*" >&2
            exit 1
        fi
    else
        if ADB="$FAKE_ADB" FAKE_REGISTRY="$FAKE_REGISTRY" FAKE_DEVICE_ROOT="$FAKE_DEVICE_ROOT" \
            FAKE_LOG="$FAKE_LOG" FAKE_NO_CAMERA="${FAKE_NO_CAMERA:-}" \
            CONTRACT_DIR="$WORK/contract" THINGS_DIR="$WORK/things" \
            bash "$DEPLOY" "$@" >/dev/null 2>&1; then
            printf 'FAIL: expected rejection: %s\n' "$*" >&2
            exit 1
        fi
    fi
}

expect_log_contains() {
    if ! grep -q "$1" "$FAKE_LOG"; then
        printf 'FAIL: expected adb log to contain: %s\n' "$1" >&2
        exit 1
    fi
}

expect_log_absent() {
    if grep -q "$1" "$FAKE_LOG"; then
        printf 'FAIL: expected adb log to omit: %s\n' "$1" >&2
        exit 1
    fi
}

reset_log() {
    : >"$FAKE_LOG"
}

write_constants

# Usage validation.
run_deploy fail
run_deploy fail check bogus
run_deploy fail deploy bogus /bin/true

# Compatible baseline: every check passes.
run_deploy ok check things
run_deploy ok check router
run_deploy ok check camera

# A missing camera peer is skipped on the things edge but not the router edge.
FAKE_NO_CAMERA=1 run_deploy ok check things
FAKE_NO_CAMERA=1 run_deploy ok check camera

# A camera push stops only the camera service and never the router.
reset_log
run_deploy ok deploy camera "$DEPLOY"
expect_log_contains "'/etc/init.d/S82hyz-camera' stop"
expect_log_contains "'/etc/init.d/S82hyz-camera' start"
expect_log_absent 'S81hyz-router'
expect_log_absent 'S83hyz-things'
expect_log_contains 'push /userdata/hyz-things/apps/registry.json.tmp'

# A things push stops only the things service and never the router.
reset_log
run_deploy ok deploy things "$DEPLOY"
expect_log_contains "'/etc/init.d/S83hyz-things' stop"
expect_log_contains "'/etc/init.d/S83hyz-things' start"
expect_log_absent 'S81hyz-router'
expect_log_absent 'S82hyz-camera'

# A router push restarts only the router core.
reset_log
run_deploy ok deploy router "$DEPLOY"
expect_log_contains "'/etc/init.d/S81hyz-router' stop"
expect_log_contains "'/etc/init.d/S81hyz-router' start"
expect_log_absent 'S82hyz-camera'
expect_log_absent 'S83hyz-things'

# A protocol mismatch is refused before any service is stopped.
sed -i 's/u16 = 2/u16 = 3/' "$WORK/contract/src/camera.rs"
reset_log
run_deploy fail deploy camera "$DEPLOY"
expect_log_absent 'stop'
expect_log_absent 'start'
sed -i 's/u16 = 3/u16 = 2/' "$WORK/contract/src/camera.rs"

# A contract bump breaks every client edge and the router server edge.
sed -i 's/u16 = 10/u16 = 11/' "$WORK/contract/src/router.rs"
run_deploy fail check things
run_deploy fail check router
run_deploy ok check camera
sed -i 's/u16 = 2/u16 = 3/' "$WORK/contract/src/camera.rs"
run_deploy fail check things
run_deploy fail check camera
sed -i 's/u16 = 11/u16 = 10/' "$WORK/contract/src/router.rs"
run_deploy ok check things
run_deploy ok check router
run_deploy fail check camera

# A drift of things' own camera expectation is caught on both edges.
sed -i 's/u16 = 2/u16 = 3/' "$WORK/things/src/adapters/outbound/camera.rs"
run_deploy fail check things
run_deploy fail check camera
sed -i 's/u16 = 3/u16 = 2/' "$WORK/things/src/adapters/outbound/camera.rs"

# A revert is gated on the previous binary's recorded protocol versions, not
# the current source tree: source camera is 3 here, but the recorded previous
# binary speaks 2 while things expects 3, so the revert is refused.
cat >"$FAKE_REGISTRY" <<EOF
{"apps": {"things": {"current": {"binary": "/usr/bin/hyz-things", "init_script": "/etc/init.d/S83hyz-things", "sha256": "$SHA", "protocol_versions": {"camera": 3, "router": 10}}}, "camera": {"current": {"binary": "/usr/bin/hyz-camera", "init_script": "/etc/init.d/S82hyz-camera", "sha256": "$(printf 'b%.0s' {1..64})", "protocol_versions": {"camera": 3}}, "previous": {"binary": "/usr/bin/hyz-camera", "init_script": "/etc/init.d/S82hyz-camera", "sha256": "$SHA", "protocol_versions": {"camera": 2}}}}}
EOF
reset_log
run_deploy fail revert camera
expect_log_absent 'stop'
expect_log_absent 'start'

# A camera revert restores the recorded previous binary without the router.
sed -i 's/u16 = 3/u16 = 2/' "$WORK/contract/src/camera.rs"
cat >"$FAKE_REGISTRY" <<EOF
{"apps": {"camera": {"current": {"binary": "/usr/bin/hyz-camera", "init_script": "/etc/init.d/S82hyz-camera", "sha256": "$(printf 'b%.0s' {1..64})", "protocol_versions": {"camera": 2}}, "previous": {"binary": "/usr/bin/hyz-camera", "init_script": "/etc/init.d/S82hyz-camera", "sha256": "$SHA", "protocol_versions": {"camera": 2}}}}}
EOF
reset_log
run_deploy ok revert camera
expect_log_contains "'/etc/init.d/S82hyz-camera' stop"
expect_log_contains "'/etc/init.d/S82hyz-camera' start"
expect_log_absent 'S81hyz-router'
expect_log_absent 'S83hyz-things'

# Reverting with no recorded previous binary is refused.
cat >"$FAKE_REGISTRY" <<EOF
{"apps": {"camera": {"current": {"binary": "/usr/bin/hyz-camera", "init_script": "/etc/init.d/S82hyz-camera", "sha256": "$SHA", "protocol_versions": {"camera": 2}}}}}
EOF
reset_log
run_deploy fail revert camera
expect_log_absent 'stop'
expect_log_absent 'start'

echo 'deploy-app.sh tests passed'
