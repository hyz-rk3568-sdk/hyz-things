#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
# shellcheck source=share-flashcards.sh
source "$SCRIPT_DIR/share-flashcards.sh"

for address in \
    100.89.103.59 \
    100.109.217.61 \
    192.168.8.1 \
    192.168.8.202 \
    192.168.8.254; do
    valid_share_ipv4 "$address" || {
        printf 'expected valid share IPv4: %s\n' "$address" >&2
        exit 1
    }
done

for address in \
    10.0.0.1 \
    192.168.7.202 \
    192.168.8.0 \
    192.168.8.255 \
    100.256.1.1 \
    100.89.103; do
    if valid_share_ipv4 "$address"; then
        printf 'expected invalid share IPv4: %s\n' "$address" >&2
        exit 1
    fi
done

if grep -Eq '^[[:space:]]*hosts (allow|deny)[[:space:]]*=' "$SCRIPT_DIR/share-flashcards.sh"; then
    printf '%s\n' 'expected flashcard share to allow authenticated clients without an IP ACL' >&2
    exit 1
fi
if ! grep -Eq '^[[:space:]]*browseable[[:space:]]*=[[:space:]]*yes$' "$SCRIPT_DIR/share-flashcards.sh"; then
    printf '%s\n' 'expected flashcard share to be browseable for SMB clients' >&2
    exit 1
fi

printf '%s\n' 'share-flashcards access policy validation passed'
printf '%s\n' 'share-flashcards IPv4 validation passed'
