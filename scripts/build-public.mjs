// Public build: Henry's own sprite only; build.rs refuses the petdex placeholder when HENRY_PUBLIC=1.
// Build-machine paths (repo, cargo home: they carry the Windows user name) are remapped out of the exe's
// panic/debug strings, then the exe is scanned: any leftover path stops the build.
import { execSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { resolve, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const cargoHome = process.env.CARGO_HOME || resolve(homedir(), '.cargo')
const rustup = process.env.RUSTUP_HOME || resolve(homedir(), '.rustup')
const remap = [[cargoHome, 'cargo'], [rustup, 'rustup'], [repo, 'henry_pet'], [homedir(), 'home']]
  .map(([from, to]) => `--remap-path-prefix=${from}=${to}`).join(' ')
const RUSTFLAGS = [process.env.RUSTFLAGS, remap].filter(Boolean).join(' ')
execSync('npx tauri build', { stdio: 'inherit', cwd: repo, env: { ...process.env, HENRY_PUBLIC: '1', RUSTFLAGS } })

const exe = readFileSync(resolve(repo, 'src-tauri/target/release/henry_pet.exe')).toString('latin1')
const user = homedir().split(/[\\/]/).pop()
const bad = [homedir(), repo, user].filter((s) => s && s.length > 2 && exe.toLowerCase().includes(s.toLowerCase()))
if (bad.length) {
  console.error(`build-public: the exe still contains build-machine strings: ${bad.join(', ')}`)
  process.exit(1)
}
console.log('build-public: exe clean (no build-machine paths)')
