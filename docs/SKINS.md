# Making your own skin

Henry's engine doesn't care what it animates. Draw your own pet as loose PNG frames, pack them into a sheet and build. Everything below is about the files the engine expects. *How* you draw the frames is up to you.

> Henry's own art is not licensed for reuse (see [art/LICENSE.md](../art/LICENSE.md)). For a fork, replace `art/henry/` with your own frames.

## Quick start

```bash
# 1. your frames, one folder per animation (folder name = animation name)
art/henry/walk/01.png ... 16.png
art/henry/idle/01.png ... 08.png
art/henry/sleep/01.png ...

# 2. pack: -> ui/pets/henry/spritesheet.png + pet.json  (needs uv: https://docs.astral.sh/uv/)
npm run pack-sprites
#    add --preview to also get a GIF per animation in ui/pets/henry/preview/

# 3. build (the sheet is embedded into the exe)
npm run build:debug
```

Only `walk` is required. Anything missing falls back to something that exists (table below), so you can start with `walk` + `idle` and add the rest later.

## Frames

- **PNG with transparency**, one file per frame, played in file-name order (`01.png`, `02.png`, …).
- **Same scale in every frame and every animation.** The canvas size may vary between animations: the packer aligns them.
- **Draw facing right.** The engine mirrors the art when she walks left. If mirroring looks wrong for an animation (asymmetric markings), add a `<name>_left/` folder with left-facing frames.
- **Size:** ~96–128 px per frame works well. The tray *Size* setting scales the cell on screen, nearest-neighbour when enlarging, so pixel art stays crisp.
- **Hard edges.** Any pixel with alpha ≥ 16 counts as body (for grabbing, petting, the glow), so avoid soft semi-transparent fringes.
- **Palette (optional):** put `art/henry/palette.json` (`{"colors": ["#rrggbb", ...]}`) next to the folders and the packer reports pixels outside it; `--snap` remaps them to the nearest palette colour.

The packer puts every ground animation's lowest pixels (the paws) on one floor line and centres each animation horizontally. Airborne ones (`jump`, `fly`, `fall`, `held`, `hop`) keep their motion as drawn.

## Timing

Each animation plays its whole cycle in `ms` milliseconds, however many frames it has: one frame lasts `ms / frames`. More frames means smoother, not slower. Defaults per name are in `scripts/pack_sprites.py` (`MS`); override them per animation in `anim.json`.

## Animation names

| name | plays when | if missing |
|---|---|---|
| `walk` | walking anywhere | — (required) |
| `idle` | sitting around (the resting pose) | `sit` |
| `stand` | standing still between sitting and walking | `idle` |
| `trot` | running: zoomies, hurrying to the cursor | `walk`, played faster |
| `look` | looking around | `idle` |
| `stare` | sitting, facing you (agent waiting for you) | `idle` |
| `petted` | being stroked | `stare` |
| `wave` | greeting | `stare` |
| `sleep` | sleeping | `idle` |
| `twitch` | ear flick in her sleep | `sleep` |
| `groom` | washing (random pastime) | `idle` |
| `stretch` | after waking up (plays once) | `idle` |
| `jump` / `fly` / `fall` / `land` | take-off crouch / going up / coming down / landing | each falls back to the previous one, then `stand` / `idle` |
| `held` | dangling when picked up | `fly` → `jump` |
| `splat` | hard landing after a throw | `idle` |
| `failed` | agent task failed | `splat` |
| `hop` | agent finished everything | `jump` → `wave` |
| `push` | nudging a desktop icon | `walk` |
| `climb` | going up a screen edge | `walk` turned 90° |

**Variants:** `sleep_curl/` and `sleep_sprawl/` both belong to `sleep` (everything before the first `_` or `-` is the group); one is picked at random each time she falls asleep and kept for the whole nap.

**Pose transitions (optional):** with `"pose": "sit"` or `"lie"` in an animation's `anim.json` the engine knows what posture it's in, and plays these between postures if they exist: `trans_sit` (stand → sit), `trans_stand` (sit → stand), `trans_lie` (sit → lie), `trans_unlie_*` (lie → sit, variants allowed), `trans_walk` (the first step from standing). Without them she just switches poses.

## anim.json

Optional, one per animation folder. All keys are optional:

```json
{
  "ms": 1500,
  "loop": false,
  "facing": "none",
  "pose": "sit",
  "align": "anim",
  "floor": 52,
  "frames": [0, 0, 1, 2, 3, 3],
  "pivot": [64, 20]
}
```

| key | meaning |
|---|---|
| `ms` | length of one full cycle (or of the `frames` list) |
| `loop` | `false` = play once and hold the last frame |
| `facing` | `right` (default), `left`, or `none` = never mirrored |
| `pose` | `stand` (default), `sit` or `lie`, for the transitions above |
| `frames` | play order; a frame listed twice is held twice as long |
| `align` | `anim` = keep the frames' own vertical motion (an in-place hop) instead of putting each frame on the floor |
| `floor` | the pixel row (in your PNG) that rests on the taskbar. Anything drawn below hangs over the edge (a dangling paw while asleep) |
| `pivot` | `held` only: the grab point `[x, y]` in your PNG. She hangs from the cursor by it and swings as a soft body |
| `steps` | `walk` only: per frame, how many px the planted paw slides back. With it the walk cycle follows the distance actually covered, so paws don't skate |
| `travel` | `trans_walk` only: px she moves forward per frame |
| `repeat` | `[from, to, min, max]`: frames `from..to` repeat a random number of times (a scratch that goes on a bit longer) |

## pet.json

The packer writes `ui/pets/henry/pet.json` next to the sheet: cell size, floor line (`foot`), scaling `filter` (`auto` / `nearest` / `smooth`) and, per animation, its row, frames, `ms`, `loop` and `facing`. Hand edits are fine but get overwritten by the next pack; put lasting changes in `anim.json`.

## Testing a sheet

`HENRY_SPRITE_DIR=<folder with spritesheet.png + pet.json>` at build time embeds that folder instead of `ui/pets/henry`. With the app running, `POST http://127.0.0.1:47821/do {"action": "groom"}` (or `sleep`, `jump`, `stretch`, `walk`, …) plays a given behaviour on demand. `GET /` lists them all.
