// Public build: Henry's own sprite only; build.rs refuses the petdex placeholder when HENRY_PUBLIC=1.
import { execSync } from 'node:child_process'
execSync('npx tauri build', { stdio: 'inherit', env: { ...process.env, HENRY_PUBLIC: '1' } })
