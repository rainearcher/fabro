#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
ARC="${ARC:-$REPO_ROOT/target/release/arc}"

PHASE="${1:-validate}"
PARALLEL="${PARALLEL:-1}"

RESULTS_DIR="$(mktemp -d)"
trap 'rm -rf "$RESULTS_DIR"' EXIT

run_one() {
    local dot="$1"
    local dot_dir
    dot_dir="$(dirname "$dot")"
    local dot_name
    dot_name="$(basename "$dot")"
    local rel
    rel="$(python3 -c "import os; print(os.path.relpath('$dot', '$SCRIPT_DIR'))")"

    # Check for companion run.toml (run-<stem>.toml in same dir)
    local stem
    stem="$(basename "${dot%.dot}")"
    local toml
    toml="${dot_dir}/run-${stem}.toml"

    local result_file="$RESULTS_DIR/$(echo "$rel" | tr '/' '_')"

    case "$PHASE" in
        validate)
            if "$ARC" validate "$dot" > "$result_file.log" 2>&1; then
                echo "PASS" > "$result_file"
                echo "  PASS  $rel"
            else
                echo "FAIL" > "$result_file"
                echo "  FAIL  $rel"
            fi
            ;;
        preflight)
            local target="$dot_name"
            [[ -f "$toml" ]] && target="run-${stem}.toml"

            if (cd "$dot_dir" && "$ARC" run start "$target" --preflight > "$result_file.log" 2>&1); then
                echo "PASS" > "$result_file"
                echo "  PASS  $rel"
            else
                echo "FAIL" > "$result_file"
                echo "  FAIL  $rel"
                grep -E "Errors:|•" "$result_file.log" | head -3 >&2
            fi
            ;;
        dry-run|haiku|full)
            local target="$dot_name"
            [[ -f "$toml" ]] && target="run-${stem}.toml"

            local flags=(--auto-approve)
            [[ "$PHASE" == "dry-run" ]] && flags+=(--dry-run)
            [[ "$PHASE" == "haiku" ]] && flags+=(--model claude-haiku-4-5)

            if (cd "$dot_dir" && "$ARC" run start "$target" "${flags[@]}" > "$result_file.log" 2>&1); then
                echo "PASS" > "$result_file"
                echo "  PASS  $rel"
            else
                echo "FAIL" > "$result_file"
                echo "  FAIL  $rel"
                tail -5 "$result_file.log" >&2
            fi
            ;;
        *)
            echo "Usage: $0 <validate|preflight|dry-run|haiku|full>"
            exit 1
            ;;
    esac
}

echo "=== Phase: $PHASE (parallelism: $PARALLEL) ==="
echo ""

# Collect all dot files
dots=()
while IFS= read -r dot; do
    dots+=("$dot")
done < <(find "$SCRIPT_DIR" -name '*.dot' | sort)

# Run with parallelism
active=0
for dot in "${dots[@]}"; do
    run_one "$dot" &
    active=$((active + 1))
    if [[ $active -ge $PARALLEL ]]; then
        wait -n 2>/dev/null || true
        active=$((active - 1))
    fi
done
wait

# Tally results
pass=0
fail=0
for f in "$RESULTS_DIR"/*; do
    [[ "$f" == *.log ]] && continue
    [[ ! -f "$f" ]] && continue
    result="$(cat "$f")"
    if [[ "$result" == "PASS" ]]; then
        pass=$((pass + 1))
    else
        fail=$((fail + 1))
    fi
done
total=$((pass + fail))

echo ""
echo "=== Results: $pass passed, $fail failed, $total total ==="

[[ $fail -eq 0 ]]
