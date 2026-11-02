#!/usr/bin/env bash
set -euo pipefail
root=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
script=$root/sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S83hyz-question-bank
if [[ ! -f $script ]]; then
  # The device SDK overlay is not committed to this repository. Do not fail
  # ordinary host CI when it is absent; still test when an SDK is present.
  printf '%s\n' 'skipped: device SDK Question Bank init script unavailable on host'
  exit 0
fi
# Source the function declarations; commands are replaced by fake ports below.
source <(sed '/^case /,$d' "$script")
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
LOGFILE=$work/log
UMOUNT=$work/umount
cat >"$UMOUNT" <<SHIM
#!/bin/sh
echo called > '$work/unmounted'
SHIM
chmod +x "$UMOUNT"
load_server() { SERVER=192.168.8.202; PORT=1445; }
mounted_filesystem() { echo cifs; }
mounted_source() { echo //192.168.8.202/some-other-share; }
stop_mount
[[ ! -f $work/unmounted ]]
mounted_source() { echo //192.168.8.202/hyz-question-bank; }
stop_mount
[[ -f $work/unmounted ]]
printf '%s\n' 'question-bank mount only unmounts the exact configured share'
