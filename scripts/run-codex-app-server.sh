#!/usr/bin/env bash
# Run on the PC at 192.168.8.202, never on RK3568.
set -euo pipefail
TOKEN_FILE="$HOME/.codex/app-server-token"
if ! command -v codex >/dev/null 2>&1; then
  echo "Install the official Codex CLI on the PC first" >&2
  exit 1
fi
if [[ ! -f "$TOKEN_FILE" || -L "$TOKEN_FILE" ]]; then
  echo "Missing token: umask 077; openssl rand -hex 32 > \"$TOKEN_FILE\"" >&2
  exit 1
fi
if [[ "$(stat -c %a "$TOKEN_FILE")" != 600 ]]; then
  echo "Run: chmod 600 \"$TOKEN_FILE\"" >&2
  exit 1
fi
TOKEN="$(tr -d '\r\n' < "$TOKEN_FILE")"
if [[ ! "$TOKEN" =~ ^[0-9a-fA-F]{64}$ ]]; then
  echo "Token must be 64 hex characters" >&2
  exit 1
fi
unset TOKEN
exec codex app-server --listen ws://0.0.0.0:1457 \
    --ws-auth capability-token --ws-token-file "$TOKEN_FILE"
