#!/bin/bash
# Copy the henry_pet bridge into the Hermes home's user plugins dir and enable it (agents: see hermes-plugin/INSTALL.md).
# Restart Hermes (gateway / desktop) afterwards so it loads.
set -e
HH="${HERMES_HOME:-$LOCALAPPDATA/hermes}"
SRC="$(cd "$(dirname "$0")/.." && pwd)/hermes-plugin/henry_pet"
mkdir -p "$HH/plugins/henry_pet"
cp "$SRC/plugin.yaml" "$SRC/__init__.py" "$HH/plugins/henry_pet/"
hermes plugins enable henry_pet
echo "installed -> $HH/plugins/henry_pet"
