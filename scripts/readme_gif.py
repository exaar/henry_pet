"""Regenerate docs/henry.gif (the README banner) from the packed sheet: uv run --with pillow scripts/readme_gif.py"""
import json
from pathlib import Path
from PIL import Image
R=str(Path(__file__).resolve().parent.parent)+"/"
pet=json.load(open(R+"ui/pets/henry/pet.json")); sheet=Image.open(R+"ui/pets/henry/spritesheet.png").convert("RGBA")
cw,ch=pet["cell"]; A=pet["animations"]
def cols(a): f=a["frames"]; return list(range(f)) if isinstance(f,int) else f
names=["walk","idle_3","groom_wash_x1","stretch","sleep_sigh"]
S=2; STEP=60; TOTAL=4560
BG=(232,229,242,255); FLOOR=(201,197,218,255)
cells={n:[sheet.crop((c*cw,A[n]["row"]*ch,(c+1)*cw,(A[n]["row"]+1)*ch)) for c in cols(A[n])] for n in names}
# vertical crop: union bbox of all used frames
top=min(f.getbbox()[1] for v in cells.values() for f in v if f.getbbox()); bot=max(f.getbbox()[3] for v in cells.values() for f in v if f.getbbox())
lefts=[];out=[]
for t in range(0,TOTAL,STEP):
    G=40; fy=6+pet["foot"]-top; im=Image.new("RGBA",((cw+G)*len(names)-G+32,bot-top+14),BG); im.paste(FLOOR,(0,fy,im.width,im.height))
    for i,n in enumerate(names):
        fr=cells[n]; ms=A[n]["ms"]; k=int((t%ms)/ms*len(fr)); im.alpha_composite(fr[k].crop((0,top,cw,bot)),(16+i*(cw+G),6))
    out.append(im.resize((im.width*S,im.height*S),Image.NEAREST))
fs=[f.convert("RGB").quantize(colors=128,method=Image.Quantize.MEDIANCUT,dither=Image.Dither.NONE) for f in out]
fs[0].save(R+"docs/henry.gif",save_all=True,append_images=fs[1:],duration=STEP,loop=0,optimize=True)
import os; print(os.path.getsize(R+"docs/henry.gif"), out[0].size, len(fs))
