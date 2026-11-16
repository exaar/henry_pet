# Henry ↔ Hermes bridge — install guide for the agent

This folder ships with the henry_pet installer. `henry_pet/` is a native Hermes user plugin: it posts the
agent's state (thinking / working / waiting for the user / done / failed) to Henry's local API, so the
desktop cat's mood follows live conversations. Fire-and-forget: if Henry isn't running it does nothing and
never blocks Hermes.

Where it is: `%LOCALAPPDATA%\henry_pet\hermes-plugin\henry_pet` (default install dir). If Henry is running,
`GET http://127.0.0.1:47821/hermes-plugin` returns the exact `dir`.

## Install (agent steps)

1. Copy the whole `henry_pet` folder (`plugin.yaml` + `__init__.py`) into the user plugins dir of the Hermes
   home: `$HERMES_HOME/plugins/henry_pet/` (default `%LOCALAPPDATA%\hermes\plugins\henry_pet\`). Overwrite an
   older copy.
2. Enable it: `hermes plugins enable henry_pet`.
3. Restart Hermes (gateway and/or desktop app) so the plugin loads.
4. Check: start a turn in a live session (desktop / Telegram / CLI) and read
   `GET http://127.0.0.1:47821/status` — `mood` should change from `idle` while the agent works.

## Options (environment variables for the Hermes process)

- `HENRY_PET_PROFILE` — which Hermes profile drives her (default `default`). Other profiles, subagents, cron
  and kanban workers are ignored.
- `HENRY_PET_URL` — Henry's API (default `http://127.0.0.1:47821`; change if `api_port` was changed in
  `%APPDATA%\dev.exaar.henrypet\settings.json`).

## Updating

After installing a newer henry_pet, repeat step 1 (and 3) so the plugin copy matches the app version
(`version` in `plugin.yaml` = the app version from `/hermes-plugin`).

## Removing

`hermes plugins disable henry_pet`, then delete `$HERMES_HOME/plugins/henry_pet/`.
