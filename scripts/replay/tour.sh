#!/bin/bash
# API tour for a recording: exercises most acts (no carry: it moves real desktop icons; no bond set)
A=http://127.0.0.1:47821
do_() { curl -s -X POST $A/do -d "{\"action\":\"$1\"}" >/dev/null; echo "$(date +%T) do $1"; sleep "$2"; }
mood() { curl -s -X POST $A/mood -d "{\"state\":\"$1\",\"source\":\"tour\",\"ttl_s\":60}" >/dev/null; echo "$(date +%T) mood $1"; sleep "$2"; }
do_ wake 4
do_ walk 6
do_ zoomies 6
do_ jump 6
do_ walk 5
do_ down 5
do_ come 8
do_ wave 4
do_ groom 6
do_ stretch 5
mood run 8
mood review 6
mood waiting 12
mood idle 2
mood wave 4
mood failed 4
do_ climb 20
do_ down 6
do_ glow 4
do_ bed 25
do_ wake 6
do_ movebed 25
do_ idle 4
do_ warm 20
do_ jump 6
do_ greet 12
curl -s -X POST $A/needs -d '{"playfulness":0.9,"sleepiness":0.1}' >/dev/null; echo needs; sleep 20
do_ sleep 15
do_ wake 8
do_ walk 10
echo done
