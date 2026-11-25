/* dritest: the GPU render node /dev/dri/renderD128 (kernel/ultar-kern/src/svcdev/dri.rs), used the way a
 * program uses the Linux xe driver: found in /dev/dri, opened, driver name and version, capabilities and
 * the device queries (two calls each: size, then data); wrong sizes, unknown requests and callers without
 * the right are refused.                                                         → "DRITEST: ALLES OK"
 * The node only exists after the GT driver published its device record (GTTEST on an Intel GPU).
 */
#define _GNU_SOURCE
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <grp.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/prctl.h>
#include <sys/stat.h>
#include <sys/sysmacros.h>
#include <sys/wait.h>
#include <unistd.h>

#define PR_ULTAR_RIGHTS_DROP 0x554c5201
#define R_RAW_DEVICE 0x20UL
#define NODE "/dev/dri/renderD128"

/* include/uapi/drm/drm.h and xe_drm.h, written out: the test must not depend on the build host's headers */
struct drm_version {
    int major, minor, patch;
    size_t name_len;
    char *name;
    size_t date_len;
    char *date;
    size_t desc_len;
    char *desc;
};
struct drm_get_cap {
    uint64_t capability, value;
};
struct xe_device_query {
    uint64_t extensions;
    uint32_t query, size;
    uint64_t data;
    uint64_t reserved[2];
};
#define IOCTL_VERSION _IOWR('d', 0x00, struct drm_version)
#define IOCTL_GET_CAP _IOWR('d', 0x0c, struct drm_get_cap)
#define IOCTL_QUERY _IOWR('d', 0x40, struct xe_device_query)
#define IOCTL_GEM_CREATE _IOWR('d', 0x41, char[56])
enum { Q_ENGINES, Q_MEM, Q_CONFIG, Q_GTS, Q_HWCONFIG, Q_UC_FW = 7 };

static int fails;

static void check(const char *name, int ok) {
    printf("%s %s\n", ok ? "ok " : "FEHLER", name);
    fflush(stdout);
    if (!ok) fails++;
}

/* a query as programs make it: size first, then the data; NULL on failure */
static void *query(int fd, uint32_t q, uint32_t *size) {
    struct xe_device_query dq = {.query = q};
    if (ioctl(fd, IOCTL_QUERY, &dq) != 0 || dq.size == 0) return NULL;
    void *buf = calloc(1, dq.size);
    dq.data = (uintptr_t)buf;
    if (ioctl(fd, IOCTL_QUERY, &dq) != 0) {
        free(buf);
        return NULL;
    }
    *size = dq.size;
    return buf;
}

static void version_and_caps(int fd) {
    char name[16] = "", desc[64] = "";
    struct drm_version v = {0};
    int ok = ioctl(fd, IOCTL_VERSION, &v) == 0;
    check("Version ohne Puffer nennt die Längen", ok && v.name_len == 2 && v.desc_len > 0 && v.desc_len < sizeof desc);
    v.name = name;
    v.desc = desc;
    ok = ioctl(fd, IOCTL_VERSION, &v) == 0;
    printf("    Treiber \"%s\" %d.%d.%d (%s)\n", name, v.major, v.minor, v.patch, desc);
    check("Treibername xe, Version 1.x", ok && strcmp(name, "xe") == 0 && v.major == 1);
    struct drm_get_cap c = {.capability = 0x6};
    check("Fähigkeit monotone Zeitstempel", ioctl(fd, IOCTL_GET_CAP, &c) == 0 && c.value == 1);
    c.capability = 0x13;
    c.value = 7;
    check("Synchronisationsobjekte werden (noch) nicht behauptet", ioctl(fd, IOCTL_GET_CAP, &c) == 0 && c.value == 0);
    c.capability = 0x999;
    check("unbekannte Fähigkeit: EINVAL", ioctl(fd, IOCTL_GET_CAP, &c) == -1 && errno == EINVAL);
}

static void queries(int fd) {
    uint32_t n = 0;
    uint64_t *cfg = query(fd, Q_CONFIG, &n);
    check("Konfiguration: 5 Werte", cfg && n == 48 && (uint32_t)cfg[0] == 5);
    if (cfg) printf("    Gerät %#06x Rev. %u, Adressraum %u Bit\n", (unsigned)(cfg[1] & 0xffff), (unsigned)(cfg[1] >> 16), (unsigned)cfg[4]);
    check("Intel-Grafik mit 48-Bit-Adressraum", cfg && (cfg[1] & 0xffff) != 0 && cfg[4] == 48);
    uint32_t *eng = query(fd, Q_ENGINES, &n);
    check("Engines: Anzahl passt zur Größe", eng && eng[0] >= 1 && n == 8 + eng[0] * 32);
    int render = 0, compute = 0;
    for (uint32_t i = 0; eng && i < eng[0]; i++) {
        uint16_t class = *(uint16_t *)((char *)eng + 8 + i * 32);
        render |= class == 0;
        compute |= class == 4;
    }
    check("Render- und Rechen-Engine dabei", render && compute);
    char *gts = query(fd, Q_GTS, &n);
    uint32_t clock = gts ? *(uint32_t *)(gts + 20) : 0;
    check("GT-Liste: ein GT mit Zeitstempel-Takt und IP-Version", gts && n == 104 && clock > 1000000 && *(uint16_t *)(gts + 40) >= 12);
    if (gts) printf("    IP %u.%02u, Takt %u Hz\n", *(uint16_t *)(gts + 40), *(uint16_t *)(gts + 42), clock);
    uint32_t *hw = query(fd, Q_HWCONFIG, &n);
    check("Hardware-Tabelle der GuC", hw && n >= 12 && n % 4 == 0);
    uint64_t *mem = query(fd, Q_MEM, &n);
    check("Speicher: eine Region Arbeitsspeicher", mem && n == 96 && (uint32_t)mem[0] == 1 && mem[2] > (64u << 20));
    uint32_t fw[8] = {0};
    struct xe_device_query dq = {.query = Q_UC_FW, .size = sizeof fw, .data = (uintptr_t)fw};
    check("GuC-Version der Auftragsschnittstelle", ioctl(fd, IOCTL_QUERY, &dq) == 0 && fw[2] >= 1);
    printf("    GuC-Auftragsschnittstelle %u.%u.%u\n", fw[2], fw[3], fw[4]);
    free(cfg), free(eng), free(gts), free(hw), free(mem);
}

static void refusals(int fd) {
    uint64_t buf[8];
    struct xe_device_query dq = {.query = Q_CONFIG, .size = 40, .data = (uintptr_t)buf};
    check("falsche Größe: EINVAL", ioctl(fd, IOCTL_QUERY, &dq) == -1 && errno == EINVAL);
    dq.size = 48;
    dq.data = 8;
    check("Puffer außerhalb des Programms: EFAULT", ioctl(fd, IOCTL_QUERY, &dq) == -1 && errno == EFAULT);
    dq = (struct xe_device_query){.query = 99};
    check("unbekannte Abfrage: EINVAL", ioctl(fd, IOCTL_QUERY, &dq) == -1 && errno == EINVAL);
    dq = (struct xe_device_query){.query = Q_CONFIG, .extensions = 1};
    check("Erweiterungen: EINVAL", ioctl(fd, IOCTL_QUERY, &dq) == -1 && errno == EINVAL);
    char gem[56] = {0};
    check("Puffer anlegen gibt es noch nicht: EINVAL", ioctl(fd, IOCTL_GEM_CREATE, gem) == -1 && errno == EINVAL);
    check("fremde Anfrage (TCGETS): ENOTTY", ioctl(fd, 0x5401, buf) == -1 && errno == ENOTTY);
}

static void without_right(void) {
    pid_t c = fork();
    if (c == 0) {
        prctl(PR_ULTAR_RIGHTS_DROP, R_RAW_DEVICE, 0, 0, 0);
        int fd = open(NODE, O_RDWR | O_CLOEXEC);
        _exit(fd < 0 && errno == EACCES ? 0 : 1);
    }
    int st = -1;
    waitpid(c, &st, 0);
    check("ohne das Recht RAW_DEVICE: EACCES", WIFEXITED(st) && WEXITSTATUS(st) == 0);
}

int main(void) {
    int found = 0;
    DIR *d = opendir("/dev/dri");
    for (struct dirent *e; d && (e = readdir(d));) found |= strcmp(e->d_name, "renderD128") == 0;
    if (d) closedir(d);
    if (!found) {
        printf("DRITEST: übersprungen (kein /dev/dri/renderD128 – kein Intel-GT in Betrieb)\n");
        return 0;
    }
    struct stat st;
    check("Gerätenummer 226:128 (Zeichengerät)", stat(NODE, &st) == 0 && S_ISCHR(st.st_mode) && major(st.st_rdev) == 226 && minor(st.st_rdev) == 128);
    int fd = open(NODE, O_RDWR | O_CLOEXEC);
    check("öffnen", fd >= 0);
    if (fd >= 0) {
        version_and_caps(fd);
        queries(fd);
        refusals(fd);
        int fd2 = dup(fd);
        struct drm_get_cap c = {.capability = 0x6};
        check("über ein Duplikat des Deskriptors", fd2 >= 0 && ioctl(fd2, IOCTL_GET_CAP, &c) == 0);
        close(fd2);
        close(fd);
    }
    without_right();
    printf(fails ? "DRITEST: FEHLER (%d)\n" : "DRITEST: ALLES OK\n", fails);
    return fails != 0;
}
