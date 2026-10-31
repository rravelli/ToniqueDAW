#!/usr/bin/env bash
# Compare the images rendered in DIR with master's, kept on the screenshots
# branch. Copies master's next to them, in DIR/before/, and lists in
# DIR/changed.txt the images that differ from master's, or that master
# doesn't have.
#
#     .github/scripts/compare-screenshots.sh out
set -euo pipefail

dir=$1
mkdir -p "$dir/before"
: > "$dir/changed.txt"

if git fetch --quiet --depth 1 origin screenshots 2>/dev/null; then
    for path in $(git ls-tree --name-only FETCH_HEAD master/ 2>/dev/null); do
        git show "FETCH_HEAD:$path" > "$dir/before/$(basename "$path")"
    done
else
    echo "No screenshots branch yet: every image is new."
fi

for image in "$dir"/*.png; do
    name=$(basename "$image")
    before="$dir/before/$name"
    if [ ! -f "$before" ]; then
        echo "$name: new"
        echo "$name" >> "$dir/changed.txt"
        continue
    fi
    # Pixels differing by more than 1%, to ignore rounding in the
    # rasterizer. `compare` exits 1 when the images differ, 2 on errors.
    pixels=$(compare -metric AE -fuzz 1% "$before" "$image" null: 2>&1 || true)
    if [ "$pixels" != "0" ]; then
        echo "$name: $pixels pixels differ"
        echo "$name" >> "$dir/changed.txt"
    else
        echo "$name: unchanged"
    fi
done
