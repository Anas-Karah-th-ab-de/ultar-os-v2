# Testprogramme für den Ultar-Kern (Ring 3, statisch, Linux-Systemaufrufe)
all: hallo hallo-glibc kerntest ereignis fadentest blocktest lauscher zelltest grenztest ausbruch rechtetest geraetetest weckertest spawntest ext4test fenstertest bttest
hallo: hallo.c
	gcc -Os -fno-builtin -ffreestanding -static -nostdlib -fno-pie -no-pie -fno-stack-protector -o $@ $<
hallo-glibc: hallo-glibc.c
	gcc -O2 -static -o $@ $<
clean:
	rm -f hallo hallo-glibc kerntest ereignis fadentest blocktest lauscher zelltest grenztest ausbruch rechtetest geraetetest weckertest spawntest ext4test fenstertest bttest
kerntest: kerntest.c
	gcc -O2 -static -pthread -o $@ $<
ereignis: ereignis.c
	gcc -O2 -static -o $@ $<
fadentest: fadentest.c
	gcc -O2 -static -pthread -o $@ $<
blocktest: blocktest.c
	gcc -O2 -static -pthread -o $@ $<
lauscher: lauscher.c
	gcc -O2 -static -o $@ $<
zelltest: zelltest.c zelltest-zelle.c zelltest.h
	gcc -O2 -Wall -Wextra -static -o $@ zelltest.c zelltest-zelle.c
grenztest: grenztest.c grenztest-grenzen.c grenztest-angriff.c grenztest.h
	gcc -O2 -Wall -Wextra -static -o $@ grenztest.c grenztest-grenzen.c grenztest-angriff.c
# the security reviewer's attack program (its --dos modes are run by grenztest)
ausbruch: ../../../tools/kern-test/attacks/ausbruch.c
	gcc -O2 -static -pthread -Wno-unused-result -o $@ $<
# identities: user/group ids, file modes and owners, Unix socket nodes and SO_PEERCRED, fsync
rechtetest: rechtetest.c
	gcc -O2 -Wall -Wextra -static -o $@ $<
# device interfaces: device list, driver register, bind/unbind, ABI, uevents, health, rights
geraetetest: geraetetest.c
	gcc -O2 -Wall -Wextra -static -o $@ $<
# wake-ups: posix_spawn/vfork at execve, AF_UNIX shutdown/close, accept, signalfd with threads
weckertest: weckertest.c
	gcc -O2 -Wall -Wextra -static -pthread -o $@ $<
# flock and process start (vfork released at exec), as the system updater needs them
spawntest: spawntest.c
	gcc -O2 -Wall -Wextra -static -o $@ $<
# durable whole-file writes on ext4 (extent trees, honest fsync/close), with a hard stop in between
ext4test: ext4test.c
	gcc -O2 -Wall -Wextra -static -o $@ $<
# what a compositor needs: memfd and seals, shared mappings, SCM_RIGHTS, MSG_DONTWAIT, POLLOUT, poll timeouts,
# mremap, fb0 mmap and vblank, device node modes (seat group), evdev ioctls, /sys/class/input
fenstertest: fenstertest.c fenstertest-geraete.c fenstertest.h
	gcc -O2 -Wall -Wextra -static -pthread -o $@ fenstertest.c fenstertest-geraete.c
# device nodes of the privileged services: /dev/ultar/uinput (virtual input devices) and /dev/ultar/hciN
bttest: bttest.c
	gcc -O2 -Wall -Wextra -static -o $@ $<
