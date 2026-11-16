# henry_pet

Henry — a desktop cat for Windows (Tauri 2). She lives on your taskbar and window tops, naps in her bed, comes over to be petted, and (optionally) follows your Hermes agent's mood. Sprite format and animation names: [SPRITES.md](SPRITES.md).

## Build

Needs Rust (`rustup`, MSVC toolchain), Node (Tauri CLI), Visual Studio C++ build tools.

```bash
npm install
npm run fetch-placeholder   # dev-only petdex placeholder sprite -> ui/pets/placeholder (gitignored; retired skin, only via HENRY_SPRITE_DIR)
npm run build:debug         # -> src-tauri/target/debug/henry_pet.exe
npm run build               # release + NSIS installer (personal)
npm run build:public        # blocked until ui/pets/henry/ has Henry's own sheet
```

## Layout

- `src-tauri/src/brain/` — position + behaviour (Rust owns where she is and what she does): `mod.rs` (state + the tick: sense → decide → motion → animate → present, one file per phase), `pose.rs` (pose transitions as data), `world.rs`, `state.rs`
- `src-tauri/src/tape.rs` — record / replay of every brain input (refactor safety net, `scripts/replay/`): `HENRY_RECORD=<dir>` records, `henry_pet.exe --replay <dir> --trace <file>` replays headless; same tape + same behaviour = same trace byte for byte
- `src-tauri/src/win.rs` — Win32: monitors, fullscreen/foreground-app detection, moving the overlay
- `src-tauri/src/tray.rs` — tray menu, autostart (HKCU Run)
- `src-tauri/src/settings.rs` — `%APPDATA%/dev.exaar.henrypet/settings.json` (`quiet_apps` reloads live)
- `src-tauri/src/render.rs` — native layered-window renderer for a petdex-format spritesheet (embedded by `build.rs`)
- `src-tauri/src/icons.rs`, `sound.rs`, `memfuse.rs` — desktop icons, meow/purr, memory fuse

## Agent API

`http://127.0.0.1:47821` (port in settings.json `api_port`): `GET /` lists endpoints, `GET /status`, `POST /mood`, `POST /do` (incl. `jump`, `climb`, `down`, `carry`, `bed`, `movebed`, `warm`, `glow`), `POST /bond`, `POST /needs`, `POST /card` (status card without hovering; `pin`, `x`/`y` pin and move it), `GET /bed`, `POST /bed` (`show|hide|place`), `GET /icons`, `POST /icons/return`, `POST /hide`, `POST /show`.

## Hermes bridge

`bash scripts/install-hermes-plugin.sh` copies `hermes-plugin/henry_pet` into the Hermes home's user plugins and runs `hermes plugins enable henry_pet`; restart Hermes to load. The installer ships the plugin too (`%LOCALAPPDATA%\henry_pet\hermes-plugin`, path also from `GET /hermes-plugin`); `hermes-plugin/INSTALL.md` is the guide for another user's agent.

## Release

1. Bump the version in `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`, `hermes-plugin/henry_pet/plugin.yaml`.
2. `taskkill /IM henry_pet.exe /F`, then `npm run build:public` → `src-tauri/target/release/bundle/nsis/henry_pet_<ver>_x64-setup.exe`.
3. Users just run the new installer: it installs over the old version (per-user, no admin, no Rust needed), keeps settings. Uninstall from Windows Settings → Apps.
4. Publish: commit, `git tag -a v<ver> -m "henry_pet <ver>"`, `git push origin v<ver>`, then `gh release create v<ver> <installer> --title "Henry <ver>" --notes-file <notes> --latest` (notes: install, SmartScreen "More info → Run anyway", Hermes plugin line, update/uninstall, what's new, SHA-256; v0.2.0 is the template).

## Dev knobs

- `HENRY_ICON_LV=<hwnd>` — use a test SysListView32 instead of the desktop.

- `HENRY_TIME_SCALE=<n>` — needs grow n× faster.
- `HENRY_TRACE=<file>` — log every drawn tick (ms, anim, frame, window x, x, pin) for motion/seam checks.
- `HENRY_START_X=<px>[,<dir>]` — start at x walking left (-1) or right (1) for ~8 s (edge / monitor-crossing tests).

## License

Code: MIT ([LICENSE](LICENSE)). Henry's art, spritesheet, icons and meow: all rights reserved, usable only as part of this app ([art/LICENSE.md](art/LICENSE.md)).
