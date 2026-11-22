/* fpclient – kleiner HIDL-Client direkt über /dev/hwbinder (ohne Android-Bibliotheken) für Samsungs
 * Fingerabdruckdienst (android.hardware.biometrics.fingerprint@2.1::IBiometricsFingerprint).
 *
 *   fpclient info            – Dienst holen, setNotify, setActiveGroup, getAuthenticatorId, enumerate
 *   fpclient auth [sek]      – Finger prüfen; Ausgabe "OK <fingerId>" / "NEIN" / "FEHLER <n>" / "ZEIT"
 *   fpclient enroll <hat-hex> [sek] – Finger anlernen mit HardwareAuthToken (69 Byte hex, aus Gatekeeper)
 *   fpclient remove <fid>    – Finger löschen (0 = alle)
 *   fpclient gk-set <pin> <datei>        – Gatekeeper: PIN anlegen, Passwort-Handle in <datei>
 *   fpclient learn <pin> <datei> [sek]   – Finger anlernen: preEnroll → Gatekeeper verify → enroll(HAT)
 *   fpclient daemon <socket> <gk-datei>  – Dauerdienst für Ultar (Zeilenprotokoll, siehe unten)
 *
 * Callbacks (oneway) werden als Zeilen "cb <name> ..." ausgegeben. Rückgabe 0 = Erfolg. */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <linux/android/binder.h>
#include <poll.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/un.h>
#include <time.h>
#include <unistd.h>

#define FP "android.hardware.biometrics.fingerprint@2.1::IBiometricsFingerprint"
#define CB "android.hardware.biometrics.fingerprint@2.1::IBiometricsFingerprintClientCallback"
#define SM "android.hidl.manager@1.0::IServiceManager"
#define GK "android.hardware.gatekeeper@1.0::IGatekeeper"
#define MYNODE 0x5a5a1000ull
#define MYCOOKIE 0x5a5a2000ull

static int bfd;
static void *bmap;

/* ---------- Parcel ---------- */
typedef struct {
    uint8_t d[2048]; size_t n;
    binder_size_t off[16]; size_t no;
    size_t bufsz;
} parcel;

static void p_raw(parcel *p, const void *v, size_t l) { memcpy(p->d + p->n, v, l); p->n += (l + 3) & ~3u; }
static void p_u32(parcel *p, uint32_t v) { p_raw(p, &v, 4); }
static void p_u64(parcel *p, uint64_t v) { p_raw(p, &v, 8); }
static void p_cstr(parcel *p, const char *s) { size_t l = strlen(s) + 1; memset(p->d + p->n, 0, (l + 3) & ~3u); p_raw(p, s, l); }
static void p_obj(parcel *p, const void *o, size_t l) { p->off[p->no++] = p->n; p_raw(p, o, l); }
/* Puffer-Objekt; gibt Index (Handle) zurück */
static size_t p_buf(parcel *p, const void *b, size_t l, int has_parent, size_t parent, size_t poff) {
    struct binder_buffer_object o = { .hdr.type = BINDER_TYPE_PTR, .flags = has_parent ? BINDER_BUFFER_FLAG_HAS_PARENT : 0,
        .buffer = (uintptr_t)b, .length = l, .parent = parent, .parent_offset = poff };
    size_t idx = p->no;
    p_obj(p, &o, sizeof o);
    p->bufsz += (l + 7) & ~7u;
    return idx;
}
struct hidl_string { uint64_t ptr; uint32_t size; uint8_t owns; uint8_t pad[3]; };
static void p_string(parcel *p, struct hidl_string *hs, const char *s) {
    hs->ptr = (uintptr_t)s; hs->size = strlen(s); hs->owns = 0;
    size_t h = p_buf(p, hs, sizeof *hs, 0, 0, 0);
    p_buf(p, s, strlen(s) + 1, 1, h, 0);
}

struct hidl_vec { uint64_t ptr; uint32_t size; uint8_t owns; uint8_t pad[3]; };
static void p_vec(parcel *p, struct hidl_vec *hv, const void *d, size_t l) {
    hv->ptr = (uintptr_t)d; hv->size = l; hv->owns = 0;
    size_t h = p_buf(p, hv, sizeof *hv, 0, 0, 0);
    p_buf(p, d, l, 1, h, 0);
}

/* ---------- Binder-Grundlagen ---------- */
static int bwr(void *w, size_t wl, void *r, size_t rl, size_t *got) {
    struct binder_write_read b = { .write_size = wl, .write_buffer = (uintptr_t)w, .read_size = rl, .read_buffer = (uintptr_t)r };
    int e;
    do e = ioctl(bfd, BINDER_WRITE_READ, &b); while (e < 0 && errno == EINTR);
    if (got) *got = b.read_consumed;
    return e;
}
static void cmd1(uint32_t c, const void *a, size_t l) {
    uint8_t b[64]; memcpy(b, &c, 4); if (l) memcpy(b + 4, a, l); bwr(b, 4 + l, 0, 0, 0);
}
static void free_buf(binder_uintptr_t p) { cmd1(BC_FREE_BUFFER, &p, sizeof p); }

/* ---------- Callback-Verarbeitung ---------- */
static int cb_done, cb_result = -1, cb_fid;
static int dfd = -1;     /* Daemon: verbundener Ultar-Client */
static int enum_n;       /* Daemon: Zähler für enumerate */
static void say(const char *f, ...) __attribute__((format(printf, 1, 2)));
#include <stdarg.h>
static void say(const char *f, ...) {
    if (dfd < 0) return;
    char b[128]; va_list ap; va_start(ap, f); int n = vsnprintf(b, sizeof b, f, ap); va_end(ap);
    if (n > 0 && write(dfd, b, n) < 0) {}
}
static int verbose = 1;

static void on_cb(struct binder_transaction_data *t) {
    const uint8_t *d = (const uint8_t *)(uintptr_t)t->data.ptr.buffer;
    size_t n = t->data_size, i = strlen((const char *)d) + 1;
    i = (i + 3) & ~3u;
    const uint8_t *a = d + i; /* nach Schnittstellen-Kennung */
    uint32_t u[4] = {0};
    if (i + 8 < n) memcpy(u, a + 8, n - i - 8 < 12 ? n - i - 8 : 12);
    if (getenv("FPDEBUG")) {
        printf("dbg code=%u n=%zu tok=%s off=%zu:", t->code, n, (const char *)d, i);
        for (size_t k = i; k < n && k < i + 40; k++) printf(" %02x", d[k]);
        printf("\n");
    }
    switch (t->code) {
    case 1: printf("cb enroll fid=%u gid=%u rest=%u\n", u[0], u[1], u[2]);
            if (u[2]) say("rest %u\n", u[2]); else say("fertig %u\n", u[0]);
            if (u[2] == 0) { cb_done = 1; cb_result = 0; cb_fid = u[0]; } break;
    case 2: if (verbose) printf("cb acquired info=%d vendor=%d\n", (int)u[0], (int)u[1]);
            say("acq %d\n", (int)u[0]); break;
    case 3: printf("cb authenticated fid=%u gid=%u\n", u[0], u[1]);
            if (u[0]) say("ok %u\n", u[0]); else say("nein\n");
            cb_done = 1; cb_result = u[0] ? 0 : 1; cb_fid = u[0]; break;
    case 4: printf("cb error %d vendor=%d\n", (int)u[0], (int)u[1]);
            if (u[0] != 5) say("fehler %d %d\n", (int)u[0], (int)u[1]);
            if (u[0] != 5) { cb_done = 1; cb_result = 100 + (int)u[0]; } /* 5 = CANCELED */ break;
    case 5: printf("cb removed fid=%u gid=%u rest=%u\n", u[0], u[1], u[2]);
            if (u[2] == 0) { cb_done = 1; say("entfernt\n"); } break;
    case 6: printf("cb enumerate fid=%u gid=%u rest=%u\n", u[0], u[1], u[2]);
            if (u[0]) enum_n++;
            if (u[2] == 0) { cb_done = 1; say("anzahl %d\n", enum_n); } break;
    default: if (verbose) printf("cb code=0x%x (%zu Bytes)\n", t->code, n);
    }
    fflush(stdout);
}

/* Liest und verarbeitet Rückmeldungen. reply != 0: bis BR_REPLY; sonst bis Zeitlimit (ms) oder cb_done. */
static int pump(struct binder_transaction_data *reply, int wait_ms) {
    uint64_t rb[512];
    struct timespec t0; clock_gettime(CLOCK_MONOTONIC, &t0);
    for (;;) {
        if (!reply && wait_ms >= 0) {
            if (cb_done) return 0;
            struct timespec t1; clock_gettime(CLOCK_MONOTONIC, &t1);
            long el = (t1.tv_sec - t0.tv_sec) * 1000 + (t1.tv_nsec - t0.tv_nsec) / 1000000;
            if (el >= wait_ms) return 1;
            struct pollfd pf = { bfd, POLLIN, 0 };
            if (poll(&pf, 1, wait_ms - el) <= 0) continue;
        }
        size_t got = 0;
        if (bwr(0, 0, rb, sizeof rb, &got) < 0) { perror("binder read"); return -1; }
        uint8_t *p = (uint8_t *)rb, *e = p + got;
        int have = 0;
        while (p < e) {
            uint32_t c; memcpy(&c, p, 4); p += 4;
            switch (c) {
            case BR_NOOP: case BR_TRANSACTION_COMPLETE: case BR_SPAWN_LOOPER: break;
            case BR_INCREFS: case BR_ACQUIRE: {
                struct binder_ptr_cookie pc; memcpy(&pc, p, sizeof pc); p += sizeof pc;
                cmd1(c == BR_INCREFS ? BC_INCREFS_DONE : BC_ACQUIRE_DONE, &pc, sizeof pc); break; }
            case BR_RELEASE: case BR_DECREFS: p += sizeof(struct binder_ptr_cookie); break;
            case BR_DEAD_BINDER: p += sizeof(binder_uintptr_t); break;
            case BR_CLEAR_DEATH_NOTIFICATION_DONE: p += sizeof(binder_uintptr_t); break;
            case BR_ERROR: p += 4; fprintf(stderr, "BR_ERROR\n"); break;
            case BR_DEAD_REPLY: fprintf(stderr, "BR_DEAD_REPLY\n"); return -1;
            case BR_FAILED_REPLY: fprintf(stderr, "BR_FAILED_REPLY\n"); return -1;
            case BR_REPLY: memcpy(reply, p, sizeof *reply); p += sizeof *reply; have = 1; break;
            case BR_TRANSACTION: {
                struct binder_transaction_data t; memcpy(&t, p, sizeof t); p += sizeof t;
                on_cb(&t);
                if (!(t.flags & TF_ONE_WAY)) { /* z. B. ping/interfaceChain: nur "OK" */
                    int32_t st = 0;
                    struct { uint32_t c; struct binder_transaction_data t; } __attribute__((packed)) r = { BC_REPLY, {0} };
                    r.t.data_size = 4; r.t.data.ptr.buffer = (uintptr_t)&st;
                    bwr(&r, sizeof r, 0, 0, 0);
                }
                free_buf(t.data.ptr.buffer);
                break; }
            default:
                if (_IOC_SIZE(c)) p += _IOC_SIZE(c);
                break;
            }
        }
        if (have && reply) return 0;
        if (!reply && wait_ms < 0) return 0;
    }
}

/* Aufruf mit Antwort; liefert Zeiger auf Antwortdaten (vom Aufrufer mit free_buf freigeben) */
static int call(uint32_t handle, uint32_t code, parcel *p, struct binder_transaction_data *rep) {
    struct { uint32_t c; struct binder_transaction_data_sg s; } __attribute__((packed)) w;
    memset(&w, 0, sizeof w);
    w.c = BC_TRANSACTION_SG;
    w.s.transaction_data.target.handle = handle;
    w.s.transaction_data.code = code;
    w.s.transaction_data.flags = TF_ACCEPT_FDS;
    w.s.transaction_data.data_size = p->n;
    w.s.transaction_data.offsets_size = p->no * sizeof(binder_size_t);
    w.s.transaction_data.data.ptr.buffer = (uintptr_t)p->d;
    w.s.transaction_data.data.ptr.offsets = (uintptr_t)p->off;
    w.s.buffers_size = p->bufsz;
    if (bwr(&w, sizeof w, 0, 0, 0) < 0) { perror("binder write"); return -1; }
    if (pump(rep, 0)) return -1;
    if (rep->flags & TF_STATUS_CODE) { fprintf(stderr, "Statusantwort %d\n", *(int32_t *)(uintptr_t)rep->data.ptr.buffer); free_buf(rep->data.ptr.buffer); return -1; }
    return 0;
}
static int32_t rep_i32(struct binder_transaction_data *r, size_t off) {
    int32_t v = 0; if (off + 4 <= r->data_size) memcpy(&v, (uint8_t *)(uintptr_t)r->data.ptr.buffer + off, 4); return v;
}
static uint64_t rep_u64(struct binder_transaction_data *r, size_t off) {
    uint64_t v = 0; if (off + 8 <= r->data_size) memcpy(&v, (uint8_t *)(uintptr_t)r->data.ptr.buffer + off, 8); return v;
}

static int get_service(const char *fq, uint32_t *h) {
    parcel p = {0}; struct hidl_string a, b;
    p_cstr(&p, SM); p_string(&p, &a, fq); p_string(&p, &b, "default");
    struct binder_transaction_data r;
    if (call(0, 1, &p, &r)) return -1;
    int32_t st = rep_i32(&r, 0);
    struct flat_binder_object o = {0};
    if (r.data_size >= 4 + sizeof o) memcpy(&o, (uint8_t *)(uintptr_t)r.data.ptr.buffer + 4, sizeof o);
    int ok = st == 0 && o.hdr.type == BINDER_TYPE_HANDLE;
    if (ok) { *h = o.handle; cmd1(BC_INCREFS, h, 4); cmd1(BC_ACQUIRE, h, 4); }
    free_buf(r.data.ptr.buffer);
    return ok ? 0 : -1;
}
/* einfacher FP-Aufruf mit RequestStatus/Int-Rückgabe */
static int fp_simple(uint32_t h, uint32_t code, parcel *p, int64_t *out) {
    struct binder_transaction_data r;
    if (call(h, code, p, &r)) return -1;
    int32_t st = rep_i32(&r, 0);
    if (out) *out = (code == 1 || code == 2 || code == 5) ? (int64_t)rep_u64(&r, 4) : rep_i32(&r, 4);
    free_buf(r.data.ptr.buffer);
    return st;
}
static int fp_noargs(uint32_t h, uint32_t code, int64_t *out) { parcel p = {0}; p_cstr(&p, FP); return fp_simple(h, code, &p, out); }

/* Gatekeeper-Antwort: status, dann GatekeeperResponse {int32 code; uint32 timeout; vec<uint8_t> data} */
static int gk_call(uint32_t g, uint32_t code, parcel *p, uint8_t *out, size_t cap, size_t *outl, int32_t *gcode) {
    struct binder_transaction_data r;
    if (call(g, code, p, &r)) return -1;
    int32_t st = rep_i32(&r, 0);
    *gcode = -99; *outl = 0;
    struct binder_buffer_object o;
    if (!st && r.data_size >= 4 + sizeof o) {
        memcpy(&o, (uint8_t *)(uintptr_t)r.data.ptr.buffer + 4, sizeof o);
        if (o.hdr.type == BINDER_TYPE_PTR && o.length >= 24) {
            const uint8_t *rs = (const uint8_t *)(uintptr_t)o.buffer;
            int32_t c; uint32_t to; struct hidl_vec v;
            memcpy(&c, rs, 4); memcpy(&to, rs + 4, 4); memcpy(&v, rs + 8, sizeof v);
            *gcode = c;
            if (v.size && v.size <= cap && v.ptr) { memcpy(out, (const void *)(uintptr_t)v.ptr, v.size); *outl = v.size; }
            if (c == 1) fprintf(stderr, "gatekeeper: erneut versuchen in %u ms\n", to);
        }
    }
    free_buf(r.data.ptr.buffer);
    return st;
}
static int gk_enroll(uint32_t g, const char *pin, uint8_t *handle, size_t *hl) {
    parcel p = {0}; struct hidl_vec a, b, c; int32_t gc;
    p_cstr(&p, GK); p_u32(&p, 0); p_vec(&p, &a, "", 0); p_vec(&p, &b, "", 0); p_vec(&p, &c, pin, strlen(pin));
    if (gk_call(g, 1, &p, handle, 512, hl, &gc)) return -1;
    printf("gatekeeper enroll code=%d handle=%zu Bytes\n", gc, *hl);
    return gc == 0 && *hl ? 0 : -1;
}
static int gk_verify(uint32_t g, uint64_t challenge, const uint8_t *handle, size_t hl, const char *pin, uint8_t *hat, size_t *hatl) {
    parcel p = {0}; struct hidl_vec a, b; int32_t gc;
    p_cstr(&p, GK); p_u32(&p, 0); p_u64(&p, challenge); p_vec(&p, &a, handle, hl); p_vec(&p, &b, pin, strlen(pin));
    if (gk_call(g, 2, &p, hat, 256, hatl, &gc)) return -1;
    printf("gatekeeper verify code=%d token=%zu Bytes\n", gc, *hatl);
    return (gc == 0 || gc == 2) && *hatl == 69 ? 0 : -1; /* 2 = OK, neu anlernen empfohlen */
}

static int hexval(char c) { return c >= '0' && c <= '9' ? c - '0' : c >= 'a' && c <= 'f' ? c - 'a' + 10 : c >= 'A' && c <= 'F' ? c - 'A' + 10 : -1; }

int main(int argc, char **argv) {
    const char *cmd = argc > 1 ? argv[1] : "info";
    setvbuf(stdout, 0, _IOLBF, 0);
    bfd = open("/dev/hwbinder", O_RDWR | O_CLOEXEC);
    if (bfd < 0) { perror("/dev/hwbinder"); return 2; }
    struct binder_version v; ioctl(bfd, BINDER_VERSION, &v);
    bmap = mmap(0, 1 << 20, PROT_READ, MAP_PRIVATE | MAP_NORESERVE, bfd, 0);
    uint32_t zero = 0; ioctl(bfd, BINDER_SET_MAX_THREADS, &zero);
    cmd1(BC_ENTER_LOOPER, 0, 0);

    if (!strcmp(cmd, "gk-set") && argc > 3) {
        uint32_t g; if (get_service(GK, &g)) { printf("FEHLER kein-gatekeeper\n"); return 3; }
        uint8_t hd[512]; size_t hl;
        if (gk_enroll(g, argv[2], hd, &hl)) { printf("FEHLER gatekeeper\n"); return 4; }
        FILE *f = fopen(argv[3], "wb"); if (!f || fwrite(hd, 1, hl, f) != hl) { perror(argv[3]); return 4; }
        fclose(f); printf("OK\n"); return 0;
    }
    uint32_t h;
    if (get_service("vendor.samsung.hardware.biometrics.fingerprint@3.0::ISehBiometricsFingerprint", &h) &&
        get_service(FP, &h)) { printf("FEHLER kein-dienst\n"); return 3; }

    /* setNotify(callback) */
    parcel p = {0}; p_cstr(&p, FP);
    struct flat_binder_object me = { .hdr.type = BINDER_TYPE_BINDER, .flags = 0x7f | FLAT_BINDER_FLAG_ACCEPTS_FDS,
                                     .binder = MYNODE, .cookie = MYCOOKIE };
    p_obj(&p, &me, sizeof me);
    int64_t dev = 0;
    int st = fp_simple(h, 1, &p, &dev);
    if (verbose) printf("setNotify status=%d device=0x%llx\n", st, (unsigned long long)dev);
    if (st || !dev) { printf("FEHLER setNotify\n"); return 4; }

    /* setActiveGroup(0, pfad) */
    parcel g = {0}; struct hidl_string hs; p_cstr(&g, FP); p_u32(&g, 0); p_string(&g, &hs, "/data/vendor_de/0/fpdata");
    int64_t rs = 0; st = fp_simple(h, 9, &g, &rs);
    if (verbose) printf("setActiveGroup status=%d rs=%lld\n", st, (long long)rs);


    if (!strcmp(cmd, "daemon") && argc > 3) {
        /* fpclient daemon <socket> <gk-handle-datei>
         * Zeilenprotokoll: auth | learn | cancel | count | remove <fid>
         * Antworten: acq n | ok <fid> | nein | fehler <n> <v> | rest n | fertig <fid> | anzahl n | entfernt | err <text> */
        const char *gkfile = argv[3];
        int ls = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
        struct sockaddr_un sa = { .sun_family = AF_UNIX };
        strncpy(sa.sun_path, argv[2], sizeof sa.sun_path - 1);
        unlink(argv[2]);
        if (bind(ls, (struct sockaddr *)&sa, sizeof sa) || listen(ls, 4)) { perror("socket"); return 2; }
        chmod(argv[2], 0660);
        printf("BEREIT\n");
        int busy = 0; /* 1 = auth läuft, 2 = learn läuft */
        char lb[256]; size_t ll = 0;
        for (;;) {
            struct pollfd pf[3] = { { bfd, POLLIN, 0 }, { ls, POLLIN, 0 }, { dfd, POLLIN, 0 } };
            if (poll(pf, dfd >= 0 ? 3 : 2, -1) < 0) { if (errno == EINTR) continue; return 9; }
            if (pf[0].revents & POLLIN) { cb_done = 0; if (pump(0, -1) < 0) return 8; }
            if (pf[1].revents & POLLIN) {
                int c = accept4(ls, 0, 0, SOCK_CLOEXEC);
                if (c >= 0) { if (dfd >= 0) { if (busy) fp_noargs(h, 6, 0); busy = 0; close(dfd); } dfd = c; ll = 0; }
            }
            if (dfd >= 0 && (pf[2].revents & (POLLIN | POLLHUP | POLLERR))) {
                ssize_t r = read(dfd, lb + ll, sizeof lb - 1 - ll);
                if (r <= 0) { if (busy) fp_noargs(h, 6, 0); busy = 0; close(dfd); dfd = -1; continue; }
                ll += r; lb[ll] = 0;
                char *nl;
                while ((nl = strchr(lb, '\n'))) {
                    *nl = 0;
                    if (!strcmp(lb, "auth")) {
                        if (busy) fp_noargs(h, 6, 0);
                        parcel a = {0}; p_cstr(&a, FP); p_u64(&a, 0); p_u32(&a, 0);
                        st = fp_simple(h, 10, &a, &rs);
                        if (st < 0) return 8;
                        if (rs) say("err authenticate %lld\n", (long long)rs); else busy = 1;
                    } else if (!strcmp(lb, "learn")) {
                        if (busy) fp_noargs(h, 6, 0);
                        busy = 0;
                        uint32_t g; uint8_t hd[512]; size_t hl = 0;
                        if (get_service(GK, &g)) { say("err gatekeeper\n"); goto next; }
                        FILE *f = fopen(gkfile, "rb"); if (f) { hl = fread(hd, 1, sizeof hd, f); fclose(f); }
                        if (!hl) { /* einmalig: Gatekeeper-Geheimnis des Geräts anlegen */
                            if (gk_enroll(g, "ultar-fp", hd, &hl)) { say("err gatekeeper-enroll\n"); goto next; }
                            f = fopen(gkfile, "wb"); if (f) { fwrite(hd, 1, hl, f); fclose(f); }
                        }
                        int64_t ch = 0; fp_noargs(h, 2, &ch);
                        uint8_t hat[256]; size_t hatl = 0;
                        if (gk_verify(g, (uint64_t)ch, hd, hl, "ultar-fp", hat, &hatl)) { fp_noargs(h, 4, 0); say("err gatekeeper-verify\n"); goto next; }
                        parcel e = {0}; p_cstr(&e, FP); p_buf(&e, hat, 69, 0, 0, 0); p_u32(&e, 0); p_u32(&e, 60);
                        st = fp_simple(h, 3, &e, &rs);
                        if (st < 0) return 8;
                        if (rs) { fp_noargs(h, 4, 0); say("err enroll %lld\n", (long long)rs); } else { busy = 2; say("los\n"); }
                    } else if (!strcmp(lb, "cancel")) {
                        fp_noargs(h, 6, 0); if (busy == 2) fp_noargs(h, 4, 0); busy = 0; say("abgebrochen\n");
                    } else if (!strcmp(lb, "count")) {
                        enum_n = 0; fp_noargs(h, 7, &rs);
                    } else if (!strncmp(lb, "remove ", 7)) {
                        parcel r = {0}; p_cstr(&r, FP); p_u32(&r, 0); p_u32(&r, atoi(lb + 7));
                        fp_simple(h, 8, &r, &rs);
                    } else say("err unbekannt\n");
                next:
                    memmove(lb, nl + 1, ll - (nl + 1 - lb)); ll -= nl + 1 - lb; lb[ll] = 0;
                }
                if (ll >= sizeof lb - 1) ll = 0;
            }
        }
    }
    if (!strcmp(cmd, "info")) {
        int64_t aid = 0; fp_noargs(h, 5, &aid); printf("authenticatorId=0x%llx\n", (unsigned long long)aid);
        cb_done = 0; fp_noargs(h, 7, &rs); printf("enumerate rs=%lld\n", (long long)rs);
        pump(0, 3000);
        return 0;
    }
    if (!strcmp(cmd, "auth")) {
        int sec = argc > 2 ? atoi(argv[2]) : 15;
        parcel a = {0}; p_cstr(&a, FP); p_u64(&a, 0); p_u32(&a, 0);
        st = fp_simple(h, 10, &a, &rs);
        if (st || rs) { printf("FEHLER authenticate %d/%lld\n", st, (long long)rs); return 5; }
        if (pump(0, sec * 1000) == 1) { fp_noargs(h, 6, 0); printf("ZEIT\n"); return 6; }
        fp_noargs(h, 6, 0);
        if (cb_result == 0) { printf("OK %d\n", cb_fid); return 0; }
        if (cb_result == 1) { printf("NEIN\n"); return 1; }
        printf("FEHLER %d\n", cb_result - 100); return 7;
    }
    if (!strcmp(cmd, "enroll") && argc > 2) {
        uint8_t hat[69] = {0};
        for (int i = 0; i < 69 && argv[2][2 * i] && argv[2][2 * i + 1]; i++)
            hat[i] = hexval(argv[2][2 * i]) << 4 | hexval(argv[2][2 * i + 1]);
        int sec = argc > 3 ? atoi(argv[3]) : 60;
        int64_t ch = 0; fp_noargs(h, 2, &ch); printf("preEnroll challenge=0x%llx\n", (unsigned long long)ch);
        parcel e = {0}; p_cstr(&e, FP); p_buf(&e, hat, sizeof hat, 0, 0, 0); p_u32(&e, 0); p_u32(&e, sec);
        st = fp_simple(h, 3, &e, &rs);
        if (st || rs) { printf("FEHLER enroll %d/%lld\n", st, (long long)rs); return 5; }
        int r = pump(0, sec * 1000);
        fp_noargs(h, 4, 0);
        if (r == 1) { printf("ZEIT\n"); return 6; }
        if (cb_result == 0) { printf("OK %d\n", cb_fid); return 0; }
        printf("FEHLER %d\n", cb_result - 100); return 7;
    }
    if (!strcmp(cmd, "learn") && argc > 3) {
        uint32_t g; if (get_service(GK, &g)) { printf("FEHLER kein-gatekeeper\n"); return 3; }
        uint8_t hd[512]; size_t hl = 0;
        FILE *f = fopen(argv[3], "rb"); if (f) { hl = fread(hd, 1, sizeof hd, f); fclose(f); }
        if (!hl) { printf("FEHLER kein-handle\n"); return 4; }
        int sec = argc > 4 ? atoi(argv[4]) : 60;
        int64_t ch = 0; fp_noargs(h, 2, &ch); printf("preEnroll challenge=0x%llx\n", (unsigned long long)ch);
        uint8_t hat[256]; size_t hatl = 0;
        if (gk_verify(g, (uint64_t)ch, hd, hl, argv[2], hat, &hatl)) { fp_noargs(h, 4, 0); printf("FEHLER pin\n"); return 4; }
        parcel e = {0}; p_cstr(&e, FP); p_buf(&e, hat, 69, 0, 0, 0); p_u32(&e, 0); p_u32(&e, sec);
        st = fp_simple(h, 3, &e, &rs);
        if (st || rs) { fp_noargs(h, 4, 0); printf("FEHLER enroll %d/%lld\n", st, (long long)rs); return 5; }
        printf("BEREIT: Finger mehrmals auflegen\n");
        int r = pump(0, sec * 1000);
        fp_noargs(h, 4, 0);
        if (r == 1) { printf("ZEIT\n"); return 6; }
        if (cb_result == 0) { printf("OK %d\n", cb_fid); return 0; }
        printf("FEHLER %d\n", cb_result - 100); return 7;
    }
    if (!strcmp(cmd, "remove") && argc > 2) {
        parcel r = {0}; p_cstr(&r, FP); p_u32(&r, 0); p_u32(&r, atoi(argv[2]));
        st = fp_simple(h, 8, &r, &rs); printf("remove %d/%lld\n", st, (long long)rs);
        pump(0, 3000); return 0;
    }
    fprintf(stderr, "fpclient info | auth [sek] | enroll <hat-hex> [sek] | remove <fid>\n");
    return 2;
}
