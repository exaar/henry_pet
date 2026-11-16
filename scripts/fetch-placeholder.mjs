// Dev-only placeholder pet (petdex "pixel-black-cat"). Never shipped in a public build (PLAN #6).
import { mkdirSync, writeFileSync } from 'node:fs'

const base = 'https://assets.petdex.dev/pets/pixel-black-cat-85c2e0a40279/'
const dir = new URL('../ui/pets/placeholder/', import.meta.url)
mkdirSync(dir, { recursive: true })
for (const [remote, local] of [['sprite.webp', 'spritesheet.webp'], ['petjson.json', 'pet.json']]) {
  const res = await fetch(base + remote)
  if (!res.ok) throw new Error(`${remote}: HTTP ${res.status}`)
  writeFileSync(new URL(local, dir), Buffer.from(await res.arrayBuffer()))
  console.log('ok', local)
}
