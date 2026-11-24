/* bttest: the device nodes of the privileged services (kernel/ultar-kern/src/svcdev).
 *   bttest uinput – /dev/ultar/uinput: rights, a virtual keyboard/mouse/tablet that the seat sees as
 *                   /dev/input/eventN (EVIOCG*, events in order), undeclared events refused, rate limit,
 *                   limit of devices, gone when its descriptor closes          → "BTTEST-UINPUT: ALLES OK"
 *   bttest hci    – /dev/ultar/hci0 of a USB Bluetooth controller (QEMU usb-redir + tools/kern-test/bt-usb.py):
 *                   rights, one opener, HCI Reset, Read BD_ADDR, an event cut over 17 USB packets,
 *                   ACL both ways, refused records, O_NONBLOCK and poll            → "BTTEST-HCI: ALLES OK"
 */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <grp.h>
#include <linux/input.h>
#include <linux/uinput.h>
#include <poll.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/prctl.h>
#include <sys/stat.h>
#include <sys/sysmacros.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#define PR_ULTAR_RIGHTS_DROP 0x554c5201
#define R_RAW_DEVICE 0x20UL
#define R_FILE_ADMIN 0x200UL
#define R_BLUETOOTH 0x400UL
#define R_INPUT_INJECT 0x800UL
#define BTD_UID 827
#define UINPUT "/dev/ultar/uinput"
#define HCI "/dev/ultar/hci0"

static int fails;

static void check(const char *name, int ok) {
    printf("%s %s\n", ok ? "ok " : "FEHLER", name);
    fflush(stdout);
    if (!ok) fails++;
}

/* run fn as uid with only the rights `keep`; its result counts here */
static void as(uid_t uid, unsigned long keep, int (*fn)(void)) {
    pid_t c = fork();
    if (c == 0) {
        fails = 0;
        int ok = setgroups(0, NULL) == 0 && setresgid(uid, uid, uid) == 0 && setresuid(uid, uid, uid) == 0;
        prctl(PR_ULTAR_RIGHTS_DROP, ~keep, 0, 0, 0);
        if (!ok) _exit(99);
        _exit(fn() ? 1 : 0);
    }
    int st = -1;
    waitpid(c, &st, 0);
    if (!WIFEXITED(st) || WEXITSTATUS(st) == 99) check("Hilfsprozess mit eigener Kennung lief", 0);
    else if (WEXITSTATUS(st)) fails++;
}

static int open_err(const char *path) {
    int fd = open(path, O_RDWR | O_CLOEXEC | O_NONBLOCK);
    if (fd >= 0) {
        close(fd);
        return 0;
    }
    return errno;
}

static const char *node_path;
static int no_right(void) {
    check("ohne das Recht (RAW_DEVICE + FILE_ADMIN): EACCES", open_err(node_path) == EACCES);
    return fails;
}
static int with_right(void) {
    check("Dienst-Kennung 827 mit dem Recht: öffnet", open_err(node_path) == 0);
    return fails;
}

static void rights(const char *path, unsigned long right, unsigned minor) {
    struct stat st;
    check("Knoten: Zeichengerät 10:N, root 0600", stat(path, &st) == 0 && S_ISCHR(st.st_mode) && major(st.st_rdev) == 10 &&
          minor(st.st_rdev) == minor && (st.st_mode & 0777) == 0600);
    node_path = path;
    as(BTD_UID, R_RAW_DEVICE | R_FILE_ADMIN | ((R_BLUETOOTH | R_INPUT_INJECT) & ~right), no_right);
    as(0, R_RAW_DEVICE | R_FILE_ADMIN, no_right);
    as(BTD_UID, right, with_right);
}

/* ================================================================ uinput */

static void set(int fd, unsigned long req, int v) {
    if (ioctl(fd, req, v) != 0) check("UI_SET_*BIT", 0);
}

static int ev_write(int fd, const int (*ev)[3], int n) {
    struct input_event e[300];
    memset(e, 0, sizeof e);
    for (int i = 0; i < n; i++) e[i].type = ev[i][0], e[i].code = ev[i][1], e[i].value = ev[i][2];
    long r = write(fd, e, sizeof(struct input_event) * n);
    return r == (long)(sizeof(struct input_event) * n) ? 0 : (r < 0 ? errno : -1);
}

/* the event node of a created device (UI_GET_SYSNAME "inputN" → /dev/input/eventN) */
static int event_node(int ufd, char *path) {
    char sys[32] = {0};
    if (ioctl(ufd, UI_GET_SYSNAME(sizeof sys), sys) < 0 || strncmp(sys, "input", 5)) return -1;
    sprintf(path, "/dev/input/event%s", sys + 5);
    return open(path, O_RDONLY | O_NONBLOCK | O_CLOEXEC);
}

static int keyboard(const char *name) {
    int fd = open(UINPUT, O_WRONLY | O_CLOEXEC | O_NONBLOCK);
    set(fd, UI_SET_EVBIT, EV_KEY);
    set(fd, UI_SET_KEYBIT, KEY_A);
    set(fd, UI_SET_KEYBIT, KEY_LEFTSHIFT);
    struct uinput_setup s = {.id = {.bustype = BUS_BLUETOOTH, .vendor = 0x1d6b, .product = 0x4b42, .version = 1}};
    strcpy(s.name, name);
    if (ioctl(fd, UI_DEV_SETUP, &s) != 0 || ioctl(fd, UI_DEV_CREATE) != 0) {
        close(fd);
        return -1;
    }
    return fd;
}

static void keyboard_checks(void) {
    int fd = keyboard("Ultar Test\x01Tastatur");
    char path[64];
    int ev = event_node(fd, path);
    check("Tastatur angelegt, eigener Eingabeknoten", fd >= 0 && ev >= 0);
    char name[128] = {0};
    unsigned char bits[96] = {0}, types[4] = {0};
    struct input_id id;
    ioctl(ev, EVIOCGNAME(sizeof name), name);
    ioctl(ev, EVIOCGBIT(0, sizeof types), types);
    ioctl(ev, EVIOCGBIT(EV_KEY, sizeof bits), bits);
    check("EVIOCGNAME: Name ohne Steuerzeichen", strcmp(name, "Ultar TestTastatur") == 0);
    check("EVIOCGID: Bluetooth, Hersteller/Produkt", ioctl(ev, EVIOCGID, &id) == 0 && id.bustype == BUS_BLUETOOTH && id.product == 0x4b42);
    check("EVIOCGBIT: nur die erklärten Tasten", (types[0] & 2) && !(types[0] & 4) && (bits[KEY_A / 8] & 1 << KEY_A % 8) && !(bits[KEY_B / 8] & 1 << KEY_B % 8));
    char sysname[128] = {0};
    snprintf(sysname, sizeof sysname, "/sys/class/input/%s/device/name", path + 11);
    FILE *f = fopen(sysname, "r");
    char line[128] = {0};
    check("/sys/class/input/eventN/device/name", f && fgets(line, sizeof line, f) && strcmp(line, "Ultar TestTastatur\n") == 0);
    if (f) fclose(f);
    const int press[4][3] = {{EV_KEY, KEY_A, 1}, {EV_SYN, 0, 0}, {EV_KEY, KEY_A, 0}, {EV_SYN, 0, 0}};
    check("Ereignisse schreiben", ev_write(fd, press, 4) == 0);
    struct pollfd p = {ev, POLLIN, 0};
    check("Sitz sieht POLLIN am Eingabeknoten", poll(&p, 1, 1000) == 1 && (p.revents & POLLIN));
    struct input_event got[8];
    long n = read(ev, got, sizeof got);
    check("Sitz liest Drücken, SYN, Loslassen, SYN in Reihenfolge", n == 4 * (long)sizeof(struct input_event) && got[0].type == EV_KEY &&
          got[0].code == KEY_A && got[0].value == 1 && got[1].type == EV_SYN && got[2].value == 0 && got[3].type == EV_SYN);
    const int undeclared[2][3] = {{EV_KEY, KEY_B, 1}, {EV_SYN, 0, 0}};
    const int rel[1][3] = {{EV_REL, REL_X, 3}};
    const int badvalue[1][3] = {{EV_KEY, KEY_A, 7}};
    check("nicht erklärte Taste: EINVAL", ev_write(fd, undeclared, 2) == EINVAL);
    check("Achse an einer Tastatur: EINVAL", ev_write(fd, rel, 1) == EINVAL);
    check("Tastenwert 7: EINVAL", ev_write(fd, badvalue, 1) == EINVAL);
    check("nichts davon kam an", read(ev, got, sizeof got) < 0 && errno == EAGAIN);
    check("halber Datensatz: EINVAL", write(fd, got, 23) < 0 && errno == EINVAL);
    check("nach dem Anlegen keine neuen Fähigkeiten", ioctl(fd, UI_SET_KEYBIT, KEY_B) < 0 && errno == EINVAL);
    check("Lesen am uinput-Knoten: EINVAL", read(fd, got, sizeof got) < 0 && errno == EINVAL);
    close(fd);
    struct stat st;
    check("Deskriptor zu: Gerät und Knoten weg", stat(path, &st) < 0 && errno == ENOENT);
    check("Leser bekommt ENODEV", read(ev, got, sizeof got) < 0 && errno == ENODEV);
    close(ev);
}

/* what ultar-btd does through ultar-hal-linux (uinput.rs): integer ioctls + the legacy setup record */
static void legacy_mouse_checks(void) {
    int fd = open(UINPUT, O_WRONLY | O_CLOEXEC | O_NONBLOCK);
    set(fd, UI_SET_EVBIT, EV_KEY);
    set(fd, UI_SET_EVBIT, EV_REL);
    for (int b = BTN_LEFT; b <= BTN_MIDDLE; b++) set(fd, UI_SET_KEYBIT, b);
    set(fd, UI_SET_RELBIT, REL_X);
    set(fd, UI_SET_RELBIT, REL_Y);
    set(fd, UI_SET_RELBIT, REL_WHEEL);
    struct uinput_user_dev u;
    memset(&u, 0, sizeof u);
    strcpy(u.name, "BT Maus");
    u.id.bustype = BUS_BLUETOOTH;
    check("uinput_user_dev (1116 Bytes) schreiben", write(fd, &u, sizeof u) == (long)sizeof u && sizeof u == 1116);
    check("UI_DEV_CREATE", ioctl(fd, UI_DEV_CREATE) == 0);
    char path[64];
    int ev = event_node(fd, path);
    unsigned char rel[2] = {0};
    ioctl(ev, EVIOCGBIT(EV_REL, sizeof rel), rel);
    check("Maus: X, Y, Rad", ev >= 0 && rel[0] == 0x03 && rel[1] == 0x01);
    const int move[3][3] = {{EV_REL, REL_X, -5}, {EV_KEY, BTN_LEFT, 1}, {EV_SYN, 0, 0}};
    struct input_event got[4];
    check("Bewegung und Taste", ev_write(fd, move, 3) == 0 && read(ev, got, sizeof got) == 3 * (long)sizeof(struct input_event) && got[0].value == -5 &&
          got[1].code == BTN_LEFT);
    /* rate limit: the bucket holds 256 events, refilled at 1000 per second */
    int burst[256][3];
    for (int i = 0; i < 256; i++) burst[i][0] = EV_REL, burst[i][1] = REL_X, burst[i][2] = 1;
    int r = ev_write(fd, (const int (*)[3])burst, 256);
    if (r == EAGAIN) {
        usleep(300000);
        r = ev_write(fd, (const int (*)[3])burst, 256);
    }
    check("256 Ereignisse auf einmal", r == 0);
    check("sofort danach 64 weitere: EAGAIN (Ratenbegrenzung)", ev_write(fd, (const int (*)[3])burst, 64) == EAGAIN);
    usleep(150000);
    check("nach 150 ms gehen 64 wieder", ev_write(fd, (const int (*)[3])burst, 64) == 0);
    struct input_event many[257];
    memset(many, 0, sizeof many);
    check("257 Ereignisse in einem Aufruf: EINVAL", write(fd, many, sizeof many) < 0 && errno == EINVAL);
    check("UI_DEV_DESTROY: Knoten weg", ioctl(fd, UI_DEV_DESTROY) == 0 && access(path, F_OK) != 0);
    close(ev);
    close(fd);
}

static void tablet_and_limit_checks(void) {
    int fd = open(UINPUT, O_WRONLY | O_CLOEXEC | O_NONBLOCK);
    set(fd, UI_SET_EVBIT, EV_ABS);
    set(fd, UI_SET_ABSBIT, ABS_X);
    set(fd, UI_SET_ABSBIT, ABS_Y);
    struct uinput_abs_setup a = {.code = ABS_X, .absinfo = {.minimum = 0, .maximum = 1000}};
    struct uinput_abs_setup bad = {.code = ABS_Y, .absinfo = {.minimum = 10, .maximum = 5}};
    check("UI_ABS_SETUP; Minimum > Maximum: EINVAL", ioctl(fd, UI_ABS_SETUP, &a) == 0 && ioctl(fd, UI_ABS_SETUP, &bad) < 0 && errno == EINVAL);
    check("Tablett angelegt", ioctl(fd, UI_DEV_CREATE) == 0);
    char path[64];
    int ev = event_node(fd, path);
    struct input_absinfo ai;
    check("EVIOCGABS(ABS_X): 0..1000", ioctl(ev, EVIOCGABS(ABS_X), &ai) == 0 && ai.minimum == 0 && ai.maximum == 1000);
    const int far[2][3] = {{EV_ABS, ABS_X, 5000}, {EV_SYN, 0, 0}};
    struct input_event got[2];
    check("Wert außerhalb wird auf den Bereich gesetzt", ev_write(fd, far, 2) == 0 && read(ev, got, sizeof got) > 0 && got[0].value == 1000);
    close(ev);
    /* at most 8 virtual devices at once */
    int fds[9], made = 1;
    for (int i = 0; i < 9; i++) {
        fds[i] = open(UINPUT, O_WRONLY | O_CLOEXEC);
        set(fds[i], UI_SET_EVBIT, EV_KEY);
        set(fds[i], UI_SET_KEYBIT, KEY_A);
        if (ioctl(fds[i], UI_DEV_CREATE) == 0) made++;
        else if (errno != ENOSPC) check("Grenze: ENOSPC", 0);
    }
    check("höchstens 8 virtuelle Geräte (das neunte: ENOSPC)", made == 8);
    for (int i = 0; i < 9; i++) close(fds[i]);
    close(fd);
    int empty = open(UINPUT, O_WRONLY | O_CLOEXEC);
    check("ohne erklärte Ereignisse kein Gerät", ioctl(empty, UI_DEV_CREATE) < 0 && errno == EINVAL);
    close(empty);
    /* a process that ends without closing: its device goes with it */
    char cpath[64] = {0};
    int pipefd[2];
    if (pipe(pipefd)) check("pipe", 0);
    pid_t c = fork();
    if (c == 0) {
        int k = keyboard("Kind");
        char p[64] = {0};
        int e = event_node(k, p);
        (void)e;
        if (write(pipefd[1], p, sizeof p) < 0) _exit(1);
        _exit(0);
    }
    if (read(pipefd[0], cpath, sizeof cpath) <= 0) cpath[0] = 0;
    waitpid(c, NULL, 0);
    check("Prozess endet ohne close: Gerät weg", cpath[0] && access(cpath, F_OK) != 0);
}

static int btd_creates(void) {
    int fd = keyboard("btd");
    check("Dienst (827, nur INPUT_INJECT) legt eine Tastatur an", fd >= 0);
    close(fd);
    return fails;
}

static void uinput_test(void) {
    rights(UINPUT, R_INPUT_INJECT, 223);
    as(BTD_UID, R_INPUT_INJECT, btd_creates);
    keyboard_checks();
    legacy_mouse_checks();
    tablet_and_limit_checks();
    printf("BTTEST-UINPUT: %s\n", fails ? "FEHLER" : "ALLES OK");
}

/* ================================================================ hci */

static long rec(int fd, unsigned char *b, size_t n, int ms) {
    struct pollfd p = {fd, POLLIN, 0};
    if (poll(&p, 1, ms) != 1) return -2;
    return read(fd, b, n);
}

/* send command `op` with no parameters; the matching Command Complete (skipping other events) */
static long cmd(int fd, unsigned op, unsigned char *b, size_t n) {
    unsigned char c[4] = {1, op & 0xff, op >> 8, 0};
    if (write(fd, c, 4) != 4) return -1;
    for (int i = 0; i < 8; i++) {
        long r = rec(fd, b, n, 3000);
        if (r < 0) return r;
        if (r >= 7 && b[0] == 4 && b[1] == 0x0e && b[4] == (op & 0xff) && b[5] == op >> 8) return r;
    }
    return -1;
}

static void hci_test(void) {
    rights(HCI, R_BLUETOOTH, 240);
    int fd = open(HCI, O_RDWR | O_CLOEXEC | O_NONBLOCK);
    check("hci0 öffnen", fd >= 0);
    check("zweiter Öffner: EBUSY", open_err(HCI) == EBUSY);
    unsigned char b[1100];
    check("leer und O_NONBLOCK: EAGAIN", read(fd, b, sizeof b) < 0 && errno == EAGAIN);
    struct pollfd p = {fd, POLLIN, 0};
    check("poll ohne Daten: Frist läuft ab", poll(&p, 1, 100) == 0);
    long n = cmd(fd, 0x0c03, b, sizeof b);
    check("HCI Reset: Command Complete, Status 0", n == 7 && b[6] == 0);
    n = cmd(fd, 0x1009, b, sizeof b);
    const unsigned char addr[6] = {0x01, 0xB7, 0x00, 0xEE, 0xFF, 0xC0};
    check("Read BD_ADDR: C0:FF:EE:00:B7:01", n == 13 && b[6] == 0 && memcmp(b + 7, addr, 6) == 0);
    unsigned char c[4] = {1, 0x77, 0xfc, 0};
    int big = write(fd, c, 4) == 4 && rec(fd, b, sizeof b, 3000) == 258 && b[0] == 4 && b[1] == 0xff && b[2] == 255;
    for (int i = 0; big && i < 255; i++) big = b[3 + i] == i;
    check("Ereignis mit 255 Parametern über 17 USB-Pakete zusammengesetzt", big);
    check("danach dessen Command Complete", rec(fd, b, sizeof b, 3000) == 7 && b[1] == 0x0e);
    unsigned char acl[1 + 4 + 1021];
    acl[0] = 2, acl[1] = 0x01, acl[2] = 0x20, acl[3] = 300 & 0xff, acl[4] = 300 >> 8;
    for (int i = 0; i < 1021; i++) acl[5 + i] = (unsigned char)(i * 7);
    check("ACL 300 Bytes hinaus", write(fd, acl, 305) == 305);
    n = rec(fd, b, sizeof b, 3000);
    check("ACL 300 Bytes zurück (Bulk IN)", n == 305 && memcmp(b, acl, 305) == 0);
    acl[3] = 1021 & 0xff, acl[4] = 1021 >> 8;
    check("ACL 1021 Bytes hin und zurück", write(fd, acl, sizeof acl) == (long)sizeof acl && rec(fd, b, sizeof b, 3000) == (long)sizeof acl &&
          memcmp(b, acl, sizeof acl) == 0);
    unsigned char sco[4] = {3, 1, 0, 0}, shortcmd[4] = {1, 3, 0x0c, 2}, acl_big[6] = {2, 1, 0x20, 0xfe, 0x03, 0};
    check("SCO: EINVAL", write(fd, sco, 4) < 0 && errno == EINVAL);
    check("Befehl mit falscher Länge: EINVAL", write(fd, shortcmd, 4) < 0 && errno == EINVAL);
    check("ACL über 1021 Bytes: EINVAL", write(fd, acl_big, 6) < 0 && errno == EINVAL);
    check("Befehl", write(fd, c, 4) == 4);
    check("zu kleiner Puffer: EINVAL, Datensatz bleibt", rec(fd, b, 10, 3000) < 0 && errno == EINVAL && read(fd, b, sizeof b) == 258);
    rec(fd, b, sizeof b, 3000);
    int dup_fd = dup(fd);
    close(fd);
    check("dup hält den Knoten offen", open_err(HCI) == EBUSY);
    close(dup_fd);
    check("letzter Deskriptor zu: wieder frei", open_err(HCI) == 0);
    printf("BTTEST-HCI: %s\n", fails ? "FEHLER" : "ALLES OK");
}

int main(int argc, char **argv) {
    if (argc > 1 && strcmp(argv[1], "uinput") == 0) uinput_test();
    else if (argc > 1 && strcmp(argv[1], "hci") == 0) hci_test();
    else printf("bttest uinput | hci\n");
    return fails != 0;
}
