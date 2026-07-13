#!/bin/bash
#
# Tagma POC Full Validation Script
# Automatically discovers all subdirectories under poc/ that contain
# a run.sh script and executes them in sequence.
#
# Usage:
#   ./run.sh                    # Run all PoC validations
#   ./run.sh --list             # List discovered PoCs without running
#

set -e

MODE="run"

while [[ "$#" -gt 0 ]]; do
    case $1 in
        --list) MODE="list" ;;
        *) echo "Unknown parameter: $1"; exit 1 ;;
    esac
    shift
done

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "─────────────────────────────────────────────────────────────"
echo "Tagma POC — discovering PoC directories..."
echo "─────────────────────────────────────────────────────────────"

POC_DIRS=()
while IFS= read -r -d '' run_script; do
    [ "$run_script" = "$SCRIPT_DIR/run.sh" ] && continue
    [[ "$run_script" == */target/* || "$run_script" == */.git/* ]] && continue
    poc_dir="$(dirname "$run_script")"
    poc_name="$(basename "$poc_dir")"
    POC_DIRS+=("$poc_dir")
    echo "  Found: $poc_name/"
done < <(find "$SCRIPT_DIR" -maxdepth 3 -name "run.sh" -type f -print0 2>/dev/null)

if [ ${#POC_DIRS[@]} -eq 0 ]; then
    echo "  No PoC directories found."
    exit 0
fi

echo ""
echo "Total PoCs: ${#POC_DIRS[@]}"
echo ""

if [ "$MODE" = "list" ]; then
    for dir in "${POC_DIRS[@]}"; do
        echo "  $(basename "$dir")/"
    done
    exit 0
fi

echo "─────────────────────────────────────────────────────────────"
echo "Running all PoC validations..."
echo "─────────────────────────────────────────────────────────────"

ALL_PASSED=0
ALL_FAILED=0
FAILED_NAMES=()

for dir in "${POC_DIRS[@]}"; do
    poc_name="$(basename "$dir")"
    echo ""
    echo "~~~~~~~~~~~~ $poc_name/run.sh ~~~~~~~~~~~~"

    set +e
    (cd "$dir" && bash run.sh 2>&1)
    STATUS=$?
    set -e

    if [ $STATUS -eq 0 ]; then
        echo "~~~~~~~~~~~~ $poc_name/run.sh PASSED ~~~~~~~~~~~~"
        ALL_PASSED=$((ALL_PASSED + 1))
    else
        echo "~~~~~~~~~~~~ $poc_name/run.sh FAILED (exit code $STATUS) ~~~~~~~~~~~~"
        ALL_FAILED=$((ALL_FAILED + 1))
        FAILED_NAMES+=("$poc_name")
    fi
done

echo ""
echo "═════════════════════════════════════════════════════════════"
echo "  Validation Summary"
echo "═════════════════════════════════════════════════════════════"
echo ""
echo "  Passed: $ALL_PASSED"
echo "  Failed: $ALL_FAILED"
if [ ${#FAILED_NAMES[@]} -gt 0 ]; then
    echo "  Failed PoCs:"
    for name in "${FAILED_NAMES[@]}"; do
        echo "    - $name"
    done
fi
echo ""

if [ $ALL_FAILED -eq 0 ]; then
    echo "═════════════════════════════════════════════════════════════"
    echo "  ALL POC VALIDATIONS PASSED!"
    echo "═════════════════════════════════════════════════════════════"
    exit 0
else
    echo "═════════════════════════════════════════════════════════════"
    echo "  SOME POC VALIDATIONS FAILED!"
    echo "═════════════════════════════════════════════════════════════"
    exit 1
fi
