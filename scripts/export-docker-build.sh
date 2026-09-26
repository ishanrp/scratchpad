#!/usr/bin/env bash
set -euo pipefail
IMAGE="${1:-system-scratchpad-build}"
OUT="${2:-dist}"
docker build -t "$IMAGE" .
cid="$(docker create "$IMAGE")"
trap 'docker rm -f "$cid" >/dev/null 2>&1 || true' EXIT
rm -rf "$OUT"
mkdir -p "$OUT"
docker cp "$cid:/dist/." "$OUT/"
printf 'Built binaries exported to %s/\n' "$OUT"
