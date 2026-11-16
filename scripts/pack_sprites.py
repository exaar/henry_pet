# /// script
# requires-python = ">=3.10"
# dependencies = ["pillow>=10"]
# ///
"""Pack Henry's frames into the sheet the app embeds (docs/SKINS.md).

    uv run scripts/pack_sprites.py [SRC] [--out ui/pets/henry] [--cell 128] [--preview]

SRC (default art/henry) holds one folder per animation with PNG frames in name order:

    art/henry/walk/01.png ... 08.png
    art/henry/sleep_curl/01.png ...      variants: sleep_curl + sleep_starfish both play as "sleep"
    art/henry/walk_left/...              optional art for the other direction (else mirrored)
    art/henry/push/anim.json             optional overrides: {"ms": 1500, "loop": false, "facing": "none",
                                         "align": "anim"}  (anim = keep in-place hops, don't level each frame)
                                         "pivot": [x, y]  grab point in the frame PNG (held: the scruff she
                                         hangs from; the engine swings her around it)
                                         "floor": 52  row (in the frame PNG) that rests on the taskbar/window
                                         top; whatever is drawn below it hangs over the edge (a dangling
                                         paw while she sleeps). The sheet's floor line moves up to make room.
                                         "frames": [0, 0, 1, ...]  play order (a frame listed twice holds twice
                                         as long; "ms" = whole list)
                                         "steps": [..]  walk: per frame, sprite px the planted paw slides back
                                         while it shows; the engine picks frames by distance walked and pins
                                         the drawing to the paw (no foot slip at any speed)
                                         "travel": [..]  sprite px she moves forward during each played frame
                                         (trans_walk: the engine moves her by it, paw locked on the floor)
    art/henry/aliases.json               optional stand-ins until real art exists: {"idle": "walk"} makes
                                         "idle" play the walk row (and everything that falls back to idle)

Frames: transparent background, cat at the same scale in every frame. The packer
  - aligns paws: every frame of a ground animation gets its lowest pixel onto one shared floor line;
    airborne ones (jump/fly/fall/held/hop) keep their motion, aligned as a whole;
  - centres each animation horizontally (climb: paws against the cell's right edge, centred vertically);
  - writes spritesheet.png + pet.json (cell size, floor line, per-animation row/frames/ms/loop/facing).
"""
import argparse
import json
import sys
from pathlib import Path

from PIL import Image

ALPHA = 16  # same "body" threshold as the app
AIR = {"jump", "fly", "fall", "held", "hop"}
WALL = {"climb"}
ONCE = {"jump", "land", "splat", "stretch", "failed"}
NO_FACING = {"stare", "wave", "petted"}
MS = {
    "idle": 2400, "sit": 2400, "stand": 2000, "walk": 1100, "trot": 700, "look": 2400, "stare": 2400, "petted": 1600,
    "wave": 1100, "sleep": 3200, "twitch": 400, "groom": 1600, "stretch": 1400, "jump": 300, "fly": 400,
    "fall": 400, "land": 300, "held": 800, "splat": 1600, "failed": 1600, "hop": 1100, "push": 1500, "climb": 900,
}
ORDER = list(MS)  # sheet row order: known groups first, then the rest by name


def group(name: str) -> str:
    return name.replace("-", "_").split("_")[0]


def solid_rows(im: Image.Image):
    """(top, bottom) of rows having >= 2 body pixels (ignores lone specks), or None if empty."""
    a = im.getchannel("A").point(lambda v: 255 if v > ALPHA else 0)
    w, h = im.size
    data = a.tobytes()
    rows = [y for y in range(h) if data[y * w:(y + 1) * w].count(255) >= 2]
    return (rows[0], rows[-1] + 1) if rows else None


def bbox(im: Image.Image):
    return im.getchannel("A").point(lambda v: 255 if v > ALPHA else 0).getbbox()


def hex_rgb(c: str):
    c = c.lstrip("#")
    return tuple(int(c[i:i + 2], 16) for i in (0, 2, 4))


def _lab(rgb):
    def lin(v):
        v /= 255.0
        return v / 12.92 if v <= 0.04045 else ((v + 0.055) / 1.055) ** 2.4
    r, g, b = (lin(float(v)) for v in rgb)
    xyz = ((0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047, 0.2126 * r + 0.7152 * g + 0.0722 * b,
           (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883)
    f = [t ** (1 / 3) if t > 0.008856 else 7.787 * t + 16 / 116 for t in xyz]
    return 116 * f[1] - 16, 500 * (f[0] - f[1]), 200 * (f[1] - f[2])


def to_palette(frames, pal, snap):
    """Count (and with snap: remap, nearest in Lab) opaque colours not in the fixed palette."""
    from collections import Counter
    ok, labs, near, foreign = set(pal), [_lab(p) for p in pal], {}, Counter()
    out = []
    for f in frames:
        px = list(f.getdata())
        bad = [p for p in px if p[3] > 0 and p[:3] not in ok]
        if not bad:
            out.append(f)
            continue
        foreign.update(p[:3] for p in bad)
        if snap:
            for c in {p[:3] for p in bad}:
                if c not in near:
                    l = _lab(c)
                    near[c] = pal[min(range(len(pal)), key=lambda i: sum((a - b) ** 2 for a, b in zip(labs[i], l)))]
            f = f.copy()
            f.putdata([(*near[p[:3]], p[3]) if p[3] > 0 and p[:3] not in ok else p for p in px])
        out.append(f)
    return out, foreign


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("src", nargs="?", default="art/henry")
    ap.add_argument("--out", default="ui/pets/henry")
    ap.add_argument("--cell", help="cell size, e.g. 128 or 128x112 (default: largest frame)")
    ap.add_argument("--margin", type=int, default=1, help="transparent px kept under the paws (default 1)")
    ap.add_argument("--filter", default="auto", choices=["auto", "nearest", "smooth"], help="how the app scales her")
    ap.add_argument("--facing", default="right", choices=["right", "left"], help="direction the art is drawn in")
    ap.add_argument("--preview", action="store_true", help="also write OUT/preview/<anim>.gif")
    ap.add_argument("--snap", action="store_true",
                    help="remap colours outside SRC/palette.json to the nearest palette colour (in the sheet only)")
    args = ap.parse_args()

    src, out = Path(args.src), Path(args.out)
    dirs = sorted(d for d in src.iterdir() if d.is_dir() and any(d.glob("*.png"))) if src.is_dir() else []
    if not dirs:
        print(f"no animation folders with PNGs in {src}", file=sys.stderr)
        return 1
    pal_file = src / "palette.json"
    pal = [hex_rgb(c) for c in json.loads(pal_file.read_text(encoding="utf-8"))["colors"]] if pal_file.exists() else None
    anims = {}
    for d in dirs:
        frames = [Image.open(f).convert("RGBA") for f in sorted(d.glob("*.png"))]
        if pal:
            frames, foreign = to_palette(frames, pal, args.snap)
            if foreign:
                n = sum(foreign.values())
                top = ", ".join("#%02x%02x%02x x%d" % (*c, k) for c, k in foreign.most_common(5))
                print(f"{d.name}: {n} px in {len(foreign)} colours outside palette.json "
                      f"({'snapped to nearest' if args.snap else 'kept, --snap to fix'}): {top}", file=sys.stderr)
        sizes = {f.size for f in frames}
        if len(sizes) > 1:
            print(f"{d.name}: frames differ in size {sorted(sizes)} - draw them on one canvas size", file=sys.stderr)
            return 1
        frames = [f for f in frames if bbox(f)]
        if not frames:
            print(f"{d.name}: every frame is empty, skipped", file=sys.stderr)
            continue
        extra = json.loads((d / "anim.json").read_text(encoding="utf-8")) if (d / "anim.json").exists() else {}
        anims[d.name.lower()] = (frames, extra)

    if args.cell:
        cw, _, ch = args.cell.lower().partition("x")
        cw, ch = int(cw), int(ch or cw)
    else:
        cw = max(f.size[0] for fr, _ in anims.values() for f in fr)
        ch = max(f.size[1] for fr, _ in anims.values() for f in fr)
    # room under the floor line for parts that hang below it ("floor" in anim.json)
    hang = 0
    for n, (fr, extra) in anims.items():
        if "floor" in extra:
            hang = max(hang, max(bbox(f)[3] for f in fr) - int(extra["floor"]))
    foot = ch - max(args.margin, hang)
    if not args.cell:
        # a tall pose (jump take-off) needs more room above the floor line than the canvas gives:
        # grow the cell downwards-anchored (width, and so the on-screen size, stays the same)
        need = 0
        for n, (fr, extra) in anims.items():
            if "floor" in extra or group(n) in WALL:
                continue
            bs = [bbox(f) for f in fr]
            if group(n) in AIR or extra.get("align") == "anim":
                need = max(need, max(b[3] for b in bs) - min(b[1] for b in bs))
            else:
                need = max(need, max(b[3] - b[1] for b in bs))
        if need > foot:
            ch += need - foot
            foot = need

    def place(name, frames, align, floor=None, shift=0):
        """Paste every frame into a cw x ch cell; returns cells and the largest per-frame paw correction."""
        g = group(name)
        boxes = [bbox(f) for f in frames]
        ul, ut = min(b[0] for b in boxes), min(b[1] for b in boxes)
        ur, ub = max(b[2] for b in boxes), max(b[3] for b in boxes)
        if ur - ul > cw or ub - ut > ch:
            raise SystemExit(f"{name}: drawing {ur - ul}x{ub - ut} does not fit the {cw}x{ch} cell (use --cell)")
        dx = cw - ur if g in WALL else (cw - (ur - ul)) // 2 - ul
        dx += int(shift)  # "shift" in anim.json: line poses up across animations (sit idles vs trans_sit end)
        if ul + dx < 0 or ur + dx > cw:
            raise SystemExit(f"{name}: shift {shift} pushes the drawing out of the {cw}px cell")
        cells, drift, offs = [], 0, []
        for f, b in zip(frames, boxes):
            if g in WALL:
                dy = (ch - (ub - ut)) // 2 - ut
            elif floor is not None:
                dy = foot - int(floor)  # that row on the floor line, the rest hangs below
            elif g in AIR or align == "anim":
                dy = foot - ub
            else:
                bottom = (solid_rows(f) or (b[1], b[3]))[1]
                dy = foot - bottom
                drift = max(drift, abs(bottom - ub))
            dy = max(-b[1], min(dy, ch - b[3]))  # never cut the drawing
            cell = Image.new("RGBA", (cw, ch))
            cell.paste(f, (dx, dy))
            cells.append(cell)
            offs.append((dx, dy))
        return cells, drift, offs

    names = sorted(anims, key=lambda n: (ORDER.index(group(n)) if group(n) in ORDER else len(ORDER), n))
    placed = {n: place(n, anims[n][0], anims[n][1].get("align"), anims[n][1].get("floor"), anims[n][1].get("shift", 0)) for n in names}
    cols = max(len(v[0]) for v in placed.values())
    sheet = Image.new("RGBA", (cols * cw, len(names) * ch))
    manifest = {"version": 1, "cell": [cw, ch], "foot": foot, "facing": args.facing, "filter": args.filter, "animations": {}}
    report = []
    for row, n in enumerate(names):
        cells, drift, offs = placed[n]
        for col, c in enumerate(cells):
            sheet.paste(c, (col * cw, row * ch))
        g, extra = group(n), anims[n][1]
        facing = "left" if n.endswith("_left") else "right" if n.endswith("_right") else "none" if g in NO_FACING else args.facing
        entry = {"row": row, "frames": len(cells), "ms": MS.get(g, 1100), "loop": g not in ONCE, "facing": facing}
        entry.update({k: v for k, v in extra.items() if k in ("ms", "loop", "facing", "pose", "steps", "travel")})
        if isinstance(extra.get("frames"), list):  # explicit order / holds: [0, 0, 1, 1, ...] (a frame twice = twice as long)
            entry["frames"] = [int(v) for v in extra["frames"]]
            if any(not 0 <= v < len(cells) for v in entry["frames"]):
                raise SystemExit(f"{n}: frames list points past its {len(cells)} PNGs")
        if "steps" in extra and len(extra["steps"]) != (len(entry["frames"]) if isinstance(entry["frames"], list) else len(cells)):
            raise SystemExit(f"{n}: steps needs one value per played frame")
        if "travel" in extra and len(extra["travel"]) - (len(entry["frames"]) if isinstance(entry["frames"], list) else len(cells)) not in (0, 1):
            raise SystemExit(f"{n}: travel needs one value per played frame (+1 optional: on leaving)")
        if "pivot" in extra:  # grab point (held: the scruff), frame px -> cell px of the first frame
            entry["pivot"] = [float(extra["pivot"][0]) + offs[0][0], float(extra["pivot"][1]) + offs[0][1]]
        if "repeat" in extra:
            # {"repeat": [a, b, lo, hi], "frame_ms": 125}: one-shot whose frames a..b-1 (a looping middle)
            # play lo..hi times -> variants name_x<k> sharing this row; the engine picks one at random
            a_, b_, lo, hi = (int(v) for v in extra["repeat"])
            fms = int(extra.get("frame_ms", 125))
            for k in range(lo, hi + 1):
                seq = list(range(a_)) + list(range(a_, b_)) * k + list(range(b_, len(cells)))
                manifest["animations"][f"{n}_x{k}"] = dict(entry, frames=seq, ms=fms * len(seq), loop=False)
                report.append(f"  {n + '_x' + str(k):<16} row {row:<2} {len(seq):>2} frames {fms * len(seq):>5} ms once  (middle x{k})")
        else:
            manifest["animations"][n] = entry
        known = "" if g in MS else "  (unknown name: plays only once code knows when)"
        report.append(f"  {n:<16} row {row:<2} {len(cells):>2} frames {entry['ms']:>5} ms {'loop' if entry['loop'] else 'once':<4} {entry['facing']:<5}"
                      + (f"  paw drift fixed up to {drift}px" if drift else "") + known)
        if args.preview:
            (out / "preview").mkdir(parents=True, exist_ok=True)
            bg = [Image.alpha_composite(Image.new("RGBA", (cw, ch), (200, 200, 200, 255)), c).convert("P") for c in cells]
            bg[0].save(out / "preview" / f"{n}.gif", save_all=True, append_images=bg[1:], loop=0,
                       duration=max(20, entry["ms"] // len(cells)))
    alias_file = src / "aliases.json"
    aliases = json.loads(alias_file.read_text(encoding="utf-8")) if alias_file.exists() else {}
    for name, target in aliases.items():
        name, target = name.lower(), target.lower()
        if name in manifest["animations"]:
            continue  # real art wins over a stand-in
        if target not in manifest["animations"]:
            print(f"aliases.json: {name} -> {target}: no such animation", file=sys.stderr)
            return 1
        manifest["animations"][name] = dict(manifest["animations"][target])
        report.append(f"  {name:<16} = {target} (alias from aliases.json)")
    out.mkdir(parents=True, exist_ok=True)
    sheet.save(out / "spritesheet.png", optimize=True)
    (out / "pet.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    drawn = {group(n) for n in manifest["animations"]}
    missing = [g for g in ("walk", "trot", "sit", "look", "stare", "sleep") if g not in drawn and not (g == "sit" and "idle" in drawn)]
    print(f"{out / 'spritesheet.png'}: {sheet.size[0]}x{sheet.size[1]}, cell {cw}x{ch}, floor line {foot}, {len(names)} animations")
    print("\n".join(report))
    if missing:
        print(f"  not drawn yet (stand-ins used): {', '.join(missing)}")
    print("rebuild the app to embed it (npm run build)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
