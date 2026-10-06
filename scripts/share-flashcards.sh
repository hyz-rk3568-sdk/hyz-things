#!/usr/bin/env bash
set -euo pipefail

# Configure the host Samba share consumed by /etc/init.d/S83hyz-cards.
# Run from the repository with sudo; the default action is idempotent.

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
REPO_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd -P)
SHARE_DIR=${HYZ_CARDS_SHARE_DIR:-$REPO_ROOT/flashcards}
HOST_IP=${HYZ_CARDS_HOST_IP:-192.168.8.202}
SMB_PORT=${HYZ_CARDS_SMB_PORT:-1445}
SHARE_NAME=${HYZ_CARDS_SHARE_NAME:-hyz-cards}
SMB_USER=${HYZ_CARDS_SMB_USER:-${SUDO_USER:-hyz}}
CONFIG_FILE=${HYZ_CARDS_SAMBA_CONFIG:-/etc/samba/smb.conf}
GLOBAL_MARKER_BEGIN='# BEGIN hyz-things cards global'
GLOBAL_MARKER_END='# END hyz-things cards global'
MARKER_BEGIN='# BEGIN hyz-things cards share'
MARKER_END='# END hyz-things cards share'

usage() {
    cat <<'USAGE'
Usage:
  sudo scripts/share-flashcards.sh [--install]
  sudo scripts/share-flashcards.sh --status
  sudo scripts/share-flashcards.sh --remove

The default action configures the persistent Samba share on TCP port 1445 and enables smbd.
--install installs samba and smbclient with apt when they are missing.
--remove removes only the blocks managed by this script.
USAGE
}

fail() {
    printf 'ERROR: %s\n' "$*" >&2
    exit 1
}

require_root() {
    [ "$(id -u)" -eq 0 ] || fail 'run this script with sudo'
}

valid_share_ipv4() {
    local address=$1
    local first second third fourth extra octet
    local IFS=.

    read -r first second third fourth extra <<<"$address"
    [ -n "$first" ] && [ -n "$second" ] && [ -n "$third" ] && [ -n "$fourth" ] || return 1
    [ -z "${extra:-}" ] || return 1

    for octet in "$first" "$second" "$third" "$fourth"; do
        case "$octet" in
            ''|*[!0-9]*)
                return 1
                ;;
        esac
        if ((10#$octet > 255)); then
            return 1
        fi
    done
    if ((10#$fourth < 1 || 10#$fourth > 254)); then
        return 1
    fi

    case "$first.$second.$third" in
        100.*|192.168.8)
            return 0
            ;;
        *)
            return 1
            ;;
    esac
}

require_valid_inputs() {
    [ -d "$SHARE_DIR" ] || fail "flashcard directory does not exist: $SHARE_DIR"
    [ ! -L "$SHARE_DIR" ] || fail "flashcard directory must not be a symlink: $SHARE_DIR"
    valid_share_ipv4 "$HOST_IP" || fail "host IP must be a Tailscale 100.0.0.0/8 or LAN 192.168.8.0/24 IPv4 address: $HOST_IP"
    case "$SMB_PORT" in
        ''|*[!0-9]*) fail "SMB port must be numeric: $SMB_PORT" ;;
    esac
    [ "$SMB_PORT" -ge 1 ] 2>/dev/null && [ "$SMB_PORT" -le 65535 ] 2>/dev/null || fail "SMB port must be between 1 and 65535: $SMB_PORT"
    [ -n "$SHARE_NAME" ] || fail 'share name must not be empty'
    printf '%s\n' "$SHARE_NAME" | grep -Eq '^[A-Za-z0-9_.-]+$' || fail "invalid share name: $SHARE_NAME"
    [ -n "$SMB_USER" ] || fail 'Samba user must not be empty'
    printf '%s\n' "$SMB_USER" | grep -Eq '^[A-Za-z_][A-Za-z0-9_.-]*\$?$' || fail "invalid Samba user: $SMB_USER"
    getent passwd "$SMB_USER" >/dev/null || fail "Unix user does not exist: $SMB_USER"
}

ensure_samba_tools() {
    if command -v smbd >/dev/null 2>&1 && \
       command -v testparm >/dev/null 2>&1 && \
       command -v smbpasswd >/dev/null 2>&1; then
        return
    fi

    [ "${INSTALL_SAMBA:-0}" = 1 ] || fail 'Samba is not installed; rerun with --install or install samba and smbclient first'
    command -v apt-get >/dev/null 2>&1 || fail 'apt-get is required for --install'
    apt-get update
    DEBIAN_FRONTEND=noninteractive apt-get install -y samba smbclient
}

ensure_config_file() {
    mkdir -p "$(dirname -- "$CONFIG_FILE")"
    if [ ! -e "$CONFIG_FILE" ]; then
        cat >"$CONFIG_FILE" <<'CONF'
[global]
   workgroup = WORKGROUP
   server role = standalone server
   security = user
   map to guest = Never
CONF
        chmod 0644 "$CONFIG_FILE"
        chown root:root "$CONFIG_FILE"
    fi
    [ -f "$CONFIG_FILE" ] || fail "Samba configuration is not a regular file: $CONFIG_FILE"
    [ ! -L "$CONFIG_FILE" ] || fail "Samba configuration must not be a symlink: $CONFIG_FILE"
}

backup_config() {
    local backup_dir backup_file
    backup_dir=/var/backups/samba
    mkdir -p "$backup_dir"
    chmod 0750 "$backup_dir"
    backup_file="$backup_dir/smb.conf.hyz-things.$(date +%Y%m%d-%H%M%S).bak"
    cp -a "$CONFIG_FILE" "$backup_file"
    chmod 0600 "$backup_file"
    printf 'Backed up Samba configuration to %s\n' "$backup_file"
}

render_config_without_managed_block() {
    local destination=$1
    awk -v global_begin="$GLOBAL_MARKER_BEGIN" \
        -v global_end="$GLOBAL_MARKER_END" \
        -v begin="$MARKER_BEGIN" \
        -v end="$MARKER_END" '
        $0 == global_begin || $0 == begin {
            in_block = 1
            next
        }
        in_block && ($0 == global_end || $0 == end) {
            in_block = 0
            next
        }
        !in_block { print }
        END {
            if (in_block) exit 2
        }
    ' "$CONFIG_FILE" >"$destination" || fail "unclosed managed block in $CONFIG_FILE"
}

write_share_block() {
    local destination=$1
    cat >>"$destination" <<CONF

$GLOBAL_MARKER_BEGIN
[global]
   smb ports = $SMB_PORT
$GLOBAL_MARKER_END

$MARKER_BEGIN
[$SHARE_NAME]
   path = $SHARE_DIR
   browseable = yes
   read only = no
   guest ok = no
   valid users = $SMB_USER
   force user = $SMB_USER
$MARKER_END
CONF
}

install_config() {
    local temporary
    temporary=$(mktemp "$(dirname -- "$CONFIG_FILE")/.smb.conf.hyz-things.XXXXXX")
    chmod 0600 "$temporary"
    trap 'rm -f "$temporary"' RETURN
    render_config_without_managed_block "$temporary"
    write_share_block "$temporary"
    testparm -s "$temporary" >/dev/null || fail "testparm rejected the generated Samba configuration"
    install -o root -g root -m 0644 "$temporary" "$CONFIG_FILE"
    rm -f "$temporary"
    trap - RETURN
}

ensure_samba_account() {
    if ! pdbedit -L 2>/dev/null | awk -F: -v user="$SMB_USER" '$1 == user {found = 1} END {exit !found}'; then
        printf 'No Samba account exists for %s. Set its Samba password now.\n' "$SMB_USER"
        smbpasswd -a "$SMB_USER"
    fi
    smbpasswd -e "$SMB_USER" >/dev/null
}

restart_samba() {
    if command -v systemctl >/dev/null 2>&1; then
        systemctl enable smbd
        systemctl restart smbd
    elif command -v service >/dev/null 2>&1; then
        service smbd restart
    else
        fail 'neither systemctl nor service is available to start smbd'
    fi
}

remove_share() {
    require_root
    ensure_samba_tools
    ensure_config_file
    backup_config
    local temporary
    temporary=$(mktemp "$(dirname -- "$CONFIG_FILE")/.smb.conf.hyz-things.XXXXXX")
    chmod 0600 "$temporary"
    trap 'rm -f "$temporary"' RETURN
    render_config_without_managed_block "$temporary"
    testparm -s "$temporary" >/dev/null || fail "testparm rejected the configuration after removing the share"
    install -o root -g root -m 0644 "$temporary" "$CONFIG_FILE"
    rm -f "$temporary"
    trap - RETURN
    if command -v systemctl >/dev/null 2>&1; then
        systemctl restart smbd
    elif command -v service >/dev/null 2>&1; then
        service smbd restart
    fi
    printf 'Removed the %s share block from %s\n' "$SHARE_NAME" "$CONFIG_FILE"
}

show_status() {
    require_root
    ensure_samba_tools
    ensure_config_file
    testparm -s "$CONFIG_FILE" >/dev/null
    if command -v systemctl >/dev/null 2>&1; then
        systemctl --no-pager --full status smbd || true
    else
        service smbd status || true
    fi
    printf 'Share path: %s\n' "$SHARE_DIR"
    printf 'Share endpoint: //%s:%s/%s\n' "$HOST_IP" "$SMB_PORT" "$SHARE_NAME"
    printf 'Access control: Samba username and password\n'
    printf 'Share mode: read/write\n'
}

main() {
    local action=configure
    INSTALL_SAMBA=0

    while [ "$#" -gt 0 ]; do
        case "$1" in
            --install)
                INSTALL_SAMBA=1
                ;;
            --status)
                action=status
                ;;
            --remove)
                action=remove
                ;;
            -h|--help)
                usage
                return 0
                ;;
            *)
                usage >&2
                return 2
                ;;
        esac
        shift
    done

    case "$action" in
        status)
            show_status
            ;;
        remove)
            remove_share
            ;;
        configure)
            require_root
            require_valid_inputs
            ensure_samba_tools
            ensure_config_file
            backup_config
            install_config
            ensure_samba_account
            restart_samba
            testparm -s "$CONFIG_FILE" >/dev/null
            printf 'Samba share configured: //%s:%s/%s\n' "$HOST_IP" "$SMB_PORT" "$SHARE_NAME"
            printf 'Shared directory: %s\n' "$SHARE_DIR"
            printf 'Access control: Samba username and password\n'
            printf 'Test with: smbclient -p %s //%s/%s -U %s -c '\''ls'\''\n' "$SMB_PORT" "$HOST_IP" "$SHARE_NAME" "$SMB_USER"
            ;;
    esac
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
    main "$@"
fi
