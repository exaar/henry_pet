#!/bin/bash
# Stop Henry, build the release exe, start it again (detached). Log: target/rebuild.log
cd "$(dirname "$0")/../src-tauri" || exit 1
taskkill /IM henry_pet.exe /F >/dev/null 2>&1
sleep 1
cargo build --release 2>&1 | grep -E "^(error|warning)|-->|^\s+\|" | head -60
if [ "${PIPESTATUS[0]}" != "0" ]; then echo BUILD FAILED; exit 1; fi
exe="$(pwd -W)/target/release/henry_pet.exe"
powershell -NoProfile -Command "Start-Process -FilePath '$exe'"
sleep 3
curl -s -m 3 http://127.0.0.1:47821/status | head -c 120; echo
