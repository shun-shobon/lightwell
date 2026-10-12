#!/usr/bin/env bash
# resources/*.svg から AppIcon.icns と MenuBarIcon.tiff を生成する
set -euo pipefail

cd "$(dirname "$0")/.."
work="$(mktemp -d "${TMPDIR:-/tmp}/lightwell-icons.XXXXXX")"
trap 'rm -rf "$work"' EXIT

swiftc scripts/render-svg.swift -o "$work/render"

iconset="$work/AppIcon.iconset"
mkdir "$iconset"
for size in 16 32 128 256 512; do
  "$work/render" resources/AppIcon.svg "$iconset/icon_${size}x${size}.png" "$size"
  "$work/render" resources/AppIcon.svg "$iconset/icon_${size}x${size}@2x.png" "$((size * 2))"
done
iconutil -c icns "$iconset" -o resources/AppIcon.icns

"$work/render" resources/MenuBarIcon.svg "$work/menu.png" 18
"$work/render" resources/MenuBarIcon.svg "$work/menu@2x.png" 36
tiffutil -cathidpicheck "$work/menu.png" "$work/menu@2x.png" -out resources/MenuBarIcon.tiff

echo "generated: resources/AppIcon.icns resources/MenuBarIcon.tiff"
