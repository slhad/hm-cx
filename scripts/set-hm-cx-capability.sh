#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
BIN_INPUT="${1:-target/debug/hm_cx}"
CAPS="cap_dac_read_search,cap_perfmon=ep"

if [[ "${BIN_INPUT}" = /* ]]; then
  BIN_PATH="${BIN_INPUT}"
else
  BIN_PATH="${REPO_DIR}/${BIN_INPUT}"
fi

SETCAP_BIN="$(command -v setcap 2>/dev/null || true)"
GETCAP_BIN="$(command -v getcap 2>/dev/null || true)"
SETCAP_BIN="${SETCAP_BIN:-/usr/sbin/setcap}"
GETCAP_BIN="${GETCAP_BIN:-/usr/sbin/getcap}"

if [[ ! -x "${BIN_PATH}" ]]; then
  echo "error: binary not found or not executable: ${BIN_PATH}" >&2
  exit 1
fi

if [[ ! -x "${SETCAP_BIN}" ]]; then
  echo "error: setcap not found" >&2
  exit 1
fi

if [[ ! -x "${GETCAP_BIN}" ]]; then
  echo "error: getcap not found" >&2
  exit 1
fi

echo "Applying ${CAPS} to ${BIN_PATH}"
sudo "${SETCAP_BIN}" "${CAPS}" "${BIN_PATH}"

echo "Current capabilities:"
"${GETCAP_BIN}" "${BIN_PATH}"
