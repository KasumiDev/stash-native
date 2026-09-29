#!/usr/bin/env bash
# Finite synthetic route captures. No fixture server or other service is started.
set -euo pipefail
sim=${1:?simulator executable required}
assets=${2:?staged host assets required}
runtime=${3:?private runtime directory required}
out=${4:?capture directory required}
mkdir -p "$runtime" "$out"
for screen in home performers scenes galleries tags search settings scene performer tag gallery viewer; do
  STASH_FIXTURES=1 STASH_SCREEN="$screen" \
  PLXNATIVE_RUNTIME_DIR="$runtime" PLXNATIVE_APP_DIR="$assets" PLXNATIVE_WIN=1920x1080 \
  PLXNATIVE_SHOT="$out/stash-$screen.png" PLXNATIVE_SHOT_FRAME=6 PLXNATIVE_SHOT_EXIT=1 \
  SDL_VIDEODRIVER=x11 timeout 20s "$sim" >"$runtime/capture-$screen.log" 2>&1
  test -s "$out/stash-$screen.png"
  printf 'captured %s\n' "$screen"
done
