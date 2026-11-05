#!/bin/sh
# Render the README screenshots in docs/images/ from the real UI code
# (ratatui TestBackend, fictional data; see src/ui/screenshots.rs).
#
# Usage: scripts/render-screenshots.sh [output-dir]
# In the development container:
#   docker run --rm -v "$PWD":/src -v wgtarget-shots-wireguard-tui:/src/target \
#     -w /src wgdev scripts/render-screenshots.sh
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
out=${1:-"$root/docs/images"}
mkdir -p "$out"
out=$(cd "$out" && pwd)

cd "$root"
WG_TUI_SCREENSHOTS="$out" cargo test --locked --bin wg-tui -- \
  --ignored --exact ui::screenshots::render_docs_screenshots
ls -l "$out"/tui-*.svg
