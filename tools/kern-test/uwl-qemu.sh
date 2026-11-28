# Started on the Ultar kernel by wayland-qemu.py (busybox sh): the compositor
# on the framebuffer, then the Wayland bridge with the static demo app.
mkdir -p /tmp/uwl/run
/bin/ultar-compositor --socket /tmp/uwl/compositor --allow-root --lang en --no-cursor &
sleep 3
/bin/ultar-wayland --compositor /tmp/uwl/compositor --runtime-dir /tmp/uwl/run \
  --app-id org.ultar.uwl-demo -- /bin/uwl-demo 640 400 &
sleep 2
echo "uwl-qemu: started"
