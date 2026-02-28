#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "${ROOT_DIR}"

SRC_DIR="${SRC_DIR:-external/has060xx/src}"
EXTRA_DIR="${EXTRA_DIR:-tests/compat_src}"
RHAS_BIN="${RHAS_BIN:-target/debug/rhas}"
RUN68_BIN="${RUN68_BIN:-tools/bin/run68}"
HAS="${HAS:-/tmp/has_test/HAS060.X}"
RUN68_BIN="$(cd "$(dirname "${RUN68_BIN}")" && pwd)/$(basename "${RUN68_BIN}")"
HAS="$(cd "$(dirname "${HAS}")" && pwd)/$(basename "${HAS}")"

if [[ ! -x "${RHAS_BIN}" ]]; then
  echo "ERROR: rhas not found: ${RHAS_BIN}" >&2
  exit 1
fi
if [[ ! -x "${RUN68_BIN}" ]]; then
  echo "ERROR: run68 not found: ${RUN68_BIN}" >&2
  exit 1
fi
if [[ ! -f "${HAS}" ]]; then
  echo "ERROR: HAS060.X not found: ${HAS}" >&2
  exit 1
fi

BASE_FILES=(
  commitlog doasm eamode encode error2 expr fexpr file
  hupair macro objgen opname optimize pseudo regname symbol work
)
EXTRA_FILES=(
  ms6_fpu_real
  ms6_scd_real
)

TMP="$(mktemp -d)"
trap 'rm -rf "${TMP}"' EXIT
mkdir -p "${TMP}/orig" "${TMP}/rhas"
cp "${SRC_DIR}"/* "${TMP}/orig/" 2>/dev/null || true
cp "${EXTRA_DIR}"/*.s "${TMP}/orig/" 2>/dev/null || true

ok=0
diffs=0

run_case() {
  local src="$1"
  local base="$2"

  "${RHAS_BIN}" -c4 -u -w0 -i"${SRC_DIR}" -i"${EXTRA_DIR}" "${src}" -o "${TMP}/rhas/${base}.o" \
    >"/tmp/rhas_${base}.log" 2>&1 || true
  (cd "${TMP}/orig" && "${RUN68_BIN}" "${HAS}" -c4 -u -w0 "${base}.s" \
    >"/tmp/has_${base}.log" 2>&1) || true

  local ro="${TMP}/rhas/${base}.o"
  local oo="${TMP}/orig/${base}.o"
  if [[ ! -f "${ro}" || ! -f "${oo}" ]]; then
    echo "MISS ${base} rhas=$(test -f "${ro}" && echo yes || echo no) has=$(test -f "${oo}" && echo yes || echo no)"
    diffs=$((diffs + 1))
    return
  fi

  if cmp -s "${oo}" "${ro}"; then
    echo "OK   ${base}"
    ok=$((ok + 1))
  else
    local osz rsz
    osz="$(wc -c < "${oo}" | tr -d ' ')"
    rsz="$(wc -c < "${ro}" | tr -d ' ')"
    echo "DIFF ${base} has=${osz} rhas=${rsz} delta=$((rsz-osz))"
    diffs=$((diffs + 1))
  fi
}

for base in "${BASE_FILES[@]}"; do
  src="${SRC_DIR}/${base}.s"
  if [[ ! -f "${src}" ]]; then
    echo "SKIP ${base}: source missing"
    continue
  fi
  run_case "${src}" "${base}"
done

for base in "${EXTRA_FILES[@]}"; do
  src="${EXTRA_DIR}/${base}.s"
  if [[ ! -f "${src}" ]]; then
    echo "SKIP ${base}: source missing"
    continue
  fi
  run_case "${src}" "${base}"
done

echo "RESULT ok=${ok} diff=${diffs}"
