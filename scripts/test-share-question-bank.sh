#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
source "$SCRIPT_DIR/share-question-bank.sh"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
CONFIG_FILE=$work/smb.conf
cat >"$CONFIG_FILE" <<'CONF'
[global]
  smb ports = 1445
# BEGIN hyz-things cards share
[hyz-cards]
  path = /example/flashcards
  read only = no
# END hyz-things cards share
CONF
before=$(cat "$CONFIG_FILE")
render_question_bank_config "$work/first"
[[ $(cat "$CONFIG_FILE") == "$before" ]]
grep -q '^\[hyz-cards\]' "$work/first"
grep -q '^\[hyz-question-bank\]' "$work/first"
[[ $(grep -c 'smb ports' "$work/first") == 1 ]]
CONFIG_FILE=$work/first
render_question_bank_config "$work/second"
cmp "$work/first" "$work/second"
printf '%s\n' 'question-bank second share preserves cards/global config and is idempotent'
