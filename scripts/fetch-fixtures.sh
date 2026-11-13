#!/usr/bin/env sh
# Download the public replays used by the integration tests from the HoN replay CDN.
set -e
dir="$(dirname "$0")/../tests/fixtures"
mkdir -p "$dir"
for id in 13337970 12857222; do
  if [ ! -f "$dir/$id.honreplay" ]; then
    echo "fetching $id"
    curl -fL -o "$dir/$id.honreplay" "https://replays.juvio.com/replays/$id.honreplay" || echo "  could not download $id (replays expire from the CDN); copy it into $dir manually"
  fi
done
