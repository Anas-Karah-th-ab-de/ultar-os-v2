/* gnssclient – GPS für Ultar über Samsungs GNSS-Dienst in der abgeschotteten Android-Umgebung.
 *
 * Der Kepler-GNSS-Kern des A30 wird von Samsungs gpsd (Firmware-Laden, Protokoll) betrieben; der
 * HIDL-Dienst vendor.samsung.hardware.gnss@2.0-service bietet android.hardware.gnss@1.0::IGnss an.
 * Dieser kleine Client spricht ihn direkt über /dev/hwbinder an (ohne Android-Bibliotheken) und
 * schreibt die Ergebnisse als Textzeilen nach <ausgabe> (Vorgabe /run/ultar/gnss.nmea):
 *   NMEA-Sätze unverändert ($GPGGA, $GPRMC, $GPGSV …)
 *   $ULLOC,<lat>,<lon>,<alt>,<genauigkeit m>,<m/s>,<grad>,<zeit ms>   (aus gnssLocationCb)
 *   $ULSV,<anzahl>,<benutzt>,<prn>:<el>:<az>:<cn0>:<u>,…              (aus gnssSvStatusCb)
 *
 *   gnssclient daemon [ausgabe] [wunschdatei]
 *     Läuft dauerhaft. GNSS ist nur an, solange <wunschdatei> (Vorgabe /run/ultar/gnss.want) jünger
 *     als 10 s ist – die Oberfläche berührt sie, solange ein Standort gebraucht wird.
 *   gnssclient test [sek]  – sofort starten und Zeilen auf stdout ausgeben. */
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
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>

#define GNSS "android.hardware.gnss@1.0::IGnss"
#define SM "android.hidl.manager@1.0::IServiceManager"
#define LASTFIX "/data/ultar/gnss.last"
#define MYNODE 0x6a6a1000ull
#define MYCOOKIE 0x6a6a2000ull

static int bfd;

typedef struct { uint8_t d[1024]; size_t n; binder_size_t off[8]; size_t no; size_t bufsz; } parcel;
static void p_raw(parcel *p, const void *v, size_t l) { memcpy(p->d + p->n, v, l); p->n += (l + 3) & ~3u; }
static void p_u32(parcel *p, uint32_t v) { p_raw(p, &v, 4); }
static void p_cstr(parcel *p, const char *s) { size_t l = strlen(s) + 1; memset(p->d + p->n, 0, (l + 3) & ~3u); p_raw(p, s, l); }
static void p_obj(parcel *p, const void *o, size_t l) { p->off[p->no++] = p->n; p_raw(p, o, l); }
static size_t p_buf(parcel *p, const void *b, size_t l, int has_parent, size_t parent, size_t poff) {
    struct binder_buffer_object o = { .hdr.type = BINDER_TYPE_PTR, .flags = has_parent ? BINDER_BUFFER_FLAG_HAS_PARENT : 0,
        .buffer = (uintptr_t)b, .length = l, .parent = parent, .parent_offset = poff };
    size_t idx = p->no; p_obj(p, &o, sizeof o); p->bufsz += (l + 7) & ~7u; return idx;
}
struct hidl_string { uint64_t ptr; uint32_t size; uint8_t owns; uint8_t pad[3]; };
static void p_string(parcel *p, struct hidl_string *hs, const char *s) {
    hs->ptr = (uintptr_t)s; hs->size = strlen(s); hs->owns = 0;
    size_t h = p_buf(p, hs, sizeof *hs, 0, 0, 0); p_buf(p, s, strlen(s) + 1, 1, h, 0);
}

static int bwr(void *w, size_t wl, void *r, size_t rl, size_t *got) {
    struct binder_write_read b = { .write_size = wl, .write_buffer = (uintptr_t)w, .read_size = rl, .read_buffer = (uintptr_t)r };
    int e; do e = ioctl(bfd, BINDER_WRITE_READ, &b); while (e < 0 && errno == EINTR);
    if (got) *got = b.read_consumed; return e;
}
static void cmd1(uint32_t c, const void *a, size_t l) { uint8_t b[64]; memcpy(b, &c, 4); if (l) memcpy(b + 4, a, l); bwr(b, 4 + l, 0, 0, 0); }
static void free_buf(binder_uintptr_t p) { cmd1(BC_FREE_BUFFER, &p, sizeof p); }

/* ---------- Ausgabe ---------- */
static FILE *out;
static const char *outpath;
static long nlines, nfix;
static void emit(const char *s) {
    if (!out) return;
    if (outpath && ftell(out) > 256 * 1024) { /* klein halten: neu beginnen (Leser erkennt Schrumpfen) */
        FILE *n = freopen(outpath, "w", out); if (!n) { out = 0; return; } out = n;
    }
    fputs(s, out); if (s[0] && s[strlen(s) - 1] != '\n') fputc('\n', out);
    fflush(out); nlines++;
}

/* Puffer-Objekte einer eingehenden Transaktion */
static const struct binder_buffer_object *bufobj(struct binder_transaction_data *t, int k) {
    const uint8_t *d = (const uint8_t *)(uintptr_t)t->data.ptr.buffer;
    const binder_size_t *of = (const binder_size_t *)(uintptr_t)t->data.ptr.offsets;
    size_t no = t->offsets_size / sizeof(binder_size_t);
    for (size_t i = 0; i < no; i++) {
        const struct binder_buffer_object *o = (const void *)(d + of[i]);
        if (o->hdr.type == BINDER_TYPE_PTR && k-- == 0) return o;
    }
    return 0;
}
static int verbose;
static void on_cb(struct binder_transaction_data *t) {
    const uint8_t *d = (const uint8_t *)(uintptr_t)t->data.ptr.buffer;
    size_t n = t->data_size, i = (strnlen((const char *)d, n) + 1 + 3) & ~3u;
    char line[2048];
    if (verbose) fprintf(stderr, "cb code=%u n=%zu tok=%.80s\n", t->code, n, (const char *)d);
    switch (t->code) {
    case 1: { /* gnssLocationCb(GnssLocation) */
        const struct binder_buffer_object *o = bufobj(t, 0);
        if (!o || o->length < 64) break;
        const uint8_t *g = (const uint8_t *)(uintptr_t)o->buffer;
        uint16_t fl; double la, lo, al; float sp, be, ha; int64_t ts;
        memcpy(&fl, g, 2); memcpy(&la, g + 8, 8); memcpy(&lo, g + 16, 8); memcpy(&al, g + 24, 8);
        memcpy(&sp, g + 32, 4); memcpy(&be, g + 36, 4); memcpy(&ha, g + 40, 4); memcpy(&ts, g + 56, 8);
        if (!(fl & 1)) break; /* HAS_LAT_LONG */
        snprintf(line, sizeof line, "$ULLOC,%.7f,%.7f,%.1f,%.1f,%.2f,%.1f,%lld", la, lo, (fl & 2) ? al : 0.0,
                 (fl & 16) ? ha : -1.0f, (fl & 4) ? sp : 0.0f, (fl & 8) ? be : 0.0f, (long long)ts);
        emit(line); nfix++;
        /* Hilfsdaten für den nächsten Start: letzte Position (höchstens alle 60 s schreiben) */
        { static time_t last; time_t now = time(0);
          if (now - last >= 60 && (fl & 16) && ha > 0 && ha < 500) {
              FILE *f = fopen(LASTFIX ".tmp", "w");
              if (f) { fprintf(f, "%.7f %.7f %.1f %ld\n", la, lo, ha, (long)now); fclose(f); rename(LASTFIX ".tmp", LASTFIX); last = now; }
          } }
        break; }
    case 3: { /* gnssSvStatusCb(GnssSvStatus { uint32 numSvs; GnssSvInfo[64] }) */
        const struct binder_buffer_object *o = bufobj(t, 0);
        if (!o || o->length < 4) break;
        const uint8_t *g = (const uint8_t *)(uintptr_t)o->buffer;
        uint32_t ns; memcpy(&ns, g, 4); if (ns > 64) ns = 64;
        if (o->length < 4 + ns * 24) break;
        int used = 0; size_t l = 0; char sv[1900] = "";
        for (uint32_t k = 0; k < ns; k++) {
            const uint8_t *s = g + 4 + k * 24;
            int16_t id; uint8_t c = s[2], f = s[20]; float cn, el, az;
            memcpy(&id, s, 2); memcpy(&cn, s + 4, 4); memcpy(&el, s + 8, 4); memcpy(&az, s + 12, 4);
            if (f & 4) used++;
            /* PRN nach Konstellation verschieben, damit sie eindeutig bleibt: GPS 1-32, GLONASS 65+, … */
            int prn = id + (c == 3 ? 64 : c == 5 ? 200 : c == 6 ? 300 : c == 4 ? 400 : c == 2 ? 120 : 0);
            if (l < sizeof sv - 40) l += snprintf(sv + l, sizeof sv - l, ",%d:%.0f:%.0f:%.0f:%d", prn, el, az, cn, (f & 4) ? 1 : 0);
        }
        snprintf(line, sizeof line, "$ULSV,%u,%d%s", ns, used, sv);
        emit(line);
        break; }
    case 4: { /* gnssNmeaCb(int64 timestamp, string nmea) */
        const struct binder_buffer_object *o = bufobj(t, 0);
        if (!o || o->length < sizeof(struct hidl_string)) break;
        const struct hidl_string *hs = (const void *)(uintptr_t)o->buffer;
        if (!hs->ptr || hs->size == 0 || hs->size > 4000) break;
        char nm[4096]; memcpy(nm, (const void *)(uintptr_t)hs->ptr, hs->size); nm[hs->size] = 0;
        /* kann mehrere Sätze enthalten */
        for (char *s = strtok(nm, "\r\n"); s; s = strtok(0, "\r\n")) if (*s == '$') emit(s);
        break; }
    case 2: if (verbose) fprintf(stderr, "status %u\n", i + 4 <= n ? d[i] : 0); break;
    default: break;
    }
}

static int pump(struct binder_transaction_data *reply, int wait_ms) {
    uint64_t rb[1024];
    for (;;) {
        if (!reply) {
            struct pollfd pf = { bfd, POLLIN, 0 };
            if (poll(&pf, 1, wait_ms) <= 0) return 1;
        }
        size_t got = 0;
        if (bwr(0, 0, rb, sizeof rb, &got) < 0) { perror("binder read"); return -1; }
        uint8_t *p = (uint8_t *)rb, *e = p + got; int have = 0;
        while (p < e) {
            uint32_t c; memcpy(&c, p, 4); p += 4;
            switch (c) {
            case BR_NOOP: case BR_TRANSACTION_COMPLETE: case BR_SPAWN_LOOPER: break;
            case BR_INCREFS: case BR_ACQUIRE: {
                struct binder_ptr_cookie pc; memcpy(&pc, p, sizeof pc); p += sizeof pc;
                cmd1(c == BR_INCREFS ? BC_INCREFS_DONE : BC_ACQUIRE_DONE, &pc, sizeof pc); break; }
            case BR_RELEASE: case BR_DECREFS: p += sizeof(struct binder_ptr_cookie); break;
            case BR_DEAD_BINDER: case BR_CLEAR_DEATH_NOTIFICATION_DONE: p += sizeof(binder_uintptr_t); break;
            case BR_ERROR: p += 4; fprintf(stderr, "BR_ERROR\n"); break;
            case BR_DEAD_REPLY: fprintf(stderr, "BR_DEAD_REPLY\n"); return -1;
            case BR_FAILED_REPLY: fprintf(stderr, "BR_FAILED_REPLY\n"); return -1;
            case BR_REPLY: memcpy(reply, p, sizeof *reply); p += sizeof *reply; have = 1; break;
            case BR_TRANSACTION: {
                struct binder_transaction_data t; memcpy(&t, p, sizeof t); p += sizeof t;
                on_cb(&t);
                if (!(t.flags & TF_ONE_WAY)) {
                    int32_t st = 0;
                    struct { uint32_t c; struct binder_transaction_data t; } __attribute__((packed)) r = { BC_REPLY, {0} };
                    r.t.data_size = 4; r.t.data.ptr.buffer = (uintptr_t)&st;
                    bwr(&r, sizeof r, 0, 0, 0);
                }
                free_buf(t.data.ptr.buffer);
                break; }
            default: if (_IOC_SIZE(c)) p += _IOC_SIZE(c); break;
            }
        }
        if (have && reply) return 0;
        if (!reply) return 0;
    }
}
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
/* IGnss-Aufruf mit bool-Ergebnis; liefert -1 bei Transportfehler, sonst 0/1 */
static int gcall(uint32_t h, uint32_t code, parcel *p) {
    struct binder_transaction_data r;
    if (call(h, code, p, &r)) return -1;
    int32_t st = rep_i32(&r, 0); int ok = st == 0 && r.data_size >= 8 ? (rep_i32(&r, 4) & 0xff) != 0 : (st == 0);
    if (verbose) fprintf(stderr, "code %u -> status %d ok %d\n", code, st, ok);
    free_buf(r.data.ptr.buffer);
    return st ? -1 : ok;
}
static int g_noargs(uint32_t h, uint32_t code) { parcel p = {0}; p_cstr(&p, GNSS); return gcall(h, code, &p); }

/* „A-GPS light": Ohne SUPL/Bahndaten verkürzen genaue Uhrzeit und ungefähre Position den Kaltstart deutlich.
 * Zeit nur, wenn die Systemuhr plausibel gestellt ist (NTP/Netz); Position aus dem letzten Fix (≤ 30 Tage). */
static void inject_aiding(uint32_t h) {
    struct timespec rt, bt; clock_gettime(CLOCK_REALTIME, &rt); clock_gettime(CLOCK_BOOTTIME, &bt);
    if (rt.tv_sec > 1735689600) { /* nach 2025-01-01 */
        parcel t = {0}; p_cstr(&t, GNSS);
        int64_t ms = (int64_t)rt.tv_sec * 1000 + rt.tv_nsec / 1000000, ref = (int64_t)bt.tv_sec * 1000 + bt.tv_nsec / 1000000;
        p_raw(&t, &ms, 8); p_raw(&t, &ref, 8); p_u32(&t, 2000);
        fprintf(stderr, "injectTime=%d\n", gcall(h, 5, &t));
    }
    FILE *f = fopen(LASTFIX, "r");
    if (f) {
        double la, lo, acc; long when;
        if (fscanf(f, "%lf %lf %lf %ld", &la, &lo, &acc, &when) == 4 && time(0) - when < 30L * 86400 && la >= -90 && la <= 90) {
            parcel l = {0}; p_cstr(&l, GNSS);
            float a = (float)(acc < 2000 ? 2000 : acc); /* Gerät kann sich seitdem bewegt haben */
            p_raw(&l, &la, 8); p_raw(&l, &lo, 8); p_raw(&l, &a, 4);
            fprintf(stderr, "injectLocation(%.3f,%.3f)=%d\n", la, lo, gcall(h, 6, &l));
        }
        fclose(f);
    }
}

static int gnss_on(uint32_t h) {
    inject_aiding(h);
    parcel m = {0}; p_cstr(&m, GNSS);
    p_u32(&m, 0);     /* GnssPositionMode STANDALONE (uint8, als 4 Bytes) */
    p_u32(&m, 0);     /* GnssPositionRecurrence PERIODIC */
    p_u32(&m, 1000);  /* minIntervalMs */
    p_u32(&m, 0);     /* preferredAccuracyMeters */
    p_u32(&m, 0);     /* preferredTimeMs */
    int r = gcall(h, 8, &m);
    int s = g_noargs(h, 2); /* start */
    fprintf(stderr, "gnss an: setPositionMode=%d start=%d\n", r, s);
    return s;
}

static double mtime_age(const char *f) {
    struct stat st; if (stat(f, &st)) return 1e9;
    struct timespec now; clock_gettime(CLOCK_REALTIME, &now);
    return (double)(now.tv_sec - st.st_mtime);
}

int main(int argc, char **argv) {
    const char *cmd = argc > 1 ? argv[1] : "test";
    setvbuf(stderr, 0, _IOLBF, 0);
    verbose = getenv("GNSSDEBUG") != 0;
    bfd = open("/dev/hwbinder", O_RDWR | O_CLOEXEC);
    if (bfd < 0) { perror("/dev/hwbinder"); return 2; }
    mmap(0, 1 << 20, PROT_READ, MAP_PRIVATE | MAP_NORESERVE, bfd, 0);
    uint32_t zero = 0; ioctl(bfd, BINDER_SET_MAX_THREADS, &zero);
    cmd1(BC_ENTER_LOOPER, 0, 0);

    uint32_t h = 0; int tries = 0;
    while (get_service(GNSS, &h)) {
        if (++tries > 30) { fprintf(stderr, "FEHLER kein GNSS-Dienst\n"); return 3; }
        sleep(1);
    }
    /* setCallback(IGnssCallback) */
    parcel p = {0}; p_cstr(&p, GNSS);
    struct flat_binder_object me = { .hdr.type = BINDER_TYPE_BINDER, .flags = 0x7f | FLAT_BINDER_FLAG_ACCEPTS_FDS,
                                     .binder = MYNODE, .cookie = MYCOOKIE };
    p_obj(&p, &me, sizeof me);
    int cb = gcall(h, 1, &p);
    fprintf(stderr, "setCallback=%d\n", cb);
    if (cb < 0) return 4;

    if (!strcmp(cmd, "test")) {
        out = stdout; verbose = 1;
        int sek = argc > 2 ? atoi(argv[2]) : 60;
        gnss_on(h);
        time_t t0 = time(0);
        while (time(0) - t0 < sek) if (pump(0, 1000) < 0) return 8;
        g_noargs(h, 3);
        fprintf(stderr, "ENDE zeilen=%ld fixe=%ld\n", nlines, nfix);
        return 0;
    }
    /* daemon */
    outpath = argc > 2 ? argv[2] : "/run/ultar/gnss.nmea";
    const char *want = argc > 3 ? argv[3] : "/run/ultar/gnss.want";
    out = fopen(outpath, "w");
    if (!out) { perror(outpath); return 2; }
    chmod(outpath, 0644);
    int on = 0;
    for (;;) {
        int w = mtime_age(want) < 10;
        if (w && !on) { if (gnss_on(h) < 0) return 8; on = 1; }
        if (!w && on) { g_noargs(h, 3); on = 0; fprintf(stderr, "gnss aus (zeilen=%ld fixe=%ld)\n", nlines, nfix); }
        if (pump(0, 1000) < 0) return 8;
    }
}
