#!/usr/bin/env bash
set -euo pipefail
docker build -t system-scratchpad-build .
id=$(docker create system-scratchpad-build)
mkdir -p dist
docker cp "$id:/src/target/release/scratchpad-daemon" dist/
docker cp "$id:/src/target/release/scratchpad-ui" dist/
docker cp "$id:/src/target/release/scratchpad" dist/scratchpad-cli
docker rm "$id" >/dev/null
