#!/usr/bin/env bash
# Runs the same UI bench (`crates/tonique-core/src/bench.rs`) in the egui app
# and in this one, then compares them.
#
#   scripts/bench.sh [project.tonique]       (default: Demo Song)
#   RUNS=3 TONIQUE_BENCH_SCALE=4 scripts/bench.sh
#
# Each app opens fullscreen, loads the project scaled up, waits for its
# audio, runs ~40 s of phases (idle, play, zoom, scroll) and quits. Keep the
# machine otherwise idle meanwhile; the master is muted.
set -euo pipefail
# From the workspace root: both apps build into its target/.
cd "$(dirname "$0")/../../.."

PROJECT=${1:-"$HOME/Documents/Tonique Projects/Demo Song/Demo Song.tonique"}
RUNS=${RUNS:-1}
OUT=crates/tonique-gpui/bench-results/$(date +%Y%m%d-%H%M%S)
mkdir -p "$OUT"

[ -f "$PROJECT" ] || { echo "No project at $PROJECT" >&2; exit 1; }

echo "Building both (release)…"
cargo build --release --quiet -p tonique_daw -p tonique-gpui

for run in $(seq 1 "$RUNS"); do
    for ui in egui gpui; do
        echo "Run $run/$RUNS: $ui"
        if [ "$ui" = egui ]; then bin=target/release/tonique_daw; else bin=target/release/tonique-gpui; fi
        TONIQUE_BENCH="$PROJECT" TONIQUE_BENCH_OUT="$OUT/$ui-$run.json" "$bin" 2> "$OUT/$ui-$run.log" \
            || { echo "$ui failed, see $OUT/$ui-$run.log" >&2; exit 1; }
        sleep 2
    done
done

python3 -I crates/tonique-gpui/scripts/compare.py "$OUT"
