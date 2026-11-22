#!/bin/bash
# Quellen parallel laden, dann bauen; Ergebnis/Log in ~/ultar-web-build/out-$1
export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
unset LD_LIBRARY_PATH PERL_MM_OPT
A=${1:-aarch64}
W=$HOME/ultar-web-build
cd $W/buildroot
M="make BR2_EXTERNAL=$W/br-ext O=$W/out-$A"
date > $W/build-$A.start
{
  echo "== Quellen laden (parallel)"
  $M -s show-targets 2>/dev/null | tr ' ' '\n' | grep -v '^$' | sort -u > $W/targets-$A.txt
  # Werkzeuge des Rechners (host-*) mit dazu
  $M -s show-info 2>/dev/null | python3 -c 'import json,sys; d=json.load(sys.stdin); print("\n".join(k for k in d))' > $W/allpk-$A.txt || cp $W/targets-$A.txt $W/allpk-$A.txt
  wc -l $W/allpk-$A.txt
  xargs -P 10 -I{} sh -c "$M -s {}-source >/dev/null 2>&1 || echo 'Quelle fehlt: {}'" < $W/allpk-$A.txt
  echo "== Bauen"
  $M ${2:-}
  echo "EXIT $?"
} > $W/build-$A.log 2>&1
date > $W/build-$A.end
