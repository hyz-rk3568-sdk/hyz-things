#!/bin/bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 <destination-directory>" >&2
  exit 2
fi

destination=$1
commit=00d18d8f9eec132181f3968d10d1f608ff382ff5
base_url="https://raw.githubusercontent.com/MetaCubeX/meta-rules-dat/${commit}"

mkdir -p "${destination}"

stage_asset() {
  local name=$1
  local expected_size=$2
  local expected_sha256=$3
  local target="${destination}/${name}"
  local temporary="${target}.tmp"

  rm -f "${temporary}"
  curl --fail --location --proto '=https' --tlsv1.2 \
    "${base_url}/${name}" -o "${temporary}"

  local actual_size
  actual_size=$(wc -c <"${temporary}")
  if [[ "${actual_size}" != "${expected_size}" ]]; then
    echo "${name}: expected ${expected_size} bytes, got ${actual_size}" >&2
    rm -f "${temporary}"
    exit 1
  fi
  if ! printf '%s  %s\n' "${expected_sha256}" "${temporary}" | sha256sum --check --status; then
    echo "${name}: SHA-256 verification failed" >&2
    rm -f "${temporary}"
    exit 1
  fi
  chmod 0644 "${temporary}"
  mv -f "${temporary}" "${target}"
}

stage_asset geosite.dat 4244097 83e5023cfc134700fd373d800880c62c9cb1e8b97aef59052d7779d817c27be0
stage_asset country.mmdb 7824943 4790a1479e63c8d3b67af7b47f6cb0b96a8d05a6d01ed568af8385c1d1176c8e
