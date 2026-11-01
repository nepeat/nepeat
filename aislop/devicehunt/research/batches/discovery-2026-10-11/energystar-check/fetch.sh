#!/bin/bash
D=/home/erin/nepeat/aislop/devicehunt/data/raw/discovery-2026-10-11-energystar-check
UA="devicehunt-research/0.1 (personal hardware research; contact nepeat@gmail.com)"
i=1
f(){ # name url
  [ $i -gt 0 ] && sleep 3; i=$((i+1))
  ts=$(date -u +%FT%TZ)
  code=$(curl -sS -A "$UA" -o "$D/$1" -w '%{http_code}' --max-time 120 "$2" 2>"$D/$1.err")
  echo "$1|$2|$code|$ts|$(sha256sum "$D/$1"|cut -d' ' -f1)|$(stat -c%s "$D/$1")" >> $D/fetch.log
  echo "$1 $code"
  case $code in 403|429|5*) echo STOP; exit 1;; esac
}
B=https://data.energystar.gov/api
f storage-file-metadata.json $B/views/put7-uu67
f storage-file.csv "$B/v3/views/put7-uu67/export.csv?accessType=DOWNLOAD"
f network-metadata.json $B/views/n8cx-m62r
f network.csv "$B/v3/views/n8cx-m62r/export.csv?accessType=DOWNLOAD"
