# Development

## Build

```bash
npm install
npm run build:debug     # src-tauri/target/debug/henry_pet.exe
npm run build           # release exe + NSIS installer
npm run build:public    # same, for distribution: strips build-machine paths from the exe and checks it
npm run pack-sprites    # art/henry/ -> ui/pets/henry/ (see SKINS.md)
```

The release exe is locked while Henry runs: quit her (tray → Quit) before rebuilding.

## Layout

- `src-tauri/src/brain/` — position and behaviour (Rust owns where she is and what she does). `mod.rs` holds the state and the tick: sense → decide → motion → animate → present, one file per phase. `pose.rs` = pose transitions as data, plus `world.rs`, `state.rs`.
- `src-tauri/src/render.rs` — native layered-window renderer (UpdateLayeredWindow, no webview) for the spritesheet embedded by `build.rs`.
- `src-tauri/src/slinky.rs` — the soft-body "held by the scruff" physics; `halo.rs` — petting glow; `card.rs` — tray status card; `bed.rs` — her bed.
- `src-tauri/src/win.rs` — Win32: monitors, windows, fullscreen / foreground-app detection.
- `src-tauri/src/tray.rs` — tray menu, autostart (HKCU Run). `settings.rs` — `%APPDATA%/dev.exaar.henrypet/settings.json` (hand edits apply live).
- `src-tauri/src/bond.rs`, `gpu.rs`, `icons.rs`, `sound.rs`, `update.rs`, `memfuse.rs` — bond, GPU load (warm spot), desktop icons, meow, update check, memory fuse.
- `src-tauri/src/tape.rs` — record / replay of every brain input: `HENRY_RECORD=<dir>` records, `henry_pet.exe --replay <dir> --trace <file>` replays headless. Same tape + same behaviour = the same trace byte for byte, so refactors can be checked against recordings (`scripts/replay/`).
- `hermes-plugin/` — the Hermes Agent bridge (`INSTALL.md` is written for the agent). `bash scripts/install-hermes-plugin.sh` installs it from the repo.

## API

`http://127.0.0.1:47821` (port: `api_port` in settings.json). `GET /` lists everything with arguments. Main ones:

| | |
|---|---|
| `GET /status` | act, animation, needs, mood, bond, position, bed, card |
| `POST /do` | `{"action": ...}`: `come`, `walk`, `zoomies`, `jump`, `climb`, `down`, `sleep`, `wake`, `groom`, `stretch`, `wave`, `carry`, `bed`, `warm`, `glow`, … |
| `POST /mood` | `{"state": "thinking|working|waiting|done|failed|idle", "source": "...", "ttl_s": 600}` |
| `POST /needs`, `POST /bond` | set needs / bond (tests) |
| `POST /card` | show / pin / move the status card |
| `GET /bed`, `POST /bed` | `show`, `hide`, `place` |
| `GET /icons`, `POST /icons/return` | desktop icon mischief |
| `GET /update`, `POST /update` | update check state / check now |
| `POST /hide`, `POST /show` | |

## Dev switches (environment)

- `HENRY_TIME_SCALE=<n>` — needs grow n× faster.
- `HENRY_TRACE=<file>` — log every drawn tick (ms, anim, frame, window x, pin) for motion checks.
- `HENRY_START_X=<px>[,<dir>]` — start at x walking left (-1) or right (1) for ~8 s (edge / monitor tests).
- `HENRY_ICON_LV=<hwnd>` — use a test SysListView32 instead of the desktop.
- `HENRY_UPDATE_URL=<url>` — run the update check against another releases/latest-shaped JSON.
- `HENRY_SPRITE_DIR=<dir>` — embed a sheet from any folder instead of `ui/pets/henry` (build time).
