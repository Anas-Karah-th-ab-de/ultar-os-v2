#!/usr/bin/env bash
# Captures how the running Linux drives the Intel display (reference for the Ultar display driver).
# Run as root on the target machine under Ubuntu. Prints one base64 tarball line between BEGIN/END,
# so the result can be carried over a plain ssh session.
#   tools/kern-test/gfx-capture.sh [extra intel_reg register names or offsets ...]
set -u
out=$(mktemp -d)
dri=/sys/kernel/debug/dri
card=$(ls -d "$dri"/*/ 2>/dev/null | while read -r d; do [ -f "$d/i915_display_info" ] && echo "$d"; done | head -1)
[ -n "$card" ] || { echo "no i915 debugfs entry" >&2; exit 1; }
for f in i915_display_info i915_dmc_info i915_power_domain_info i915_shared_dplls_info i915_ddb_info i915_vbt \
         i915_cdclk_info i915_gem_framebuffer i915_dp_mst_info i915_lpsp_status i915_ddi_buf_trans; do
  [ -r "$card/$f" ] && cat "$card/$f" > "$out/$f.txt" 2>/dev/null
done
for c in /sys/class/drm/card*-*; do
  n=$(basename "$c")
  { echo "status=$(cat "$c/status") enabled=$(cat "$c/enabled")"; cat "$c/modes"; } > "$out/conn-$n.txt"
  [ -s "$c/edid" ] && cp "$c/edid" "$out/edid-$n.bin"
done
dmesg | grep -iE 'i915|drm|dmc|typec|ucsi|thunderbolt' > "$out/dmesg-gfx.txt"
lspci -vvv -s 00:02.0 > "$out/lspci.txt" 2>&1
lspci -nn > "$out/lspci-all.txt" 2>&1
if command -v intel_reg >/dev/null; then
  intel_reg dump > "$out/intel_reg_dump.txt" 2>&1
  for r in "$@"; do intel_reg read "$r" >> "$out/intel_reg_extra.txt" 2>&1; done
fi
echo BEGIN
tar czf - -C "$out" . | base64 -w0
echo
echo END
rm -rf "$out"
