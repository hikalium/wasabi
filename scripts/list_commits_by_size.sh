#!/bin/bash -e
PROJ_ROOT="$(dirname $(dirname ${BASH_SOURCE:-$0}))"
cd "${PROJ_ROOT}"

# List all commits sorted by diff size (total lines added + deleted), largest first.
# Output format: <lines_changed> <short_sha> <subject>

git log --format="%H %s" | grep -v ' SKIP_EXPLAIN:' | while read sha subject; do
    lines=$(git show --shortstat "$sha" | tail -1 | grep -oE '[0-9]+ insertion|[0-9]+ deletion' | grep -oE '[0-9]+' | awk '{s+=$1} END {print s+0}')
    printf "%6d %s %s\n" "$lines" "$(git rev-parse --short "$sha")" "$subject"
done | sort -rn
