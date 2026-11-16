#!/bin/bash
# Corpus (recordings, scenario dirs with golden.txt) lives in henry_pet/.replay (git-excluded: window titles/exe names); copy this script there.
# verify.sh <exe> [--bless]: replay every recording/scenario, compare with golden.txt (--bless writes them)
cd "${REPLAY_DIR:-$(dirname "$0")}"
exe=$1
fail=0
for d in *-snap sc-*; do
  [ -f "$d/tape.txt" ] || continue
  "$exe" --replay "$d" --trace "$d/now.txt" 2> "$d/now.err"
  rc=$?
  if [ "$2" = "--bless" ]; then
    cp "$d/now.txt" "$d/golden.txt"
    echo "blessed $d ($(grep -vc '^  ' "$d/golden.txt") ticks)"
    continue
  fi
  if [ $rc -ne 0 ]; then echo "FAIL $d: replay exit $rc: $(tail -c 400 "$d/now.err")"; fail=1; continue; fi
  if cmp -s "$d/golden.txt" "$d/now.txt"; then
    echo "ok   $d ($(grep -vc '^  ' "$d/now.txt") ticks)"
  else
    fail=1
    echo "DIFF $d:"
    diff "$d/golden.txt" "$d/now.txt" | head -6
  fi
done
exit $fail
