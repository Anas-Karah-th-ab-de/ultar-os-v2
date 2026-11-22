// Ultar-Prüfstart: kleinstes mögliches /init ohne jede Bibliothek (nur Systemaufrufe, arm64).
// Zweck: auf einem Handy ohne Konsole feststellen, ob der Kernel unser Ramdisk überhaupt startet.
// Ablauf: 2 s lang vibrieren, dann je bedientem Wachhund ein kurzer Stoß, 3 Herzschläge (alle 2 s),
// danach /sbin/init starten; ein Kindprozess bedient die Wachhunde weiter.
// Startet das Handy schon vor dem langen Vibrieren neu, liegt es am Kernel/Abbild; erst nach den Herzschlägen: an ultar-init.
typedef unsigned long u64;
static long sc(long n, long a, long b, long c, long d, long e) {
    register long x8 __asm__("x8") = n, x0 __asm__("x0") = a, x1 __asm__("x1") = b, x2 __asm__("x2") = c, x3 __asm__("x3") = d, x4 __asm__("x4") = e;
    __asm__ volatile("svc 0" : "+r"(x0) : "r"(x8), "r"(x1), "r"(x2), "r"(x3), "r"(x4) : "memory");
    return x0;
}
static int len(const char *s) { int n = 0; while (s[n]) n++; return n; }
static void wr(const char *path, const char *v) {
    long fd = sc(56, -100, (long)path, 1 /*O_WRONLY*/, 0, 0);
    if (fd >= 0) { sc(64, fd, (long)v, len(v), 0, 0); sc(57, fd, 0, 0, 0, 0); }
}
static void sleep_ms(long ms) { long ts[2] = { ms / 1000, (ms % 1000) * 1000000 }; sc(101, (long)ts, 0, 0, 0, 0); }
static void vib(const char *ms) {
    wr("/sys/class/timed_output/vibrator/enable", ms);
    wr("/sys/class/leds/vibrator/duration", ms); wr("/sys/class/leds/vibrator/activate", "1");
}
static long wd[4]; static int nwd;
static void pet(void) { for (int i = 0; i < nwd; i++) sc(64, wd[i], (long)"\0", 1, 0, 0); }
static void add_wd(const char *path) { long fd = sc(56, -100, (long)path, 1, 0, 0); if (fd >= 0 && nwd < 4) wd[nwd++] = fd; }
// Wachhund ohne devtmpfs/ueventd: Gerätenummer aus /sys lesen und den Knoten selbst anlegen.
static void add_wd_sys(const char *sysdev, const char *node) {
    char b[32]; long fd = sc(56, -100, (long)sysdev, 0, 0, 0); if (fd < 0) return;
    long n = sc(63, fd, (long)b, 31, 0, 0); sc(57, fd, 0, 0, 0, 0); if (n <= 0) return;
    long maj = 0, min = 0; int i = 0;
    for (; i < n && b[i] >= '0' && b[i] <= '9'; i++) maj = maj * 10 + (b[i] - '0');
    for (i++; i < n && b[i] >= '0' && b[i] <= '9'; i++) min = min * 10 + (b[i] - '0');
    long dev = (min & 0xff) | (maj << 8) | ((min & ~0xffL) << 12);
    sc(33, -100, (long)node, 020600, dev, 0);
    add_wd(node);
}
// Pause in 500-ms-Schritten, dabei alle Wachhunde bedienen
static void wait_pet(int halves) { for (int i = 0; i < halves; i++) { sleep_ms(500); pet(); } }

// --- Anzeige-Test: Framebuffer mit einer Farbe füllen, Hintergrundlicht auf Maximum ---
static void write_all(const char *path, const char *v) { wr(path, v); }
static void set_backlight_max(void) {
    // bekannte Knoten des A30 (und Allgemeinfälle) auf Maximum
    const char *bl[] = { "/sys/class/backlight/panel/brightness", "/sys/class/backlight/panel/bl_power",
                         "/sys/class/leds/lcd-backlight/brightness", 0 };
    write_all("/sys/class/backlight/panel/bl_power", "0");
    write_all("/sys/class/backlight/panel/brightness", "255");
    write_all("/sys/class/backlight/panel/brightness", "306");
    write_all("/sys/class/leds/lcd-backlight/brightness", "255");
    (void)bl;
}
static void fb_fill(unsigned int color) {
    long fd = sc(56, -100, (long)"/dev/fb0", 2 /*O_RDWR*/, 0, 0);
    if (fd < 0) { sc(33, -100, (long)"/dev/fb0", 020600, (29 << 8) | 0, 0); fd = sc(56, -100, (long)"/dev/fb0", 2, 0, 0); }
    if (fd < 0) return;
    sc(29, fd, 0x4611, 4, 0, 0);          // FBIOBLANK POWERDOWN: DECON aus INIT nach OFF zwingen
    sleep_ms(80);
    sc(29, fd, 0x4611, 0, 0, 0);          // FBIOBLANK UNBLANK: jetzt volle Panel-Einschaltsequenz
    static unsigned int buf[1080];
    for (int i = 0; i < 1080; i++) buf[i] = color;
    for (int y = 0; y < 2340; y++) sc(64, fd, (long)buf, 1080 * 4, 0, 0);  // Zeile für Zeile schreiben
    sc(62, fd, 0, 0, 0, 0);               // lseek zurück (SEEK_SET via separate) – unkritisch
    sc(29, fd, 0x4606, 0, 0, 0);          // FBIOPAN_DISPLAY anstoßen (ohne var: viele Treiber nehmen 0)
    sc(57, fd, 0, 0, 0, 0);
}


// Direkt in physischen Speicher schreiben (Panel-Puffer des Bootloaders bei 0xec000000).
static void devmem_fill(unsigned long phys, unsigned int color, int rows) {
    long fd = sc(56, -100, (long)"/dev/mem", 2 /*O_RDWR*/, 0, 0);
    if (fd < 0) { sc(33, -100, (long)"/dev/mem", 020600, (1 << 8) | 1, 0); fd = sc(56, -100, (long)"/dev/mem", 2, 0, 0); }
    if (fd < 0) { wr("/dev/kmsg", "<1>ULTAR: /dev/mem nicht offen\n"); return; }
    static unsigned int line[1080];
    for (int i = 0; i < 1080; i++) line[i] = color;
    long off = (long)phys;
    for (int y = 0; y < rows; y++) { sc(68 /*pwrite64*/, fd, (long)line, 1080 * 4, off, 0); off += 1080 * 4; }
    sc(57, fd, 0, 0, 0, 0);
    wr("/dev/kmsg", "<1>ULTAR: devmem geschrieben\n");
}

void _start(void) {
    sc(34, -100, (long)"/sys", 0755, 0, 0); sc(34, -100, (long)"/dev", 0755, 0, 0); sc(34, -100, (long)"/proc", 0755, 0, 0);
    sc(40, (long)"sysfs", (long)"/sys", (long)"sysfs", 0, 0);
    sc(40, 0, (long)"/", 0, 32 /*MS_REMOUNT*/, 0);  // Wurzel beschreibbar (System-als-Root: Kernel hängt sie nur lesbar ein)
    if (sc(40, (long)"devtmpfs", (long)"/dev", (long)"devtmpfs", 0, 0) < 0)
        sc(40, (long)"tmpfs", (long)"/dev", (long)"tmpfs", 0, (long)"mode=0755");
    sc(40, (long)"proc", (long)"/proc", (long)"proc", 0, 0);
    // Ohne devtmpfs (viele Android-Kernel) die nötigsten Geräteknoten selbst anlegen; mit devtmpfs schlägt das harmlos fehl
    sc(33, -100, (long)"/dev/null", 020666, (1 << 8) | 3, 0); sc(33, -100, (long)"/dev/zero", 020666, (1 << 8) | 5, 0);
    sc(33, -100, (long)"/dev/urandom", 020666, (1 << 8) | 9, 0); sc(33, -100, (long)"/dev/kmsg", 020600, (1 << 8) | 11, 0);
    sc(33, -100, (long)"/dev/tty", 020666, (5 << 8) | 0, 0); sc(33, -100, (long)"/dev/console", 020600, (5 << 8) | 1, 0);
    wr("/dev/kmsg", "<1>ULTAR-PRUEFSTART: init laeuft\n");
    add_wd("/dev/watchdog");
    if (!nwd) { sc(33, -100, (long)"/dev/watchdog", 020600, (10 << 8) | 130, 0); add_wd("/dev/watchdog"); }
    add_wd_sys("/sys/class/watchdog/watchdog0/dev", "/dev/.uwd0");
    add_wd_sys("/sys/class/watchdog/watchdog1/dev", "/dev/.uwd1");
    pet();
    vib("2000"); wait_pet(6);                       // 2 s lang, 1 s Pause
    for (int i = 0; i < nwd; i++) { vib("300"); wait_pet(2); } // so viele kurze Stöße wie Wachhunde bedient werden
    wait_pet(4);
    for (int i = 0; i < 3; i++) { vib("150"); wait_pet(4); }   // 6 s Herzschlag
    set_backlight_max();
    static char *argv[] = { "/sbin/init", 0 }; static char *envp[] = { "PATH=/sbin:/bin:/usr/bin", 0 };
    long pid = sc(220, 17 /*SIGCHLD*/, 0, 0, 0, 0);            // Kind bedient die Wachhunde weiter, PID 1 wird ultar-init
    if (pid == 0) { for (;;) { sleep_ms(500); pet(); } }
    sc(221, (long)"/sbin/init", (long)argv, (long)envp, 0, 0);
    for (;;) { sleep_ms(1000); vib("1000"); pet(); } // Start von ultar-init misslungen: Dauer-Vibrieren im Sekundentakt
}
