#!/bin/bash
# Release server: signs OS release files (SSHSIG with the os key), puts them under
# /opt/ultar/web/downloads/<version>/ and adds the release to downloads/releases.json.
# Usage: publish-release.sh <version> <channel> <notes.json> <file>...
#   notes.json: {"ar":"…","en":"…","de":"…"}  (shown with textContent on the website)
# Only for the OS image after the lead's release decision; backup of releases.json first.
set -euo pipefail
VER=${1:?version}; CH=${2:?channel}; NOTES=$(realpath "${3:?notes.json}"); shift 3
[[ "$VER" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[a-z0-9.]+)?$ ]] || { echo "bad version"; exit 1; }
SOCK=/run/ultar-sign/sign.sock; SIGN=/usr/local/lib/ultar-sign/ultar-signd
DL=/opt/ultar/web/downloads; DST=$DL/$VER
TS=$(date -u +%Y%m%d-%H%M%S); B=/root/ultar-backup-$TS; mkdir -p "$B"; cp -a "$DL/releases.json" "$B/"
mkdir -p "$DST"
for f in "$@"; do
  n=$(basename "$f"); [[ "$n" =~ ^[A-Za-z0-9._-]+$ ]] || { echo "bad name $n"; exit 1; }
  cp "$f" "$DST/$n.part" && mv "$DST/$n.part" "$DST/$n"
  "$SIGN" sign-release "$SOCK" "$DST/$n" >/dev/null
  ssh-keygen -Y verify -f "$DL/ultar-allowed-signers" -I release@ultar.code-craft.tech -n ultar-release \
    -s "$DST/$n.sig" <"$DST/$n" >/dev/null || { echo "signature does not verify: $n"; exit 1; }
done
chmod 0644 "$DST"/*
python3 - "$DL/releases.json" "$VER" "$CH" "$NOTES" "$DST" "$@" <<'PY'
import json,sys,os,hashlib,datetime
path,ver,ch,notes,dst=sys.argv[1:6]; files=sys.argv[6:]
data=json.load(open(path)); rel=[r for r in data.get("releases",[]) if r.get("version")!=ver]
out=[]
for f in files:
    n=os.path.basename(f); p=os.path.join(dst,n)
    h=hashlib.sha256()
    with open(p,"rb") as fh:
        for chunk in iter(lambda: fh.read(1<<20), b""): h.update(chunk)
    out.append({"name":n,"url":f"/downloads/{ver}/{n}","size":os.path.getsize(p),"sha256":h.hexdigest(),"sig":f"/downloads/{ver}/{n}.sig"})
rel.insert(0,{"version":ver,"date":datetime.date.today().isoformat(),"channel":ch,"notes":json.load(open(notes)),"files":out})
tmp=path+".new"; json.dump({"releases":rel},open(tmp,"w"),ensure_ascii=False,indent=1); os.replace(tmp,path)
PY
chmod 0644 "$DL/releases.json"
echo "PUBLISH-RELEASE: OK $VER ($#) files, backup $B"
