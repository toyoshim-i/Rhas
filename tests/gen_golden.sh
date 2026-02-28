#!/bin/zsh
# Generate golden .o files using the original HAS060.X assembler via run68.
# Run this script from the repository root:
#   zsh tests/gen_golden.sh

set -euo pipefail

HAS="${HAS:-/private/tmp/has_test/HAS060.X}"
RUN68_BIN="${RUN68_BIN:-run68}"
ASM_DIR="$(cd "$(dirname "$0")/asm" && pwd)"
GOLDEN_DIR="$(cd "$(dirname "$0")/golden" && pwd)"
WORK_DIR=$(mktemp -d)

cleanup() { rm -rf "$WORK_DIR"; }
trap cleanup EXIT

if [[ ! -f "$HAS" ]]; then
    echo "ERROR: HAS060.X not found at $HAS" >&2
    exit 1
fi
if ! command -v "$RUN68_BIN" &>/dev/null; then
    echo "ERROR: run68 not found: $RUN68_BIN" >&2
    exit 1
fi

mkdir -p "$GOLDEN_DIR"

echo "Generating golden files..."
echo "  HAS:    $HAS"
echo "  ASM:    $ASM_DIR"
echo "  OUTPUT: $GOLDEN_DIR"
echo ""

OK=0; FAIL=0

for asm_file in "$ASM_DIR"/*.s; do
    name=$(basename "$asm_file" .s)
    golden="$GOLDEN_DIR/${name}.o"
    work_src="$WORK_DIR/${name}.s"
    work_out="$WORK_DIR/${name}.o"

    cp "$asm_file" "$work_src"

    printf "  %-24s ... " "$name"

    # HAS060.X (Human68k binary) only accepts bare filenames, not full paths.
    # Run from the work directory using the basename only; output goes there too.
    # Files ending with _opt use -c4 to enable extended optimizations.
    if [[ "$name" == *_opt ]]; then
        (cd "$WORK_DIR" && "$RUN68_BIN" "$HAS" -c4 -u -w0 "${name}.s" 2>/dev/null) || true
    else
        (cd "$WORK_DIR" && "$RUN68_BIN" "$HAS" -u -w0 "${name}.s" 2>/dev/null) || true
    fi

    if [[ -f "$work_out" ]]; then
        cp "$work_out" "$golden"
        echo "OK  ($(wc -c < "$golden" | tr -d ' ') bytes)"
        OK=$((OK + 1))
    else
        echo "FAILED (no output)"
        FAIL=$((FAIL + 1))
    fi
done

echo ""
echo "Done: OK=$OK FAIL=$FAIL"
echo "Golden files written to: $GOLDEN_DIR"
