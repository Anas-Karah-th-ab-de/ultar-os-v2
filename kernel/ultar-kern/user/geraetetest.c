/* geraetetest – the device interfaces of the Ultar kernel (for ultar-devd): device list, driver register,
 * bind/unbind/enable/disable of a built-in driver, driver ABI number, device events (uevents with SEQNUM on
 * an AF_NETLINK socket), probe/health results, rights. One line per check ("ok" / "FEHLER"); the last line
 * is "GERAETETEST: ALLES OK" when every check passed.
 *   geraetetest              list, register, rights; the bind cycle on an e1000 (8086:100e) if one exists
 *   geraetetest --unbind-e1000 / --state-e1000   helpers of tools/kern-test/devd-qemu.sh
 *   geraetetest --hotplug    wait for a USB device to come and go (the host adds and removes it through the
 *                            QEMU monitor after "GERAETETEST: USB BEREIT …" and types a key after "… (taste)");
 *                            the keyboard gets its own input node; marker "GERAETETEST-USB: ALLES OK"
 * Runs as a program started by the kernel: root, every right. */
#define _GNU_SOURCE
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <linux/input.h>
#include <poll.h>
#include <sched.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/prctl.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/sysmacros.h>
#include <sys/wait.h>
#include <unistd.h>

#define PR_ULTAR_RIGHTS_DROP 0x554c5201
#define R_LOAD_DRIVER 0x4UL
#define AF_NETLINK_ 16
#define NETLINK_KOBJECT_UEVENT_ 15
#define PCI "/sys/bus/pci"

static int fails;
static long last_seq = -1;
static int seq_gaps;

static void say(const char *fmt, ...) {
    char b[1024];
    va_list ap;
    va_start(ap, fmt);
    int n = vsnprintf(b, sizeof b, fmt, ap);
    va_end(ap);
    if (n > (int)sizeof b - 1) n = sizeof b - 1;
    if (n > 0 && write(1, b, n) < 0) fails++;
}

static void check(const char *name, int ok) {
    if (!ok) fails++;
    say("%s %s\n", ok ? "ok    " : "FEHLER", name);
}

/* the call must have failed with exactly this error */
static void denied(const char *name, long r, int want) {
    int e = errno, ok = r < 0 && e == want;
    if (!ok) fails++;
    say("%s %s -> %s\n", ok ? "ok    " : "FEHLER", name, r < 0 ? strerror(e) : "angenommen");
}

/* whole small file into buf ("" if unreadable) */
static const char *slurp(const char *path, char *buf, size_t n) {
    buf[0] = 0;
    int fd = open(path, O_RDONLY);
    if (fd < 0) return buf;
    ssize_t k = read(fd, buf, n - 1);
    close(fd);
    buf[k > 0 ? k : 0] = 0;
    return buf;
}

/* the whole file, read in pieces of `piece` bytes until end of file (no size assumed); malloc'd, "" if
 * unreadable – a generated file of the device tree must give the same bytes for every piece size */
static char *whole(const char *path, size_t piece) {
    size_t len = 0, cap = 4096;
    char *b = malloc(cap + 1);
    int fd = open(path, O_RDONLY);
    if (!b) return NULL;
    if (fd >= 0) {
        for (;;) {
            if (len + piece > cap) {
                cap = (len + piece) * 2;
                char *n = realloc(b, cap + 1);
                if (!n) break;
                b = n;
            }
            ssize_t k = read(fd, b + len, piece);
            if (k <= 0) break;
            len += k;
        }
        close(fd);
    }
    b[len] = 0;
    return b;
}

static int file_is(const char *path, const char *want) {
    char b[256];
    return strcmp(slurp(path, b, sizeof b), want) == 0;
}

static int file_has(const char *path, const char *part) {
    char *b = whole(path, 65536);
    int ok = b && strstr(b, part) != NULL;
    free(b);
    return ok;
}

/* write one control value; returns 0 or -1 with errno */
static int put(const char *path, const char *text) {
    int fd = open(path, O_WRONLY);
    if (fd < 0) return -1;
    ssize_t k = write(fd, text, strlen(text));
    int e = errno;
    close(fd);
    errno = e;
    return k == (ssize_t)strlen(text) ? 0 : -1;
}

static int in_dir(const char *dir, const char *name) {
    DIR *d = opendir(dir);
    struct dirent *e;
    int found = 0;
    if (!d) return 0;
    while ((e = readdir(d))) found |= strcmp(e->d_name, name) == 0;
    closedir(d);
    return found;
}

static int count_dir(const char *dir) {
    DIR *d = opendir(dir);
    struct dirent *e;
    int n = 0;
    if (!d) return -1;
    while ((e = readdir(d))) n += e->d_name[0] != '.';
    closedir(d);
    return n;
}

/* USB devices without their interface entries (`1-2`, not `1-2:1.0`) */
static int count_usb_devices(void) {
    DIR *d = opendir("/sys/bus/usb/devices");
    struct dirent *e;
    int n = 0;
    if (!d) return -1;
    while ((e = readdir(d))) n += e->d_name[0] != '.' && !strchr(e->d_name, ':');
    closedir(d);
    return n;
}

static int lines(const char *path) {
    char *b = whole(path, 65536);
    int n = 0;
    for (const char *p = b ? b : ""; *p; p++) n += *p == '\n';
    free(b);
    return n;
}

/* every generated table gives the same content in 7-byte reads as in one large read (offsets work, no
 * truncation at a page or a line count) */
static int pieces_agree(const char *path) {
    char *a = whole(path, 65536), *b = whole(path, 7);
    int ok = a && b && a[0] && strcmp(a, b) == 0;
    free(a);
    free(b);
    return ok;
}

/* the line of driver `name` in the register table (NULL: none) */
static const char *table_line(const char *table, const char *name) {
    size_t n = strlen(name);
    for (const char *l = table; l && *l; l = strchr(l, '\n') ? strchr(l, '\n') + 1 : NULL)
        if (strncmp(l, name, n) == 0 && strncmp(l + n, " pci ", 5) == 0) return l;
    return NULL;
}

/* every driver directory has its line in /sys/kernel/ultar/drivers, and the line names exactly the
 * devices linked in the directory, or "-" for none (whatever machine this is) */
static int register_complete(void) {
    char *table = whole("/sys/kernel/ultar/drivers", 65536);
    DIR *d = opendir(PCI "/drivers");
    struct dirent *e;
    int ok = table && d;
    while (ok && (e = readdir(d))) {
        if (e->d_name[0] == '.') continue;
        const char *line = table_line(table, e->d_name);
        const char *end = line ? strchr(line, '\n') : NULL;
        if (!line || !end) { ok = 0; break; }
        char dir[160], last[2048];
        snprintf(dir, sizeof dir, PCI "/drivers/%.60s", e->d_name);
        snprintf(last, sizeof last, "%.*s", (int)(end - line), line);
        char *devs = strrchr(last, ' ');
        int linked = 0;
        DIR *dd = opendir(dir);
        struct dirent *x;
        while (dd && (x = readdir(dd)))
            if (strncmp(x->d_name, "0000:", 5) == 0) {
                linked++;
                if (!devs || !strstr(devs, x->d_name)) ok = 0;
            }
        if (dd) closedir(dd);
        int named = 0;
        for (const char *c = devs ? devs + 1 : ""; *c; c++) named += *c == ',';
        named = devs && strcmp(devs, " -") ? named + 1 : 0;
        if (named != linked) ok = 0;
    }
    if (d) closedir(d);
    free(table);
    return ok;
}

/* the value of KEY in a uevent message (keys end with NUL) */
static const char *key(const char *msg, int len, const char *k) {
    size_t kl = strlen(k);
    for (int i = 0; i < len; i += strlen(msg + i) + 1)
        if (strncmp(msg + i, k, kl) == 0 && msg[i + kl] == '=') return msg + i + kl + 1;
    return "";
}

/* next event (waiting up to ms); its sequence number must follow the last one */
static int next_event(int fd, char *msg, int n, int ms) {
    struct pollfd p = {fd, POLLIN, 0};
    if (poll(&p, 1, ms) != 1) return -1;
    int len = recv(fd, msg, n - 1, 0);
    if (len <= 0) return -1;
    msg[len] = 0;
    long s = atol(key(msg, len, "SEQNUM"));
    if (last_seq >= 0 && s != last_seq + 1) seq_gaps++;
    last_seq = s;
    return len;
}

/* the next event must be this action on this path */
static void expect_event(int fd, const char *action, const char *path, const char *driver) {
    char msg[2048], name[256];
    int len = next_event(fd, msg, sizeof msg, 3000);
    snprintf(name, sizeof name, "Ereignis %s@%s%s%s", action, path, driver ? " DRIVER=" : "", driver ? driver : "");
    check(name, len > 0 && strcmp(key(msg, len, "ACTION"), action) == 0 && strcmp(key(msg, len, "DEVPATH"), path) == 0 &&
                    (!driver || strcmp(key(msg, len, "DRIVER"), driver) == 0) && strncmp(msg, action, strlen(action)) == 0);
}

static int event_socket(void) {
    int fd = socket(AF_NETLINK_, SOCK_RAW | SOCK_CLOEXEC, NETLINK_KOBJECT_UEVENT_);
    struct {
        unsigned short family, pad;
        unsigned pid, groups;
    } a = {AF_NETLINK_, 0, 0, 1};
    check("Ereignis-Socket: socket(AF_NETLINK, NETLINK_KOBJECT_UEVENT) und bind", fd >= 0 && bind(fd, (void *)&a, sizeof a) == 0);
    return fd;
}

static void abi_register_list(void) {
    char b[256];
    check("ABI: /sys/kernel/ultar/driver_abi ist 1", file_is("/sys/kernel/ultar/driver_abi", "1\n"));
    int npci = count_dir(PCI "/devices");
    check("Liste: PCI-Funktionen in /sys/bus/pci/devices", npci > 2);
    check("Liste: Tabelle /sys/kernel/ultar/devices hat je Gerät eine Zeile",
          lines("/sys/kernel/ultar/devices") == npci + count_usb_devices() + count_dir("/sys/bus/platform/devices"));
    check("Liste: Plattformgerät serial8250 mit Treiber", in_dir("/sys/bus/platform/devices", "serial8250") && file_has("/sys/bus/platform/devices/serial8250/uevent", "DRIVER=serial8250"));
    check("Register: virtio-blk, virtio-net, nvme, gen-intel-eth, gen-xhci", in_dir(PCI "/drivers", "virtio-blk") && in_dir(PCI "/drivers", "virtio-net") && in_dir(PCI "/drivers", "nvme") && in_dir(PCI "/drivers", "gen-intel-eth") && in_dir(PCI "/drivers", "gen-xhci"));
    check("Register: Herkunft werkstatt / hand", file_is(PCI "/drivers/gen-intel-eth/ultar_origin", "werkstatt\n") && file_is(PCI "/drivers/virtio-blk/ultar_origin", "hand\n"));
    check("Register: Abbau unterstützt (gen-intel-eth) / nicht (virtio-blk) – sagt warum", file_is(PCI "/drivers/gen-intel-eth/ultar_unbind", "supported\n") && file_has(PCI "/drivers/virtio-blk/ultar_unbind", "unsupported: "));
    check("Register: /sys/kernel/ultar/drivers nennt jeden Treiber mit genau seinen Geräten", lines("/sys/kernel/ultar/drivers") >= count_dir(PCI "/drivers") && register_complete());
    int agree = 1;
    const char *tables[] = {"/sys/kernel/ultar/devices", "/sys/kernel/ultar/drivers", "/sys/kernel/ultar/uevents", "/sys/kernel/ultar/driver_abi", "/sys/kernel/uevent_seqnum"};
    for (unsigned i = 0; i < sizeof tables / sizeof *tables; i++) agree &= pieces_agree(tables[i]);
    check("Tabellen: in 7-Byte-Stücken gelesen gleich wie am Stück (Versatz, nichts abgeschnitten)", agree);
    char *dt = whole("/sys/kernel/ultar/devices", 65536), *rt = whole("/sys/kernel/ultar/drivers", 65536);
    say("--     Tabellen: devices %zu Bytes (%d Zeilen), drivers %zu Bytes\n", dt ? strlen(dt) : 0, lines("/sys/kernel/ultar/devices"), rt ? strlen(rt) : 0);
    free(dt);
    free(rt);
    check("Register: Bindedateien bind/unbind (wie Linux)", in_dir(PCI "/drivers/gen-intel-eth", "bind") && in_dir(PCI "/drivers/gen-intel-eth", "unbind"));
    check("Ereignisse: /sys/kernel/uevent_seqnum lesbar", slurp("/sys/kernel/uevent_seqnum", b, sizeof b)[0] >= '0');
}

/* the PCI function the boot disk (virtio-blk) sits on */
static int find_by_driver(const char *drv, char *out, size_t n) {
    char dir[128];
    snprintf(dir, sizeof dir, PCI "/drivers/%s", drv);
    DIR *d = opendir(dir);
    struct dirent *e;
    out[0] = 0;
    if (!d) return 0;
    while ((e = readdir(d)))
        if (strncmp(e->d_name, "0000:", 5) == 0) snprintf(out, n, "%.60s", e->d_name);
    closedir(d);
    return out[0] != 0;
}

static int find_e1000(char *out, size_t n) {
    DIR *d = opendir(PCI "/devices");
    struct dirent *e;
    char p[512];
    out[0] = 0;
    if (!d) return 0;
    while ((e = readdir(d))) {
        if (e->d_name[0] == '.') continue;
        snprintf(p, sizeof p, PCI "/devices/%.60s/device", e->d_name);
        if (!file_is(p, "0x100e\n")) continue;
        snprintf(p, sizeof p, PCI "/devices/%.60s/vendor", e->d_name);
        if (file_is(p, "0x8086\n")) snprintf(out, n, "%.60s", e->d_name);
    }
    closedir(d);
    return out[0] != 0;
}

static void refusals(void) {
    char disk[64];
    denied("Rechte: Gerätedatei vendor zum Schreiben öffnen", open(PCI "/devices/0000:00:00.0/vendor", O_WRONLY), EACCES);
    if (find_by_driver("virtio-blk", disk, sizeof disk))
        denied("Abbau: Startplatte (virtio-blk, kein Abbaupfad) lösen", put(PCI "/drivers/virtio-blk/unbind", disk), EOPNOTSUPP);
    if (find_by_driver("bruecke", disk, sizeof disk))
        denied("Abbau: Brücke ohne Abbaupfad lösen", put(PCI "/drivers/bruecke/unbind", disk), EOPNOTSUPP);
    denied("Binden: unbekannter Treiber gibt es nicht", open(PCI "/drivers/gibt-es-nicht/bind", O_WRONLY), ENOENT);
    pid_t c = fork();
    if (c == 0) {
        fails = 0;
        prctl(PR_ULTAR_RIGHTS_DROP, R_LOAD_DRIVER, 0, 0, 0);
        denied("Rechte: ohne LOAD_DRIVER bind öffnen", open(PCI "/drivers/gen-intel-eth/bind", O_WRONLY), EACCES);
        denied("Rechte: ohne LOAD_DRIVER drivers_probe öffnen", open(PCI "/drivers_probe", O_WRONLY), EACCES);
        check("Rechte: ohne LOAD_DRIVER lesen geht", file_has("/sys/kernel/ultar/drivers", "gen-intel-eth"));
        _exit(fails ? 1 : 0);
    }
    int st = -1;
    waitpid(c, &st, 0);
    if (!WIFEXITED(st) || WEXITSTATUS(st)) fails++;
    c = fork();
    if (c == 0) {
        if (unshare(CLONE_NEWPID) != 0) _exit(2);
        pid_t g = fork();
        if (g == 0) _exit(socket(AF_NETLINK_, SOCK_RAW, NETLINK_KOBJECT_UEVENT_) < 0 && errno == EACCES ? 0 : 1);
        waitpid(g, &st, 0);
        _exit(WIFEXITED(st) ? WEXITSTATUS(st) : 3);
    }
    waitpid(c, &st, 0);
    check("Zelle: Ereignis-Socket in eigenem pid-Namensraum abgewiesen", WIFEXITED(st) && WEXITSTATUS(st) == 0);
    denied("Ereignis-Socket: anderes Protokoll", socket(AF_NETLINK_, SOCK_RAW, 0), EPROTONOSUPPORT);
}

static void dev_path(char *out, size_t n, const char *dev, const char *f) {
    snprintf(out, n, PCI "/devices/%s/%s", dev, f);
}

static void cycle(int ev, const char *e) {
    char p[256], l[128], dp[128], b[256];
    snprintf(dp, sizeof dp, "/bus/pci/devices/%s", e);
    dev_path(p, sizeof p, e, "driver");
    ssize_t k = readlink(p, l, sizeof l - 1);
    l[k > 0 ? k : 0] = 0;
    check("e1000: gebunden, driver-Verknüpfung auf gen-intel-eth", strcmp(l, "../../drivers/gen-intel-eth") == 0);
    dev_path(p, sizeof p, e, "uevent");
    check("e1000: uevent DRIVER, PCI_ID, PCI_CLASS, MODALIAS", file_has(p, "DRIVER=gen-intel-eth\n") && file_has(p, "PCI_ID=8086:100E\n") && file_has(p, "PCI_CLASS=20000\n") && file_has(p, "MODALIAS=pci:v00008086d0000100E"));
    dev_path(p, sizeof p, e, "class");
    check("e1000: Klasse 0x020000", file_is(p, "0x020000\n"));
    dev_path(p, sizeof p, e, "ultar_health");
    check("e1000: Gesundheit probe=ok link=up bus_master=on", file_has(p, "probe=ok\n") && file_has(p, "link=up\n") && file_has(p, "bus_master=on\n"));
    dev_path(p, sizeof p, e, "ultar_drivers");
    check("e1000: passende Treiber gen-intel-eth", file_is(p, "gen-intel-eth\n"));
    check("Abbau: gen-intel-eth unbind", put(PCI "/drivers/gen-intel-eth/unbind", e) == 0);
    expect_event(ev, "unbind", dp, NULL);
    dev_path(p, sizeof p, e, "ultar_state");
    check("Abbau: Zustand unbound, Verknüpfung weg", file_is(p, "unbound\n") && !in_dir(PCI "/drivers/gen-intel-eth", e));
    dev_path(p, sizeof p, e, "ultar_health");
    check("Abbau: Gesundheit probe=none bus_master=off (kein DMA mehr)", file_has(p, "probe=none\n") && file_has(p, "bus_master=off\n"));
    denied("Abbau: zweites unbind", put(PCI "/drivers/gen-intel-eth/unbind", e), ENODEV);
    denied("Binden: virtio-net passt nicht zum e1000", put(PCI "/drivers/virtio-net/bind", e), ENODEV);
    check("Treiber aus: ultar_enabled 0", put(PCI "/drivers/gen-intel-eth/ultar_enabled", "0") == 0 && file_is(PCI "/drivers/gen-intel-eth/ultar_enabled", "0\n"));
    denied("Treiber aus: bind abgewiesen", put(PCI "/drivers/gen-intel-eth/bind", e), EPERM);
    denied("Treiber aus: drivers_probe findet keinen", put(PCI "/drivers_probe", e), ENODEV);
    check("Treiber an: ultar_enabled 1", put(PCI "/drivers/gen-intel-eth/ultar_enabled", "1") == 0);
    dev_path(p, sizeof p, e, "enable");
    dev_path(b, sizeof b, e, "ultar_state");
    check("Gerät aus: enable 0, Zustand disabled", put(p, "0") == 0 && file_is(p, "0\n") && file_is(b, "disabled\n"));
    expect_event(ev, "change", dp, NULL);
    denied("Gerät aus: bind abgewiesen", put(PCI "/drivers/gen-intel-eth/bind", e), EPERM);
    check("Gerät an: enable 1", put(p, "1") == 0);
    expect_event(ev, "change", dp, NULL);
    check("Binden: gen-intel-eth bind", put(PCI "/drivers/gen-intel-eth/bind", e) == 0);
    expect_event(ev, "bind", dp, "gen-intel-eth");
    dev_path(p, sizeof p, e, "ultar_health");
    check("Binden: wieder gebunden, link=up, bus_master=on", in_dir(PCI "/drivers/gen-intel-eth", e) && file_has(p, "probe=ok\n") && file_has(p, "link=up\n") && file_has(p, "bus_master=on\n"));
    denied("Binden: schon gebunden", put(PCI "/drivers/gen-intel-eth/bind", e), EBUSY);
    check("Zweite Runde: unbind, drivers_probe bindet den ersten passenden Treiber",
          put(PCI "/drivers/gen-intel-eth/unbind", e) == 0 && put(PCI "/drivers_probe", e) == 0 && in_dir(PCI "/drivers/gen-intel-eth", e));
    expect_event(ev, "unbind", dp, NULL);
    expect_event(ev, "bind", dp, "gen-intel-eth");
    snprintf(b, sizeof b, "%ld\n", last_seq);
    check("Ereignisse: SEQNUM ohne Lücke, uevent_seqnum = letzte Nummer", seq_gaps == 0 && file_is("/sys/kernel/uevent_seqnum", b));
    check("Ereignisse: Verlauf /sys/kernel/ultar/uevents nennt bind", file_has("/sys/kernel/ultar/uevents", " bind /bus/pci/devices/"));
}

/* the keyboard's own input node: numbers, ids, capabilities; a key from the host arrives as press and
 * release (the host sends it after "USB BEREIT (taste)"). Returns the open node (-1: none). */
static int usb_keyboard(const char *node) {
    char path[64], b[128];
    int n = -1;
    sscanf(node, "input/event%d", &n);
    snprintf(path, sizeof path, "/dev/%s", node);
    struct stat st;
    check("USB-Tastatur: eigener Eingabeknoten ab event2 mit Ereignis add SUBSYSTEM=input", n >= 2);
    check("USB-Tastatur: Knoten ist ein Zeichengerät 13:64+n", n >= 2 && stat(path, &st) == 0 && S_ISCHR(st.st_mode) && major(st.st_rdev) == 13 && (int)minor(st.st_rdev) == 64 + n);
    int fd = n >= 2 ? open(path, O_RDONLY | O_NONBLOCK | O_CLOEXEC) : -1;
    struct input_id id = {0};
    unsigned char keys[KEY_MAX / 8 + 1] = {0};
    check("USB-Tastatur: EVIOCGID Bus USB, 0627:0001; EVIOCGBIT hat KEY_A", fd >= 0 && ioctl(fd, EVIOCGID, &id) == 0 && id.bustype == BUS_USB && id.vendor == 0x0627 && id.product == 1 && ioctl(fd, EVIOCGBIT(EV_KEY, sizeof keys), keys) > 0 && (keys[KEY_A / 8] >> (KEY_A % 8) & 1));
    snprintf(path, sizeof path, "/sys/class/input/event%d/device/name", n);
    check("USB-Tastatur: /sys/class/input/eventN/device/name", n >= 2 && strstr(slurp(path, b, sizeof b), "USB") != NULL);
    say("GERAETETEST: USB BEREIT (taste)\n");
    int down = 0, up = 0;
    struct input_event e[16];
    for (int i = 0; i < 100 && fd >= 0 && !up; i++) {
        struct pollfd pf = {fd, POLLIN, 0};
        poll(&pf, 1, 100);
        long r = read(fd, e, sizeof e);
        for (int j = 0; j < r / (long)sizeof e[0]; j++) {
            if (e[j].type == EV_KEY && e[j].code == KEY_A && e[j].value == 1) down = 1;
            if (e[j].type == EV_KEY && e[j].code == KEY_A && e[j].value == 0 && down) up = 1;
        }
    }
    check("USB-Tastatur: Taste a kommt als Drücken und getrenntes Loslassen", down && up);
    return fd;
}

/* input nodes named in `action` events of SUBSYSTEM=input, up to `want` of them (wait up to 30 s) */
static int input_events(int ev, const char *action, char nodes[][32], int want) {
    char msg[2048];
    int got = 0;
    for (int i = 0; i < 30 && got < want; i++) {
        int len = next_event(ev, msg, sizeof msg, 1000);
        if (len > 0 && strcmp(key(msg, len, "SUBSYSTEM"), "input") == 0 && strcmp(key(msg, len, "ACTION"), action) == 0)
            snprintf(nodes[got++], 32, "%s", key(msg, len, "DEVNAME"));
    }
    return got;
}

static int has(const unsigned char *b, int bit) {
    return b[bit / 8] >> (bit % 8) & 1;
}

/* a USB mouse and a USB tablet come (the host adds them after "USB BEREIT (zeiger)"), each gets its own
 * node with the right capabilities; after "USB BEREIT (klick)" the host moves and clicks on each
 * (QEMU `mouse_set`), the events arrive on the right node; then both go again */
static void usb_pointers(int ev) {
    char nodes[2][32] = {"", ""}, path[96];
    say("GERAETETEST: USB BEREIT (zeiger)\n");
    int n = input_events(ev, "add", nodes, 2);
    int mouse = -1, tablet = -1;
    for (int i = 0; i < n; i++) {
        snprintf(path, sizeof path, "/dev/%s", nodes[i]);
        int fd = open(path, O_RDONLY | O_NONBLOCK | O_CLOEXEC);
        unsigned char evb[4] = {0};
        if (fd >= 0 && ioctl(fd, EVIOCGBIT(0, sizeof evb), evb) > 0 && has(evb, EV_ABS)) tablet = fd;
        else if (fd >= 0 && has(evb, EV_REL)) mouse = fd;
        else if (fd >= 0) close(fd);
    }
    unsigned char rel[2] = {0};
    struct input_absinfo ax = {0}, ay = {0};
    check("USB-Maus: eigener Knoten mit EV_REL (REL_X, REL_Y, Rad)", mouse >= 0 && ioctl(mouse, EVIOCGBIT(EV_REL, sizeof rel), rel) > 0 && has(rel, REL_X) && has(rel, REL_Y) && has(rel, REL_WHEEL));
    check("USB-Tablett: eigener Knoten mit EV_ABS, EVIOCGABS 0..32767", tablet >= 0 && ioctl(tablet, EVIOCGABS(ABS_X), &ax) == 0 && ioctl(tablet, EVIOCGABS(ABS_Y), &ay) == 0 && ax.maximum == 32767 && ay.maximum == 32767 && ax.minimum == 0);
    say("GERAETETEST: USB BEREIT (klick)\n");
    int moved = 0, m_down = 0, m_up = 0, abs_seen = 0, t_down = 0, t_up = 0;
    struct input_event e[32];
    for (int i = 0; i < 100 && !(moved && m_up && abs_seen && t_up); i++) {
        struct pollfd pf[2] = {{mouse, POLLIN, 0}, {tablet, POLLIN, 0}};
        poll(pf, 2, 100);
        long r = mouse >= 0 ? read(mouse, e, sizeof e) : -1;
        for (int j = 0; j < r / (long)sizeof e[0]; j++) {
            moved |= e[j].type == EV_REL && (e[j].code == REL_X || e[j].code == REL_Y);
            if (e[j].type == EV_KEY && e[j].code == BTN_LEFT) e[j].value ? (m_down = 1) : (m_up = m_down);
        }
        r = tablet >= 0 ? read(tablet, e, sizeof e) : -1;
        for (int j = 0; j < r / (long)sizeof e[0]; j++) {
            abs_seen |= e[j].type == EV_ABS && e[j].code == ABS_X;
            if (e[j].type == EV_KEY && e[j].code == BTN_LEFT) e[j].value ? (t_down = 1) : (t_up = t_down);
        }
    }
    check("USB-Maus: Bewegung (EV_REL) und Klick links drücken/loslassen", moved && m_down && m_up);
    check("USB-Tablett: Position (EV_ABS) und Klick links drücken/loslassen", abs_seen && t_down && t_up);
    say("GERAETETEST: USB BEREIT (zeiger weg)\n");
    char gone[2][32] = {"", ""};
    int g = input_events(ev, "remove", gone, 2);
    check("USB-Zeiger: Ereignis remove für beide Knoten, Knoten weg", g == 2 && n == 2 && !in_dir("/dev/input", strchr(nodes[0], '/') + 1) && !in_dir("/dev/input", strchr(nodes[1], '/') + 1));
    if (mouse >= 0) close(mouse);
    if (tablet >= 0) close(tablet);
}

static int hotplug(void) {
    int ev = event_socket();
    char msg[2048], dir[256], name[64] = "", node[32] = "";
    int product = 0;
    say("GERAETETEST: USB BEREIT (add)\n");
    for (int i = 0; i < 30 && !(name[0] && node[0]); i++) {
        int len = next_event(ev, msg, sizeof msg, 1000);
        if (len <= 0 || strcmp(key(msg, len, "ACTION"), "add")) continue;
        if (strcmp(key(msg, len, "SUBSYSTEM"), "usb") == 0 && !name[0]) {
            product = strncmp(key(msg, len, "PRODUCT"), "627/1/", 6) == 0;
            snprintf(name, sizeof name, "%s", strrchr(key(msg, len, "DEVPATH"), '/') + 1);
        }
        if (strcmp(key(msg, len, "SUBSYSTEM"), "input") == 0) snprintf(node, sizeof node, "%s", key(msg, len, "DEVNAME"));
    }
    check("USB: Ereignis add mit SUBSYSTEM=usb, PRODUCT 627/1", product);
    snprintf(dir, sizeof dir, "/sys/bus/usb/devices/%s/idVendor", name);
    check("USB: Gerät in /sys/bus/usb/devices (idVendor 0627)", name[0] && file_is(dir, "0627\n"));
    snprintf(dir, sizeof dir, "/sys/bus/usb/devices/%s:1.0/driver", name);
    char l[128];
    ssize_t k = readlink(dir, l, sizeof l - 1);
    l[k > 0 ? k : 0] = 0;
    check("USB: Schnittstelle 1.0 von usbhid bedient", strcmp(l, "../../drivers/usbhid") == 0);
    check("USB: Tabelle /sys/kernel/ultar/devices nennt das Gerät", file_has("/sys/kernel/ultar/devices", "0627:0001"));
    int kb = usb_keyboard(node);
    say("GERAETETEST: USB BEREIT (remove)\n");
    int usb_gone = 0, node_gone = 0;
    for (int i = 0; i < 30 && !(usb_gone && node_gone); i++) {
        int len = next_event(ev, msg, sizeof msg, 1000);
        if (len <= 0 || strcmp(key(msg, len, "ACTION"), "remove")) continue;
        if (strcmp(key(msg, len, "SUBSYSTEM"), "usb") == 0 && name[0] && strstr(key(msg, len, "DEVPATH"), name)) usb_gone = 1;
        if (strcmp(key(msg, len, "SUBSYSTEM"), "input") == 0 && node[0] && strcmp(key(msg, len, "DEVNAME"), node) == 0) node_gone = 1;
    }
    check("USB: Ereignis remove für dasselbe Gerät", usb_gone);
    check("USB: Gerät aus der Liste verschwunden", name[0] && !in_dir("/sys/bus/usb/devices", name));
    check("USB-Tastatur: Ereignis remove für den Eingabeknoten, Knoten weg", node_gone && node[0] && !in_dir("/dev/input", strchr(node, '/') + 1));
    char b[24];
    check("USB-Tastatur: offener Deskriptor liest danach ENODEV", kb >= 0 && read(kb, b, sizeof b) == -1 && errno == ENODEV);
    if (kb >= 0) close(kb);
    usb_pointers(ev);
    check("USB: SEQNUM ohne Lücke", seq_gaps == 0);
    say("GERAETETEST-USB: %s\n", fails == 0 ? "ALLES OK" : "SIEHE OBEN");
    return fails != 0;
}

/* helpers of the ultar-devd test in QEMU (devd-qemu.sh): unbind the e1000 / print its state */
static int e1000_helper(const char *what) {
    char e[64], p[256], st[64];
    if (!find_e1000(e, sizeof e)) return say("GERAETETEST: kein e1000\n"), 1;
    if (strcmp(what, "--unbind-e1000") == 0) {
        int r = put(PCI "/drivers/gen-intel-eth/unbind", e);
        say("GERAETETEST: e1000 %s %s\n", e, r == 0 ? "gelöst" : strerror(errno));
        return r != 0;
    }
    dev_path(p, sizeof p, e, "ultar_state");
    slurp(p, st, sizeof st);
    say("GERAETETEST: e1000 %s %s", e, st);
    return 0;
}

int main(int argc, char **argv) {
    if (argc > 1 && strcmp(argv[1], "--hotplug") == 0) return hotplug();
    if (argc > 1 && strncmp(argv[1], "--", 2) == 0 && strstr(argv[1], "e1000")) return e1000_helper(argv[1]);
    say("GERAETETEST: Geräteliste, Treiberregister, Binden, ABI, Ereignisse, Gesundheit, Rechte\n");
    abi_register_list();
    int ev = event_socket();
    char e[64];
    struct pollfd pf = {ev, POLLIN, 0};
    check("Ereignis-Socket: noch nichts da (poll)", poll(&pf, 1, 0) == 0);
    refusals();
    if (find_e1000(e, sizeof e)) cycle(ev, e);
    else say("--     Bindezyklus übersprungen: kein e1000 (8086:100e) in dieser Maschine\n");
    say("GERAETETEST: %d Fehler\n", fails);
    say("GERAETETEST: %s\n", fails == 0 ? "ALLES OK" : "SIEHE OBEN");
    return fails != 0;
}
