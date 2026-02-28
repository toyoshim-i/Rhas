#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
BIN_DIR="${ROOT_DIR}/tools/bin"
HAS_TEST_DIR="${HAS_TEST_DIR:-/tmp/has_test}"

mkdir -p "${BIN_DIR}" "${HAS_TEST_DIR}"

copy_local_if_exists() {
  local src="$1"
  local dst="$2"
  if [[ -f "${src}" ]]; then
    cp -f "${src}" "${dst}"
    return 0
  fi
  return 1
}

echo "Setting up Human68k tool binaries..."

# Prefer locally available binaries from sibling repository.
copy_local_if_exists \
  "/home/toyoshim/Work/Rhlk/external/run68x/build/run68" \
  "${BIN_DIR}/run68" || true
copy_local_if_exists \
  "/home/toyoshim/Work/Rhlk/external/toolchain/bin/has060x.x" \
  "${HAS_TEST_DIR}/HAS060.X" || true

# Optional fallback: download HAS060X.X from official release if local copy is unavailable.
if [[ ! -f "${HAS_TEST_DIR}/HAS060.X" ]]; then
  TMP_DIR="$(mktemp -d)"
  trap 'rm -rf "${TMP_DIR}"' EXIT
  ZIP="${TMP_DIR}/hasx125.zip"
  curl -fsSL -o "${ZIP}" \
    "https://github.com/kg68k/has060xx/releases/download/v1.2.5/hasx125.zip"
  ENTRY="$(unzip -Z1 "${ZIP}" | rg -i '^has060x\.x$' | head -n1 || true)"
  if [[ -z "${ENTRY}" ]]; then
    echo "ERROR: has060x.x not found in release archive" >&2
    exit 1
  fi
  unzip -p "${ZIP}" "${ENTRY}" > "${HAS_TEST_DIR}/HAS060.X"
fi

if [[ ! -f "${BIN_DIR}/run68" ]]; then
  echo "ERROR: run68 binary not found. Please place it at ${BIN_DIR}/run68." >&2
  exit 1
fi

chmod +x "${BIN_DIR}/run68"

echo "Installed:"
echo "  run68: ${BIN_DIR}/run68"
echo "  HAS  : ${HAS_TEST_DIR}/HAS060.X"
