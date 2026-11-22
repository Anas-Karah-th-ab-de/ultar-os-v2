# Testprogramme für den Ultar-Kern (Ring 3, statisch, Linux-Systemaufrufe)
all: hallo hallo-glibc kerntest ereignis fadentest blocktest lauscher
hallo: hallo.c
	gcc -Os -fno-builtin -ffreestanding -static -nostdlib -fno-pie -no-pie -fno-stack-protector -o $@ $<
hallo-glibc: hallo-glibc.c
	gcc -O2 -static -o $@ $<
clean:
	rm -f hallo hallo-glibc kerntest ereignis fadentest blocktest lauscher
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
