#!/usr/bin/env bash
# resources/Hero.svg から README 用の resources/Hero.png を生成する
# NSImage (render-svg.swift) は SVG のテキストやフィルタを正しく描けないので Chrome で描画する
set -euo pipefail

cd "$(dirname "$0")/.."
chrome="${CHROME:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}"
profile="$(mktemp -d "${TMPDIR:-/tmp}/lightwell-hero.XXXXXX")"
trap 'rm -rf "$profile"' EXIT

"$chrome" --headless --disable-gpu --hide-scrollbars --user-data-dir="$profile" \
  --force-device-scale-factor=2 --window-size=1280,640 \
  --screenshot="$PWD/resources/Hero.png" "file://$PWD/resources/Hero.svg" 2>/dev/null
echo "generated: resources/Hero.png"
