/* propsvc – minimaler Android-Property-Dienst für die abgeschottete Android-Umgebung auf Ultar.
 *  - legt einen echten Property-Bereich an (bionic "pre-split": /dev/__properties__ als eine Datei),
 *    damit Lesen, Warten (__system_property_wait) und Benachrichtigen funktionieren;
 *  - lädt Startwerte aus build.prop-Dateien und androidboot.* der Kernel-Befehlszeile (-> ro.boot.*);
 *  - nimmt Setz-Anfragen (PROP_MSG_SETPROP / SETPROP2) über den Socket an und protokolliert sie.
 * propsvc <socket> <protokoll> <bereichsdatei> [prop-dateien...] */
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <linux/futex.h>
#include <stdatomic.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/syscall.h>
#include <sys/un.h>
#include <unistd.h>

#define PA_SIZE (128 * 1024)
#define PROP_VALUE_MAX 92
#define AL(x) (((x) + 3) & ~3u)
#define LONG_FLAG (1u << 16)
static const char LONG_ERR[] = "Must use __system_property_read_callback() to read";

struct prop_area { uint32_t bytes_used; _Atomic uint32_t serial; uint32_t magic, version, reserved[28]; char data[]; };
struct prop_bt { uint32_t namelen; _Atomic uint32_t prop, left, right, children; char name[]; };
struct prop_info { _Atomic uint32_t serial; union { char value[PROP_VALUE_MAX]; struct { char err[56]; uint32_t offset; } lp; }; char name[]; };

static struct prop_area *pa;
static size_t pa_data;
static FILE *out;

static void wake(_Atomic uint32_t *a) { syscall(SYS_futex, a, FUTEX_WAKE, INT_MAX, 0, 0, 0); }
static void *at(uint32_t off) { return pa->data + off; }
static void *alloc(size_t sz, uint32_t *off) {
    size_t a = AL(sz);
    if (pa->bytes_used + a > pa_data) return 0;
    *off = pa->bytes_used; pa->bytes_used += a;
    return pa->data + *off;
}
static int cmp(const char *a, uint32_t al, const char *b, uint32_t bl) {
    if (al != bl) return al < bl ? -1 : 1;
    return memcmp(a, b, al);
}
static struct prop_bt *new_bt(const char *n, uint32_t nl, uint32_t *off) {
    struct prop_bt *b = alloc(sizeof *b + nl + 1, off);
    if (b) { b->namelen = nl; memcpy(b->name, n, nl); b->name[nl] = 0; }
    return b;
}
/* Kindknoten 'n' im Teilbaum unter *slot suchen bzw. anlegen */
static struct prop_bt *find_child(_Atomic uint32_t *slot, const char *n, uint32_t nl) {
    for (;;) {
        uint32_t o = atomic_load(slot);
        if (!o) {
            uint32_t no; struct prop_bt *b = new_bt(n, nl, &no);
            if (!b) return 0;
            atomic_store(slot, no);
            return b;
        }
        struct prop_bt *c = at(o);
        int r = cmp(n, nl, c->name, c->namelen);
        if (!r) return c;
        slot = r < 0 ? &c->left : &c->right;
    }
}
static void set(const char *name, const char *val) {
    size_t vl = strlen(val), nl = strlen(name);
    if (!nl || nl > 400) return;
    struct prop_bt *node = 0;
    /* Knoten suchen/anlegen */
    {
        struct prop_bt *cur = at(0); const char *p = name;
        for (;;) {
            const char *dot = strchr(p, '.');
            uint32_t l = dot ? (uint32_t)(dot - p) : (uint32_t)strlen(p);
            if (!l) return;
            cur = find_child(&cur->children, p, l);
            if (!cur) return;
            if (!dot) break;
            p = dot + 1;
        }
        node = cur;
    }
    uint32_t po = atomic_load(&node->prop);
    if (po) {
        struct prop_info *pi = at(po);
        uint32_t s = atomic_load(&pi->serial);
        if (s & LONG_FLAG) return; /* lange ro.-Werte sind unveränderlich */
        if (vl >= PROP_VALUE_MAX) vl = PROP_VALUE_MAX - 1;
        char *backup = pa->data + sizeof(struct prop_bt);
        memcpy(backup, pi->value, (s >> 24) + 1);
        s |= 1; atomic_store(&pi->serial, s);
        memcpy(pi->value, val, vl); pi->value[vl] = 0;
        atomic_store(&pi->serial, (uint32_t)(vl << 24) | ((s + 1) & 0xffffff));
        wake(&pi->serial);
    } else {
        uint32_t o; struct prop_info *pi = alloc(sizeof *pi + nl + 1, &o);
        if (!pi) return;
        memcpy(pi->name, name, nl + 1);
        if (vl >= PROP_VALUE_MAX && !strncmp(name, "ro.", 3)) {
            uint32_t lo; char *lv = alloc(vl + 1, &lo);
            if (!lv) return;
            memcpy(lv, val, vl + 1);
            strcpy(pi->lp.err, LONG_ERR);
            pi->lp.offset = lo - o;
            atomic_store(&pi->serial, (uint32_t)(strlen(LONG_ERR) << 24) | LONG_FLAG);
        } else {
            if (vl >= PROP_VALUE_MAX) vl = PROP_VALUE_MAX - 1;
            memcpy(pi->value, val, vl); pi->value[vl] = 0;
            atomic_store(&pi->serial, (uint32_t)(vl << 24));
        }
        atomic_store(&node->prop, o);
    }
    atomic_fetch_add(&pa->serial, 1);
    wake(&pa->serial);
}
static void load_file(const char *f) {
    FILE *in = fopen(f, "r");
    if (!in) return;
    char l[1024];
    while (fgets(l, sizeof l, in)) {
        char *s = l; while (*s == ' ' || *s == '\t') s++;
        if (*s == '#' || !strncmp(s, "import ", 7)) continue;
        char *e = strchr(s, '='); if (!e) continue;
        *e = 0; char *v = e + 1;
        v[strcspn(v, "\r\n")] = 0;
        char *k = s; size_t kl = strlen(k); while (kl && (k[kl - 1] == ' ')) k[--kl] = 0;
        if (kl) set(k, v);
    }
    fclose(in);
}
static void load_cmdline(void) {
    char b[4096] = {0};
    int fd = open("/proc/cmdline", O_RDONLY);
    if (fd < 0) return;
    ssize_t n = read(fd, b, sizeof b - 1); close(fd);
    if (n <= 0) return;
    for (char *t = strtok(b, " \n"); t; t = strtok(0, " \n")) {
        if (strncmp(t, "androidboot.", 12)) continue;
        char *e = strchr(t, '='); if (!e) continue;
        *e = 0;
        char k[256]; snprintf(k, sizeof k, "ro.boot.%s", t + 12);
        set(k, e + 1);
        if (!strcmp(t + 12, "hardware")) set("ro.hardware", e + 1);
    }
}
static int rd(int fd, void *b, size_t n) {
    size_t g = 0;
    while (g < n) { ssize_t r = read(fd, (char *)b + g, n - g); if (r <= 0) return -1; g += r; }
    return 0;
}
static int rdstr(int fd, char *b, size_t cap) {
    uint32_t len;
    if (rd(fd, &len, 4) || len >= cap) return -1;
    if (rd(fd, b, len)) return -1;
    b[len] = 0;
    return 0;
}
int main(int c, char **v) {
    if (c < 4) { fprintf(stderr, "propsvc <socket> <protokoll> <bereich> [prop-dateien...]\n"); return 2; }
    out = fopen(v[2], "a");
    if (!out) { perror(v[2]); return 1; }
    setvbuf(out, 0, _IOLBF, 0);
    unlink(v[3]);
    int af = open(v[3], O_RDWR | O_CREAT | O_EXCL | O_CLOEXEC, 0644);
    if (af < 0 || ftruncate(af, PA_SIZE)) { perror(v[3]); return 1; }
    fchmod(af, 0644);
    pa = mmap(0, PA_SIZE, PROT_READ | PROT_WRITE, MAP_SHARED, af, 0);
    if (pa == MAP_FAILED) { perror("mmap"); return 1; }
    memset(pa, 0, PA_SIZE);
    pa->magic = 0x504f5250; pa->version = 0xfc6ed0ab;
    pa->bytes_used = sizeof(struct prop_bt) + AL(PROP_VALUE_MAX);
    pa_data = PA_SIZE - sizeof(struct prop_area);
    for (int i = 4; i < c; i++) load_file(v[i]);
    load_cmdline();
    fprintf(out, "# Bereich bereit, %u Bytes belegt\n", pa->bytes_used);

    int s = socket(AF_UNIX, SOCK_STREAM, 0);
    struct sockaddr_un a = { .sun_family = AF_UNIX };
    strncpy(a.sun_path, v[1], sizeof a.sun_path - 1);
    unlink(v[1]);
    if (bind(s, (struct sockaddr *)&a, sizeof a) || listen(s, 16)) { perror("bind/listen"); return 1; }
    chmod(v[1], 0666);
    for (;;) {
        int f = accept(s, 0, 0);
        if (f < 0) { if (errno != EINTR) perror("accept"); continue; }
        uint32_t cmd;
        if (rd(f, &cmd, 4) == 0) {
            if (cmd == 0x00020001) { /* PROP_MSG_SETPROP2 */
                char n[256], val[8192];
                if (!rdstr(f, n, sizeof n) && !rdstr(f, val, sizeof val)) {
                    if (strncmp(n, "ctl.", 4)) set(n, val);
                    fprintf(out, "%s=%s\n", n, val);
                    uint32_t ok = 0; (void)!write(f, &ok, 4);
                }
            } else if (cmd == 1) { /* PROP_MSG_SETPROP: char name[32], value[92] */
                char n[33] = {0}, val[93] = {0};
                if (!rd(f, n, 32) && !rd(f, val, 92)) { set(n, val); fprintf(out, "%s=%s\n", n, val); }
            } else {
                fprintf(out, "# unbekannter Befehl 0x%x\n", cmd);
            }
        }
        close(f);
    }
}
