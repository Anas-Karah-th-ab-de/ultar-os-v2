/* uinject – Eingaben in ein evdev-Gerät einspeisen (Gerätetests ohne Finger).
 * uinject <dev> tap <x> <y> [haltezeit_ms]   Koordinaten in Touch-Einheiten (siehe "info")
 * uinject <dev> swipe <x0> <y0> <x1> <y1> [ms]
 * uinject <dev> key <code> [haltezeit_ms]       z. B. 114 Leiser, 116 Ein/Aus
 * uinject <dev> keys <code1> <code2>            beide gleichzeitig (Bildschirmfoto)
 * uinject <dev> info                            Wertebereich ABS_MT_POSITION_X/Y
 * Schreiben in /dev/input/eventN speist die Ereignisse für alle Leser dieses Geräts ein. */
#include <fcntl.h>
#include <linux/input.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <time.h>
#include <unistd.h>

static int fd;
static void ev(int t, int c, int v) {
    struct input_event e; memset(&e, 0, sizeof e);
    e.type = t; e.code = c; e.value = v;
    if (write(fd, &e, sizeof e) != sizeof e) perror("write");
}
static void syn(void) { ev(EV_SYN, SYN_REPORT, 0); }
static void ms(int m) { struct timespec t = { m / 1000, (m % 1000) * 1000000L }; nanosleep(&t, 0); }
static int tid = 100;
static void down(int x, int y) {
    ev(EV_ABS, ABS_MT_SLOT, 0); ev(EV_ABS, ABS_MT_TRACKING_ID, tid++);
    ev(EV_ABS, ABS_MT_POSITION_X, x); ev(EV_ABS, ABS_MT_POSITION_Y, y);
    ev(EV_KEY, BTN_TOUCH, 1); syn();
}
static void move(int x, int y) { ev(EV_ABS, ABS_MT_POSITION_X, x); ev(EV_ABS, ABS_MT_POSITION_Y, y); syn(); }
static void up(void) { ev(EV_ABS, ABS_MT_TRACKING_ID, -1); ev(EV_KEY, BTN_TOUCH, 0); syn(); }

int main(int c, char **v) {
    if (c < 3) { fprintf(stderr, "uinject <dev> tap|swipe|key|keys|info ...\n"); return 2; }
    fd = open(v[1], O_RDWR);
    if (fd < 0) { perror(v[1]); return 1; }
    if (!strcmp(v[2], "info")) {
        struct input_absinfo a;
        char name[128] = "";
        ioctl(fd, EVIOCGNAME(sizeof name), name);
        printf("name=%s\n", name);
        if (!ioctl(fd, EVIOCGABS(ABS_MT_POSITION_X), &a)) printf("x %d..%d\n", a.minimum, a.maximum);
        if (!ioctl(fd, EVIOCGABS(ABS_MT_POSITION_Y), &a)) printf("y %d..%d\n", a.minimum, a.maximum);
        return 0;
    }
    if (!strcmp(v[2], "tap") && c >= 5) {
        down(atoi(v[3]), atoi(v[4])); ms(c > 5 ? atoi(v[5]) : 60); up(); return 0;
    }
    if (!strcmp(v[2], "swipe") && c >= 7) {
        int x0 = atoi(v[3]), y0 = atoi(v[4]), x1 = atoi(v[5]), y1 = atoi(v[6]), d = c > 7 ? atoi(v[7]) : 300, n = 15;
        down(x0, y0);
        for (int i = 1; i <= n; i++) { ms(d / n); move(x0 + (x1 - x0) * i / n, y0 + (y1 - y0) * i / n); }
        up(); return 0;
    }
    if (!strcmp(v[2], "key") && c >= 4) {
        int k = atoi(v[3]); ev(EV_KEY, k, 1); syn(); ms(c > 4 ? atoi(v[4]) : 80); ev(EV_KEY, k, 0); syn(); return 0;
    }
    if (!strcmp(v[2], "keys") && c >= 5) {
        int a = atoi(v[3]), b = atoi(v[4]);
        ev(EV_KEY, a, 1); syn(); ms(150); ev(EV_KEY, b, 1); syn(); ms(120);
        ev(EV_KEY, b, 0); syn(); ev(EV_KEY, a, 0); syn(); return 0;
    }
    fprintf(stderr, "unbekannt\n"); return 2;
}
