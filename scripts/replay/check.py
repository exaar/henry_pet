"""Snapshot a (possibly still growing) recording, replay it, compare with the live trace / a golden trace.

usage: check.py <rec dir> <exe> [golden trace]   -> writes <rec>-snap/, prints the first difference
"""
import shutil, subprocess, sys, pathlib

rec, exe = pathlib.Path(sys.argv[1]), sys.argv[2]
rest = [a for a in sys.argv[3:] if not a.startswith("--")]
golden = pathlib.Path(rest[0]) if rest else None
snap = rec.with_name(rec.name + "-snap")
if not (snap / "tape.txt").exists() or "--fresh" in sys.argv:
    shutil.rmtree(snap, ignore_errors=True)
    snap.mkdir()
    for f in ["bed.json", "bond.json", "card.json"]:
        if (rec / f).exists():
            shutil.copy(rec / f, snap / f)
    raw = (rec / "tape.txt").read_bytes()
    lines = raw.split(b"\n")
    if not raw.endswith(b"\n"):
        lines = lines[:-1]
    # cut before the last tick line: every tick kept is complete
    last_tick = max(i for i, l in enumerate(lines) if l.startswith(b"tick\t"))
    (snap / "tape.txt").write_bytes(b"\n".join(lines[:last_tick]) + b"\n")
    if (rec / "live-trace.txt").exists():
        shutil.copy(rec / "live-trace.txt", snap / "live-trace.txt")

out = snap / "replay-trace.txt"
r = subprocess.run([exe, "--replay", str(snap), "--trace", str(out)], capture_output=True, text=True)
if r.returncode != 0:
    print("replay failed", r.returncode, r.stderr[-3000:])
    sys.exit(1)

def ticks(p):
    """trace lines (incl. their notes) grouped by tick"""
    out = []
    for l in p.read_text(encoding="utf-8").splitlines():
        if l.startswith("  note: mood "):
            continue  # written by the API thread (set_mood), not by the brain
        if l.startswith("  note:"):
            out[-1] += "\n" + l
        else:
            out.append(l)
    return out

rep = ticks(out)
ref = ticks(golden if golden else snap / "live-trace.txt")
n = min(len(rep), len(ref))
print(f"replay ticks {len(rep)}, reference ticks {len(ref)} ({'golden' if golden else 'live'})")
for i in range(n):
    if rep[i] != ref[i]:
        print(f"FIRST DIFF at tick index {i}:\n  ref: {ref[i]}\n  rep: {rep[i]}")
        for j in range(max(0, i - 2), i):
            print("  before:", ref[j][:200])
        sys.exit(2)
if golden and len(rep) != len(ref):
    print("length differs")
    sys.exit(2)
print(f"IDENTICAL over {n} ticks")
