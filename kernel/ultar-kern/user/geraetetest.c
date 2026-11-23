/* geraetetest – the device interfaces of the Ultar kernel (for ultar-devd): device list, driver register,
 * bind/unbind/enable/disable of a built-in driver, driver ABI number, device events (uevents with SEQNUM on
 * an AF_NETLINK socket), probe/health results, rights. One line per check ("ok" / "FEHLER"); the last line
 * is "GERAETETEST: ALLES OK" when every check passed.
 *   geraetetest              list, register, rights; the bind cycle on an e1000 (8086:100e) if one exists
 *   geraetetest --unbind-e1000 / --state-e1000   helpers of tools/kern-test/devd-qemu.sh
 *   geraetetest --hotplug    wait for a USB device to come and go (the host adds and removes it through the
 *                            QEMU monitor after "GERAETETEST: USB BEREIT …"), marker "GERAETETEST-USB: ALLES OK"
 * Runs as a program started by the kernel: root, every right. */
#define _GNU_SOURCE
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <sched.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/prctl.h>
#include <sys/socket.h>
#include <sys/stat.h>
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

static int file_is(const char *path, const char *want) {
    char b[256];
    return strcmp(slurp(path, b, sizeof b), want) == 0;
}

static int file_has(const char *path, const char *part) {
    static char b[16384];
    return strstr(slurp(path, b, sizeof b), part) != NULL;
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
    static char b[16384];
    int n = 0;
    for (const char *p = slurp(path, b, sizeof b); *p; p++) n += *p == '\n';
    return n;
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
    check("Register: /sys/kernel/ultar/drivers nennt jeden Treiber", lines("/sys/kernel/ultar/drivers") >= count_dir(PCI "/drivers") && file_has("/sys/kernel/ultar/drivers", "virtio-blk pci hand 1 unsupported 0000:"));
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

static int hotplug(void) {
    int ev = event_socket();
    char msg[2048], dir[256], name[64] = "";
    say("GERAETETEST: USB BEREIT (add)\n");
    int len = -1;
    for (int i = 0; i < 30 && len < 0; i++) {
        len = next_event(ev, msg, sizeof msg, 1000);
        if (len > 0 && (strcmp(key(msg, len, "SUBSYSTEM"), "usb") || strcmp(key(msg, len, "ACTION"), "add"))) len = -1;
    }
    check("USB: Ereignis add mit SUBSYSTEM=usb, PRODUCT 627/1", len > 0 && strncmp(key(msg, len, "PRODUCT"), "627/1/", 6) == 0);
    if (len > 0) snprintf(name, sizeof name, "%s", strrchr(key(msg, len, "DEVPATH"), '/') + 1);
    snprintf(dir, sizeof dir, "/sys/bus/usb/devices/%s/idVendor", name);
    check("USB: Gerät in /sys/bus/usb/devices (idVendor 0627)", name[0] && file_is(dir, "0627\n"));
    snprintf(dir, sizeof dir, "/sys/bus/usb/devices/%s:1.0/driver", name);
    char l[128];
    ssize_t k = readlink(dir, l, sizeof l - 1);
    l[k > 0 ? k : 0] = 0;
    check("USB: Schnittstelle 1.0 von usbhid bedient", strcmp(l, "../../drivers/usbhid") == 0);
    check("USB: Tabelle /sys/kernel/ultar/devices nennt das Gerät", file_has("/sys/kernel/ultar/devices", "0627:0001"));
    say("GERAETETEST: USB BEREIT (remove)\n");
    len = -1;
    for (int i = 0; i < 30 && len < 0; i++) {
        len = next_event(ev, msg, sizeof msg, 1000);
        if (len > 0 && strcmp(key(msg, len, "ACTION"), "remove")) len = -1;
    }
    check("USB: Ereignis remove für dasselbe Gerät", len > 0 && strstr(key(msg, len, "DEVPATH"), name) != NULL);
    check("USB: Gerät aus der Liste verschwunden", name[0] && !in_dir("/sys/bus/usb/devices", name));
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
