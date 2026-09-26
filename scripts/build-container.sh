#!/usr/bin/env bash
set -euo pipefail

IMAGE="${1:-system-scratchpad-build}"
OUT="${2:-dist}"

docker build -t "$IMAGE" .
id="$(docker create "$IMAGE")"
trap 'docker rm -f "$id" >/dev/null 2>&1 || true' EXIT

rm -rf "$OUT"
mkdir -p "$OUT"
docker cp "$id:/dist/scratchpad" "$OUT/scratchpad"
printf 'Built %s/scratchpad\n' "$OUT"
