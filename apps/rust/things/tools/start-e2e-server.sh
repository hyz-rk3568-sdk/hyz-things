#!/usr/bin/env bash
set -Eeuo pipefail

readonly SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly PACKAGE_DIR="$(CDPATH= cd -- "${SCRIPT_DIR}/.." && pwd)"
readonly WORKSPACE_DIR="$(CDPATH= cd -- "${PACKAGE_DIR}/../../.." && pwd)"
readonly FRONTEND_TAR="${WORKSPACE_DIR}/target/frontend-bundle/hyz-things-frontend.tar"
readonly WEB_PORT="${HYZ_THINGS_E2E_WEB_PORT:-3190}"
readonly CONTROL_PORT="${HYZ_THINGS_E2E_CONTROL_PORT:-3191}"
readonly REUSE_FRONTEND_BUNDLE="${HYZ_THINGS_E2E_REUSE_FRONTEND_BUNDLE:-0}"

if ! command -v cargo >/dev/null 2>&1; then
    printf 'error: required command not found: cargo\n' >&2
    exit 127
fi

case "${REUSE_FRONTEND_BUNDLE}" in
    0)
        for command_name in npm trunk; do
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
        ;;
    1)
        if [[ ! -s "${FRONTEND_TAR}" ]]; then
            printf 'error: verified frontend bundle is missing: %s\n' "${FRONTEND_TAR}" >&2
            exit 1
        fi
        printf 'Reusing verified frontend bundle: %s\n' "${FRONTEND_TAR}"
        ;;
    *)
        printf 'error: HYZ_THINGS_E2E_REUSE_FRONTEND_BUNDLE must be 0 or 1\n' >&2
        exit 2
        ;;
esac

exec cargo run --locked \
    --manifest-path "${PACKAGE_DIR}/Cargo.toml" \
    --features e2e \
    --bin hyz-things-e2e \
    -- "${FRONTEND_TAR}" "${WEB_PORT}" "${CONTROL_PORT}"
