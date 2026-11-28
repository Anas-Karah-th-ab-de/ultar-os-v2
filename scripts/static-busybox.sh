#!/usr/bin/env bash
# Prints the path of a statically linked busybox on this host (nothing if there is none).
# Ubuntu up to 24.04 ships it as /bin/busybox; newer releases keep a dynamic /bin/busybox and put
# the static one (package busybox-static) at /usr/bin/busybox-static. BUSYBOX=<path> is tried first.
for b in ${BUSYBOX:-} /bin/busybox /usr/bin/busybox-static /bin/busybox-static; do
  if [ -x "$b" ] && file -L "$b" | grep -q "statically linked"; then
    echo "$b"
    exit 0
  fi
done
exit 0
