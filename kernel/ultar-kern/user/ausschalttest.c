/* Ausschalten/Neustart wie ultar-init am Ende: reboot(2) aus einem Prozess, dessen Benutzerfenster
   [4 MiB, 256 MiB) belegt ist (dort lagen in VMware die Laufzeitdienste der Firmware).
   ausschalttest aus | neustart – die virtuelle Maschine muss danach enden. */
#include <stdio.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/reboot.h>
#include <unistd.h>

int main(int argc, char **argv) {
    int aus = argc > 1 && strcmp(argv[1], "aus") == 0;
    /* feste Abbildungen im Fenster, jede Seite berührt (soweit der Speicher reicht) */
    for (unsigned long a = 0x2000000; a < 0x10000000; a += 0x1000000) {
        void *p = mmap((void *)a, 0x1000000, PROT_READ | PROT_WRITE | PROT_EXEC, MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED_NOREPLACE, -1, 0);
        if (p == MAP_FAILED) continue;
        for (unsigned long o = 0; o < 0x1000000; o += 4096) ((volatile unsigned char *)p)[o] = 0x0f; /* 0F 0F: kein gültiger Befehl */
    }
    printf("AUSSCHALTTEST: %s\n", aus ? "aus" : "neustart");
    fflush(stdout);
    sync();
    reboot(aus ? RB_POWER_OFF : RB_AUTOBOOT);
    printf("AUSSCHALTTEST: reboot kehrte zurück\n");
    return 1;
}
