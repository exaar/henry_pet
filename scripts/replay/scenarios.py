"""Build scenario dirs (tape + overrides.jsonl) from a recording, so the replay covers what the API tour can't:
grab/throw, petting, the bed in the user's hand, warm window, icon carry, quiet/fullscreen/hidden/size, away+greet, night.

usage: scenarios.py <rec-snap or sc-* dir (any tape + state files)> <out root>
"""
import json, shutil, sys, pathlib

snap, root = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])
tape = (snap / "tape.txt").read_text().splitlines()

# walk the tape: tick number -> last values of some tags
ticks = {0: {}}
last = {}
cur = 0
for l in tape[1:]:
    tag, v = l.split("\t", 1)
    if v != "=":
        last[tag] = v
    if tag == "tick":
        cur += 1
        ticks[cur] = {}
    elif tag in ("wins", "mons"):
        ticks[cur][tag] = last[tag]
N = cur
print("ticks", N)

def state_at(t, tag):
    for k in range(t, -1, -1):
        if tag in ticks.get(k, {}):
            return json.loads(ticks[k][tag])
    raise KeyError(tag)

mons = state_at(N // 2, "mons")
prim = next(m for m in mons if m["primary"])
print("primary", prim)

def o(fr, to, tag, **kw):
    d = {"from": fr, "to": to, "tag": tag}
    d.update(kw)
    return d

def cursor_rel(fr, to, anchor, path):
    return o(fr, to, "cursor", rel=anchor, path=path)

def button(fr, to, on):
    return o(fr, to, "lbutton", value=on)

def cmd(t, name):
    return o(t, t, "ctl", index=5, value=name)

def grab_throw(t, step=(26, -20), hold=40):
    return [
        button(t, t + 3 + hold + 30, False),
        cursor_rel(t, t + 2, "henry", [(0, 0)]),
        button(t + 3, t + 3 + hold, True),
        cursor_rel(t + 3, t + 3 + hold, "henry", [step]),
    ]

def pet(t, n=300):
    return [button(t, t + n, False), cursor_rel(t, t + n, "henry", [(-30, -10)] * 4 + [(30, -10)] * 4)]

S = {}
T = lambda f: max(200, int(N * f))
S["throw"] = grab_throw(T(0.15)) + grab_throw(T(0.45), step=(-8, -4), hold=90) + grab_throw(T(0.75), step=(40, 10))
S["pet"] = pet(T(0.2)) + pet(T(0.5), 500) + pet(T(0.8))
t = T(0.3)
S["bed"] = [
    button(t, t + 300, False),
    cursor_rel(t, t + 2, "bed", [(0, 0)]),
    button(t + 3, t + 60, True),
    cursor_rel(t + 3, t + 60, "bed", [(-6, -4)]),
    cmd(t + 400, "movebed"),
    cmd(t + 1400, "bed"),
    o(t + 2200, t + 2600, "ctl_bed", index=0, value=False),
    o(t + 2700, t + 2700, "ctl_bed", index=1, value={"Place": None}),
]
# a hot window: a real, not maximized one from the tape
t = T(0.25)
wins = state_at(t, "wins")
w = next((w for w in wins if not w["maximized"] and w["rect"]["right"] - w["rect"]["left"] > 500), wins[0])
S["warm"] = [
    o(t, t + 3000, "gpu", value=[True, [w, 62.0], [[w["pid"], 61.0]], 62.0]),
    o(t + 3001, t + 3600, "gpu", value=[True, None, [], 0.5]),
    cmd(t + 4000, "warm"),
    o(t + 4000, t + 4000, "warm_cmd", value=[w, 30.0]),
]
# icon carry on the primary monitor (synthetic icons: nothing real moves)
t = T(0.35)
wk = prim["work"]
icon = lambda i, x, y: {"index": i, "name": f"icon{i}", "pos": [x - wk["left"], y - wk["top"]], "rect": {"left": x, "top": y, "right": x + 76, "bottom": y + 90}}
icons_ok = {"Ok": [1234, [icon(0, wk["left"] + 900, wk["bottom"] - 400), icon(1, wk["left"] + 900, wk["bottom"] - 600)], wins[0]["hwnd"] if wins else 1]}
S["carry"] = [cmd(100, "carry"), o(1, N, "icons", value=icons_ok), cmd(322, "carry"), cmd(t, "carry"), o(t + 6000, t + 9000, "ctl", index=7, value=True)]
# quiet app, fullscreen, hidden, size
t = T(0.2)
S["quiet"] = [
    o(t, t + 1500, "fg", value=[None, "Resolve.exe"]),
    o(t, t + 1500, "ctl", index=2, value=["Resolve.exe"]),
    o(t + 2000, t + 2600, "fg", value=[prim["handle"], "game.exe"]),
    o(t + 3000, t + 3400, "ctl", index=0, value=True),
    o(t + 3600, t + 5600, "ctl", index=1, value=192),
    o(t + 5601, t + 6600, "ctl", index=1, value=64),
]
# The user away 20 min then back; night
t = T(0.3)
S["away"] = [
    o(t, t + 1200, "idle", value={"secs": 1300, "nanos": 0}),
    o(t + 1201, t + 1400, "idle", value={"secs": 0, "nanos": 0}),
    o(t + 1500, t + 4000, "hour", value=2),
    o(t + 1500, t + 4000, "idle", value={"secs": 400, "nanos": 0}),
    o(t + 4200, t + 4200, "ctl", index=5, value="greet"),
    o(t + 4200, t + 9000, "ctl", index=6, value=True),
]

# ordinary use (PLAN #64): the user active the whole time, carries her around 3x (moves ~700 px/s, lets go mid-move)
def carry(t, step=(12, -4), hold=50):
    return [button(t, t + 3 + hold + 30, False), cursor_rel(t, t + 2, "henry", [(0, 0)]),
            button(t + 3, t + 3 + hold, True), cursor_rel(t + 3, t + 4 + hold, "henry", [step])]
S["day"] = [o(1, N, "idle", value={"secs": 2, "nanos": 0})] + carry(T(0.2)) + carry(T(0.4), step=(-10, -6)) + carry(T(0.6))

for name, ov in S.items():
    d = root / f"sc-{name}"
    shutil.rmtree(d, ignore_errors=True)
    d.mkdir(parents=True)
    for f in ["tape.txt", "bed.json", "bond.json", "card.json"]:
        if (snap / f).exists():
            shutil.copy(snap / f, d / f)
    (d / "overrides.jsonl").write_text("\n".join(json.dumps(x) for x in ov) + "\n")
    print(name, len(ov))
