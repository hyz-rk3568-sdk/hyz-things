#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
# Reuse input/account/service validation, but never its global-block renderer.
source "$SCRIPT_DIR/share-flashcards.sh"
SHARE_DIR=${HYZ_QUESTION_BANK_SHARE_DIR:-$REPO_ROOT/question_bank}
SHARE_NAME=hyz-question-bank
QB_BEGIN='# BEGIN hyz-things question-bank share'
QB_END='# END hyz-things question-bank share'

strip_question_bank_block() {
    awk -v begin="$QB_BEGIN" -v end="$QB_END" '
      $0 == begin { if (inside) exit 2; inside=1; next }
      $0 == end { if (!inside) exit 2; inside=0; next }
      !inside { lines[++n]=$0 }
      END { if (inside) exit 2; while(n && lines[n] ~ /^[ \t]*$/) n--; for(i=1;i<=n;i++) print lines[i] }
    ' "$CONFIG_FILE" >"$1"
}

render_question_bank_config() {
    local destination=$1
    strip_question_bank_block "$destination"
    cat >>"$destination" <<CONF

$QB_BEGIN
[$SHARE_NAME]
   path = $SHARE_DIR
   browseable = yes
   read only = no
   guest ok = no
   valid users = $SMB_USER
   force user = $SMB_USER
   veto files = /.git/reports/
   delete veto files = no
$QB_END
CONF
}

install_question_bank_config() {
    [ -f "$CONFIG_FILE" ] && [ ! -L "$CONFIG_FILE" ] || fail 'configure hyz-cards first'
    local cards_path cards_users ports temporary
    cards_path=$(testparm -s --section-name=hyz-cards --parameter-name=path "$CONFIG_FILE" 2>/dev/null)
    cards_users=$(testparm -s --section-name=hyz-cards --parameter-name='valid users' "$CONFIG_FILE" 2>/dev/null)
    ports=$(testparm -s --parameter-name='smb ports' "$CONFIG_FILE" 2>/dev/null)
    [ -n "$cards_path" ] || fail 'existing hyz-cards share is required'
    [ "$cards_users" = "$SMB_USER" ] || fail 'use the same Samba user as hyz-cards'
    [[ " $ports " == *" $SMB_PORT "* ]] || fail 'use the same Samba port as hyz-cards'
    backup_config
    temporary=$(mktemp "$(dirname -- "$CONFIG_FILE")/.smb.conf.question-bank.XXXXXX")
    render_question_bank_config "$temporary"
    if ! testparm -s "$temporary" >/dev/null; then
        rm -f "$temporary"
        fail 'testparm rejected question-bank configuration'
    fi
    install -o root -g root -m 0644 "$temporary" "$CONFIG_FILE"
    rm -f "$temporary"
    restart_samba
}

question_bank_main() {
    case "${1:---configure}" in
      --configure)
        require_root
        require_valid_inputs
        ensure_samba_tools
        install_question_bank_config
        ;;
      --status) show_status ;;
      *) printf '%s\n' 'Usage: sudo scripts/share-question-bank.sh [--configure|--status]' >&2; return 2 ;;
    esac
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
    question_bank_main "$@"
fi
