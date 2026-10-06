#!/usr/bin/env bash
# リリースビルドして target/release/Lightwell.app を作る。
# --install を付けると ~/Applications に配置して起動する。
set -euo pipefail

cd "$(dirname "$0")/.."
cargo build --release

app="target/release/Lightwell.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS"
cp target/release/lightwell "$app/Contents/MacOS/"
cp resources/Info.plist "$app/Contents/"
codesign --force --sign - "$app"

if [[ "${1:-}" == "--install" ]]; then
  dest="$HOME/Applications/Lightwell.app"
  pkill -x lightwell || true
  # 終了しきる前に open すると、LaunchServices が終了中のプロセスに送ろうとして -600 で失敗する
  while pgrep -x lightwell >/dev/null; do sleep 0.1; done
  mkdir -p "$HOME/Applications"
  rm -rf "$dest"
  cp -R "$app" "$dest"
  open "$dest"
  echo "installed: $dest"
else
  echo "built: $app"
fi
