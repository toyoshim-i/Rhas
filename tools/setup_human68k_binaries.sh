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

# --- run68 ---
# 1. Prefer locally available binary from sibling Rhlk repository.
RHLK_RUN68="${RHLK_RUN68:-/home/toyoshim/Work/Rhlk/external/run68x/build/run68}"
copy_local_if_exists "${RHLK_RUN68}" "${BIN_DIR}/run68" || true

# 2. Fallback: build from external/run68x submodule (for CI).
if [[ ! -f "${BIN_DIR}/run68" ]]; then
  RUN68X_DIR="${ROOT_DIR}/external/run68x"
  if [[ -f "${RUN68X_DIR}/CMakeLists.txt" ]]; then
    echo "Building run68x from submodule..."
    cmake -S "${RUN68X_DIR}" -B "${RUN68X_DIR}/build"
    cmake --build "${RUN68X_DIR}/build"
    cp -f "${RUN68X_DIR}/build/run68" "${BIN_DIR}/run68"
  fi
fi

if [[ ! -f "${BIN_DIR}/run68" ]]; then
  echo "ERROR: run68 binary not found. Please place it at ${BIN_DIR}/run68." >&2
  exit 1
fi
chmod +x "${BIN_DIR}/run68"

# --- HAS060.X ---
# 1. Prefer locally available binary from sibling Rhlk repository.
RHLK_HAS="${RHLK_HAS:-/home/toyoshim/Work/Rhlk/external/toolchain/bin/has060x.x}"
copy_local_if_exists "${RHLK_HAS}" "${HAS_TEST_DIR}/HAS060.X" || true

# 2. Fallback: download from official release.
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

echo "Installed:"
echo "  run68: ${BIN_DIR}/run68"
echo "  HAS  : ${HAS_TEST_DIR}/HAS060.X"
