#!/bin/bash
# build a release exe in a side target dir (live Henry keeps target/release locked) and verify all replays
set -e
root="$(cd "$(dirname "$0")/../.." && pwd -W)"
cd "$root/src-tauri"
CARGO_TARGET_DIR="$root/.replay/target" cargo build --release 2>&1 | grep -E '^(warning|error)|Finished' -A5
cp "$root/.replay/target/release/henry_pet.exe" "$root/.replay/cand.exe"
bash "$root/.replay/verify.sh" ./cand.exe
