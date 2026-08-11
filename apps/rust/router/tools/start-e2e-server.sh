#!/usr/bin/env bash
set -Eeuo pipefail

readonly SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly PACKAGE_DIR="$(CDPATH= cd -- "${SCRIPT_DIR}/.." && pwd)"
readonly WORKSPACE_DIR="$(CDPATH= cd -- "${PACKAGE_DIR}/../../.." && pwd)"
readonly FRONTEND_TAR="${WORKSPACE_DIR}/target/frontend-bundle/router-frontend.tar"
readonly WEB_PORT="${ROUTER_E2E_WEB_PORT:-3190}"
readonly CONTROL_PORT="${ROUTER_E2E_CONTROL_PORT:-3191}"

for command_name in cargo npm trunk; do
    if ! command -v "${command_name}" >/dev/null 2>&1; then
        printf 'error: required command not found: %s\n' "${command_name}" >&2
        exit 127
    fi
done
if [[ ! -d "${PACKAGE_DIR}/node_modules" ]]; then
    printf 'error: frontend dependencies are not installed\n' >&2
    printf 'hint: run: (cd %s && npm ci)\n' "${PACKAGE_DIR}" >&2
    exit 1
fi

"${SCRIPT_DIR}/build-frontend-bundle.sh"

exec cargo run --locked \
    --manifest-path "${PACKAGE_DIR}/Cargo.toml" \
    --features e2e \
    --bin router-web-e2e \
    -- "${FRONTEND_TAR}" "${WEB_PORT}" "${CONTROL_PORT}"
