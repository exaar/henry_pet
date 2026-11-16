# Henry's sprites — how they're drawn and what to make (M4)

## Two different sizes — don't mix them up

- **On-screen size** = tray "Size": 64 / 96 / 128 / 160 / 192 / 256 px = WIDTH of one cell on screen (was height until 2026-11-14; the cell is taller than wide now, see PLAN #50) (any 32–512 by hand: `size` in `%APPDATA%/dev.exaar.henrypet/settings.json`, applies live). Old 1x/2x/3x settings migrate to 64/128/192.
- **Source resolution** = pixels in one drawn frame. Decided: **min 96×96, preferably 128×128** (64×64 has no detail). Any cell size works, non-square too.
- Scaling filter (`filter` in pet.json): `auto` (default) = nearest-neighbour when enlarging (crisp pixels), smooth (area/bilinear) when shrinking; or force `nearest` / `smooth`. Scaled once per cell and cached; drawing is a plain copy.

## How rendering works (`src-tauri/src/render.rs`)

- Native layered window (UpdateLayeredWindow), no webview; ~17 MB RAM total.
- **Skins** (2026-11-10): every sheet found is embedded at build (`build.rs`): `henry` = `ui/pets/henry/` (Henry's own, first/default), the petdex `placeholder` is retired (2026-11-13, not embedded). Tray **Skin** (shown only with >1 skin) switches live (sheet reloaded, cell cache dropped), saved as `skin` in settings.json; agent API `GET /skins`, `POST /skin {"skin": id}`, `/status` has `skin`. Changing a sheet still needs a rebuild (~2 min). `HENRY_SPRITE_DIR=<dir>` embeds one folder as the only skin `dev` (tests, refused for public builds).
- `pet.json`: `cell` [w,h], `foot` (paw line), `facing` (direction the art is drawn), `filter`, `animations` {name: {row, frames (count or list of columns), ms (one full cycle), loop (false = play once, hold last frame), facing (right/left/none)}}. The packer writes it; hand edits are fine.
- **Floor line** = `foot` (packer sets it; else lowest opaque pixel of `idle`). The packer puts every ground frame's paws on it.
- **Mirroring:** animations facing right are flipped when she goes left (and vice versa); `facing: none` is never flipped. If a `<name>_left` exists it is used instead of the mirror.
- No climb art → walk rotated 90° against the wall. With `climb` art: drawn upright, paws touching the cell's right edge (packer aligns it), mirrored for the left wall.
- Without Henry's manifest the petdex placeholder rows are mapped onto the same names in code (walk/walk_left, stare, wave, look, splat, sleep = failed frame 3, …).

## Frame rate

- Not fixed. Each animation plays its whole cycle in `ms` regardless of frame count; one frame lasts `ms / frames`.
  - 8 frames / 1100 ms ≈ 7 fps; 12 frames / 1100 ms ≈ 11 fps. More frames = smoother, not slower.
- When an animation is missing and a stand-in plays, the code's tempo is used for it (zoomies 500 ms, run-to-cursor 800 ms, paw push 1500 ms, rotated climb 900 ms); Henry's own art always plays at its `ms`.
- Ceiling ≈ 60 fps (game loop ticks every 16 ms). A frame is redrawn only when it changes.

## Frame counts / number of animations

- Min 1 frame (static pose). Max = sheet columns. Rows may differ in length.
- Memory: sheet decoded fully, w×h×4 bytes. 128×128 cells, 16 frames × 20 rows ≈ 21 MB (fine; fuse at 500 MB).
- A new animation name only plays once code knows **when** to use it (packer warns about unknown names).

## Animation names, when they play, fallbacks

Folder name = animation name. **Variants:** `name_anything` or `name-2` belong to group `name` and one is picked at random each time the action starts (`sleep_curl` + `sleep_starfish` → two sleep poses; `idle-2` → a second idle). A variant sticks for the whole action (twitches return to the same sleep pose).

| name | plays when | if missing → |
|---|---|---|
| idle | standing around | sit |
| sit | resting in a corner (quiet app) | idle |
| walk | walking, to corner/wall, going for an icon | — (required) |
| trot | zoomies, running to the cursor | walk |
| look | looks around (Hermes review, admiring an icon) | idle |
| stare | sits facing you (Hermes waiting, attending) | sit → idle |
| petted | being stroked by the cursor | stare |
| wave | greeting, Hermes turn end | stare → idle |
| sleep | sleeping (variants: sleep_curl, sleep_starfish) | idle |
| twitch | ear flick in sleep (Hermes event) | sleep |
| stretch | after waking up (once), `/do stretch` | — (skipped → look) |
| groom | random idle choice | — (never chosen) |
| jump | take-off crouch before a jump (once) | — (jumps instantly) |
| fly | in the air going up | jump |
| fall | in the air going down | fly |
| land | after a soft landing (once) | — (stands) |
| held | dangling when grabbed; with `pivot` in anim.json the engine hangs it from the scruff as a soft bending body (slinky) | fly |
| splat | hard landing (once) | idle |
| failed | Hermes error | splat |
| hop | Hermes "all todos done" | jump → wave |
| push | pushing a desktop icon | walk |
| climb | up a screen edge | walk rotated 90° |

## What to generate

Draw **one direction (facing right)**; the engine mirrors the other (decided 2026-11-09). Henry's nose blotch is asymmetric and mirroring swaps it — draw walk/trot both ways only if it looks wrong in the generations.

| animation | frames | what |
|---|---|---|
| walk | 8 | normal stroll |
| trot | 6–8 | faster "called to eat" trot: run to cursor, zoomies |
| sit (or idle) | 2–4 | calm sit, blink / tail flick |
| look | 4–6 | turns head |
| stare | 2–4 | sits facing the viewer (Hermes waiting, petting) |
| sleep_curl | 2–4 | ball, slow breathing |
| sleep_starfish | 2–4 | second sleep variant, sprawled |
| groom | 6–8 | washing |
| stretch | 6 | after sleep |
| jump / fly / fall / land | 3 / 2 / 1–2 / 2 | take-off, flight up, flight down, landing |
| held | 2 | dangling when grabbed |
| splat | 4 | hard landing |
| push | 4–6 | pushing a desktop icon |
| climb | 4–6 | up a screen edge; optional (else rotated walk) |

Minimum to start: walk, trot, sit, look, stare, sleep_curl, sleep_starfish; the rest fall back (table above). Extra custom animations welcome (rolls on back, hunts the cursor…) — send frames + say when it should play.

## Delivery format and packing

Loose PNGs with transparent background, one folder per animation (folder name = animation name above) — no need to build a sheet:

```
art/henry/walk/01.png … 08.png
art/henry/sleep_curl/01.png …
art/henry/push/anim.json      optional: {"ms": 1500, "loop": false, "facing": "none", "align": "anim"}
art/henry/sleep_starfish/anim.json  {"floor": 76}: that frame row rests on the taskbar/window top, anything drawn below hangs over the edge (dangling paw); the packer lowers the sheet floor line to make room. Without it the lowest pixel sits on the floor.
art/henry/groom_scratch/anim.json  {"repeat": [16, 32, 1, 3], "frame_ms": 125, "loop": false, "align": "anim"}: one-shot built from in (sit->scratch) + looping middle + out; frames 16..31 repeat 1-3 times -> packer emits groom_scratch_x1..x3 sharing one row, the engine picks one and the beat lasts exactly that long. API: POST /do {"action":"groom"}. `groom_lick` (paw lick) and `groom_wash` (face wash) work the same way: `repeat` [8, 12, 1, 4] -> groom_lick_x1..x4, [28, 33, 1, 3] -> groom_wash_x1..x3; the groom group picks uniformly among all _xN rows.
anim.json "pose": "sit" | "lie" (default stand). Stand->sit plays `trans_sit`, sit->stand `trans_stand`, sit->lie `trans_lie`, stand->lie `trans_sit` + `trans_lie`, lie->sit a random `trans_unlie_*`, lie->stand `trans_unlie_*` + `trans_stand` (she holds still meanwhile, the act keeps its full time). A walk that starts from standing (`stand`, or right after `trans_stand`) first plays `trans_walk`, the first step; it is the one transition that moves her: `"travel"` = sprite px forward as each played frame appears (+ one value on handing over to the walk; the body slides linearly between drawings, PLAN #63), so the planted paw stays roughly put and the drawing lines up with stand at the start and walk frame 1 at the end (first value = the alignment offset to stand, since the 128 cell has no room for a `shift`). `"frames": [0, 0, 1, 1, ...]` = explicit play order (a frame listed twice holds twice as long, `ms` = whole list). `"steps"` on walk = sprite px the planted paw slides back per frame (measured by best overlap of the paw rows of neighbouring frames); with it the brain picks the walk frame from the distance actually covered (sum = one cycle, frames in even slices; PLAN #63) while the body moves smoothly every refresh — the old fixed 2 s cycle under a 110 px/s body made her glide (escalator), pinning the drawing to the paw made her hop (stop-motion). "pivot": [x, y] (frame px, packer converts to cell px in pet.json) = grab point; `held` with a pivot hangs from the cursor at that point and swings (slinky.rs: soft 9-bone chain from the scruff to the bottom of the art, tuned in `docs/slinky-cat.html`; render.rs `render_slinky`: strips bent along a spline through the bones, nearest sampling at screen resolution). Floor of in-place actions is the HIND paws (`floor` = their row + 1): stretch keeps hind paws planted while the front paws reach forward and hang below the taskbar edge. "shift": dx px moves an animation in its cell so poses line up across animations (sitting idles / groom are shifted to match the end of trans_sit; measured by mask overlap, see PLAN #41).
art/henry/aliases.json        optional stand-ins: {"idle": "walk"} = idle plays the walk row (real art wins)
```

- All frames of one animation the same canvas size; cat at the same scale in every frame. Draw facing right.
- `npm run pack-sprites` (= `uv run scripts/pack_sprites.py art/henry --out ui/pets/henry`) → `spritesheet.png` + `pet.json`. Options: `--cell 128` (default: largest frame; the cell grows taller by itself when a pose needs more room above the floor line than that — jump take-off made it 128×142, width and on-screen size unchanged), `--preview` (GIF per animation in `ui/pets/henry/preview/`), `--filter`, `--margin`.
- Packer: puts every ground frame's paws on one floor line (prints how much drift it fixed), keeps jump/fly/fall/held/hop motion as drawn (`"align": "anim"` does the same for any animation, e.g. an in-place hop), centres horizontally, climb against the right edge.
- Then rebuild (`npm run build`). Once `ui/pets/henry/` exists it wins over the placeholder; `npm run build:public` only works with it.
