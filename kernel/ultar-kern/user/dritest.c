/* dritest: the GPU render node /dev/dri/renderD128 (kernel/ultar-kern/src/svcdev/dri.rs), used the way a
 * program uses the Linux xe driver: found in /dev/dri, opened, driver name and version, capabilities and
 * the device queries (two calls each: size, then data); wrong sizes, unknown requests and callers without
 * the right are refused. Then the GPU works for the program: buffers created and mapped, an address
 * space, bindings, a queue per engine, batches that store values into a buffer, waits on user fences
 * and on sync objects,
 * 600 jobs through one ring, rebinding, refusals, cleanup when a process just exits, and a batch that
 * never ends: its queue is stopped, the waiter is let go, and the engine works again afterwards.
 *                                                                                → "DRITEST: ALLES OK"
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
#include <sys/mman.h>
#include <sys/prctl.h>
#include <sys/stat.h>
#include <sys/sysmacros.h>
#include <sys/wait.h>
#include <time.h>
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
struct drm_gem_close {
    uint32_t handle, pad;
};
struct xe_gem_create {
    uint64_t extensions, size;
    uint32_t placement, flags, vm_id, handle;
    uint16_t cpu_caching, pad[3];
    uint64_t reserved[2];
};
struct xe_gem_mmap_offset {
    uint64_t extensions;
    uint32_t handle, flags;
    uint64_t offset, reserved[2];
};
struct xe_vm_create {
    uint64_t extensions;
    uint32_t flags, vm_id;
    uint64_t reserved[2];
};
struct xe_vm_destroy {
    uint32_t vm_id, pad;
    uint64_t reserved[2];
};
struct xe_vm_bind_op {
    uint64_t extensions;
    uint32_t obj;
    uint16_t pat_index, pad;
    uint64_t obj_offset, range, addr;
    uint32_t op, flags, prefetch, pad2;
    uint64_t reserved[3];
};
struct xe_vm_bind {
    uint64_t extensions;
    uint32_t vm_id, exec_queue_id, pad, num_binds;
    struct xe_vm_bind_op bind;
    uint32_t pad2, num_syncs;
    uint64_t syncs, reserved[2];
};
struct xe_engine_instance {
    uint16_t engine_class, engine_instance, gt_id, pad;
};
struct xe_exec_queue_create {
    uint64_t extensions;
    uint16_t width, num_placements;
    uint32_t vm_id, flags, exec_queue_id;
    uint64_t instances, reserved[2];
};
struct xe_exec_queue_destroy {
    uint32_t exec_queue_id, pad;
    uint64_t reserved[2];
};
struct xe_sync {
    uint64_t extensions;
    uint32_t type, flags;
    uint64_t addr, timeline_value, reserved[2];
};
struct xe_exec {
    uint64_t extensions;
    uint32_t exec_queue_id, num_syncs;
    uint64_t syncs, address;
    uint16_t num_batch_buffer, pad[3];
    uint64_t reserved[2];
};
struct xe_wait_user_fence {
    uint64_t extensions, addr;
    uint16_t op, flags;
    uint32_t pad;
    uint64_t value, mask;
    int64_t timeout;
    uint32_t exec_queue_id, pad2;
    uint64_t reserved[2];
};
struct drm_syncobj_create {
    uint32_t handle, flags;
};
struct drm_syncobj_wait {
    uint64_t handles;
    int64_t timeout_nsec;
    uint32_t count_handles, flags, first_signaled, pad;
};
struct drm_syncobj_timeline_wait {
    uint64_t handles, points;
    int64_t timeout_nsec;
    uint32_t count_handles, flags, first_signaled, pad;
};
#define IOCTL_SYNCOBJ_CREATE _IOWR('d', 0xbf, struct drm_syncobj_create)
#define IOCTL_SYNCOBJ_DESTROY _IOWR('d', 0xc0, struct drm_syncobj_create)
#define IOCTL_SYNCOBJ_WAIT _IOWR('d', 0xc3, struct drm_syncobj_wait)
#define IOCTL_SYNCOBJ_TIMELINE_WAIT _IOWR('d', 0xca, struct drm_syncobj_timeline_wait)
#define IOCTL_GEM_CLOSE _IOW('d', 0x09, struct drm_gem_close)
#define IOCTL_GEM_CREATE _IOWR('d', 0x41, struct xe_gem_create)
#define IOCTL_GEM_MMAP_OFFSET _IOWR('d', 0x42, struct xe_gem_mmap_offset)
#define IOCTL_VM_CREATE _IOWR('d', 0x43, struct xe_vm_create)
#define IOCTL_VM_DESTROY _IOW('d', 0x44, struct xe_vm_destroy)
#define IOCTL_VM_BIND _IOW('d', 0x45, struct xe_vm_bind)
#define IOCTL_QUEUE_CREATE _IOWR('d', 0x46, struct xe_exec_queue_create)
#define IOCTL_QUEUE_DESTROY _IOW('d', 0x47, struct xe_exec_queue_destroy)
struct xe_queue_get_property {
    uint64_t extensions;
    uint32_t exec_queue_id, property;
    uint64_t value, reserved[2];
};
#define IOCTL_QUEUE_GET_PROPERTY _IOWR('d', 0x48, struct xe_queue_get_property)
#define IOCTL_EXEC _IOW('d', 0x49, struct xe_exec)
#define IOCTL_WAIT_USER_FENCE _IOWR('d', 0x4a, struct xe_wait_user_fence)
#define IOCTL_OBSERVATION _IOW('d', 0x4b, char[24])
#define BATCH_VA 0x100000ull
#define DATA_VA 0x7f0000200000ull
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
    check("Fähigkeit Synchronisationsobjekte", ioctl(fd, IOCTL_GET_CAP, &c) == 0 && c.value == 1);
    c.capability = 0x5;
    check("Puffer zwischen Programmen teilen wird nicht behauptet", ioctl(fd, IOCTL_GET_CAP, &c) == 0 && c.value == 0);
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
    char obs[24] = {0};
    check("Anfrage, die es nicht gibt (Beobachtung): EINVAL", ioctl(fd, IOCTL_OBSERVATION, obs) == -1 && errno == EINVAL);
    check("fremde Anfrage (TCGETS): ENOTTY", ioctl(fd, 0x5401, buf) == -1 && errno == ENOTTY);
}

/* ---- the GPU works for the program ---- */

static uint32_t gem(int fd, uint64_t size) {
    struct xe_gem_create c = {.size = size, .placement = 1, .cpu_caching = 1};
    return ioctl(fd, IOCTL_GEM_CREATE, &c) == 0 ? c.handle : 0;
}

static void *gem_map(int fd, uint32_t handle, uint64_t size) {
    struct xe_gem_mmap_offset o = {.handle = handle};
    if (ioctl(fd, IOCTL_GEM_MMAP_OFFSET, &o) != 0) return MAP_FAILED;
    return mmap(NULL, size, PROT_READ | PROT_WRITE, MAP_SHARED, fd, o.offset);
}

static int bind(int fd, uint32_t vm, uint32_t obj, uint64_t offset, uint64_t range, uint64_t addr, uint32_t op) {
    struct xe_vm_bind b = {.vm_id = vm, .num_binds = 1, .bind = {.obj = obj, .obj_offset = offset, .range = range, .addr = addr, .op = op, .pat_index = 3}};
    if (op != 0) b.bind.pat_index = 0;
    return ioctl(fd, IOCTL_VM_BIND, &b);
}

static uint32_t queue(int fd, uint32_t vm, uint16_t class) {
    struct xe_engine_instance inst = {.engine_class = class};
    struct xe_exec_queue_create q = {.width = 1, .num_placements = 1, .vm_id = vm, .instances = (uintptr_t)&inst};
    return ioctl(fd, IOCTL_QUEUE_CREATE, &q) == 0 ? q.exec_queue_id : 0;
}

/* a batch at `batch` that stores `value` at address `va` of the program's GPU address space */
static void store_batch(uint32_t *batch, uint64_t va, uint32_t value) {
    batch[0] = 0x20u << 23 | 2; /* MI_STORE_DATA_IMM, one value */
    batch[1] = (uint32_t)va;
    batch[2] = (uint32_t)(va >> 32);
    batch[3] = value;
    batch[4] = 0xau << 23; /* MI_BATCH_BUFFER_END */
    batch[5] = 0;
}

/* runs the batch at BATCH_VA on `q` and waits until its fence holds `token`; 0 = done in time */
static int run(int fd, uint32_t q, volatile uint64_t *fence, uint64_t token) {
    struct xe_sync sync = {.type = 2, .flags = 1, .addr = (uintptr_t)fence, .timeline_value = token};
    struct xe_exec e = {.exec_queue_id = q, .num_syncs = 1, .syncs = (uintptr_t)&sync, .address = BATCH_VA, .num_batch_buffer = 1};
    if (ioctl(fd, IOCTL_EXEC, &e) != 0) return -1;
    struct xe_wait_user_fence w = {.addr = (uintptr_t)fence, .op = 0, .value = token, .mask = ~0ull, .timeout = 2000000000ll, .exec_queue_id = q};
    return ioctl(fd, IOCTL_WAIT_USER_FENCE, &w);
}

struct gpu {
    int fd;
    uint32_t vm, batch_h, data_h;
    uint32_t *batch;
    volatile uint32_t *data;
    volatile uint64_t fence;
};

static int setup(struct gpu *g, int fd) {
    g->fd = fd;
    g->batch_h = gem(fd, 4096);
    g->data_h = gem(fd, 8192);
    check("zwei Puffer angelegt", g->batch_h && g->data_h && g->batch_h != g->data_h);
    g->batch = gem_map(fd, g->batch_h, 4096);
    g->data = gem_map(fd, g->data_h, 8192);
    check("Puffer eingeblendet", g->batch != MAP_FAILED && g->data != MAP_FAILED);
    if (g->batch == MAP_FAILED || g->data == MAP_FAILED) return -1;
    check("neuer Puffer ist leer und beschreibbar", g->data[0] == 0 && g->data[1024] == 0 && (g->data[5] = 5) == 5);
    struct xe_vm_create v = {.flags = 1};
    check("Adressraum angelegt", ioctl(fd, IOCTL_VM_CREATE, &v) == 0 && v.vm_id);
    g->vm = v.vm_id;
    check("Auftrag und Daten eingebunden", bind(fd, g->vm, g->batch_h, 0, 4096, BATCH_VA, 0) == 0 && bind(fd, g->vm, g->data_h, 0, 8192, DATA_VA, 0) == 0);
    return 0;
}

static void engines(struct gpu *g) {
    static const struct {
        uint16_t class;
        const char *ok;
    } e[] = {{4, "Rechen-Engine schreibt in den Puffer des Programms"}, {0, "Render-Engine schreibt in den Puffer des Programms"}, {1, "Kopier-Engine schreibt in den Puffer des Programms"}};
    for (unsigned i = 0; i < 3; i++) {
        uint32_t q = queue(g->fd, g->vm, e[i].class);
        uint32_t value = 0xc0de0000u + i;
        store_batch(g->batch, DATA_VA + 16 * i, value);
        g->fence = 0;
        int r = q ? run(g->fd, q, &g->fence, 0x1000 + i) : -1;
        if (r != 0) printf("    Warteschlange %u, Rückgabe %d, errno %d, Marke %#llx, Wert %#x\n", q, r, errno, (unsigned long long)g->fence, g->data[4 * i]);
        check(e[i].ok, r == 0 && g->fence == 0x1000 + i && g->data[4 * i] == value);
        struct xe_exec_queue_destroy d = {.exec_queue_id = q};
        check("Warteschlange beendet", q && ioctl(g->fd, IOCTL_QUEUE_DESTROY, &d) == 0);
    }
}

static int64_t in_ns(int64_t ns) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return (int64_t)t.tv_sec * 1000000000ll + t.tv_nsec + ns;
}

/* the end of a job travels through sync objects: a binary one, and a point of a timeline */
static void sync_objects(struct gpu *g) {
    int fd = g->fd;
    uint32_t q = queue(fd, g->vm, 4);
    struct drm_syncobj_create so = {0}, tl = {0};
    check("Synchronisationsobjekte angelegt", ioctl(fd, IOCTL_SYNCOBJ_CREATE, &so) == 0 && ioctl(fd, IOCTL_SYNCOBJ_CREATE, &tl) == 0 && so.handle && tl.handle);
    struct drm_syncobj_wait w = {.handles = (uintptr_t)&so.handle, .count_handles = 1, .timeout_nsec = in_ns(50000000)};
    check("Warten auf ein Objekt, an dem nichts hängt: EINVAL", ioctl(fd, IOCTL_SYNCOBJ_WAIT, &w) == -1 && errno == EINVAL);
    store_batch(g->batch, DATA_VA + 512, 0x51c0);
    struct xe_sync syncs[2] = {{.type = 0, .flags = 1, .addr = so.handle}, {.type = 1, .flags = 1, .addr = tl.handle, .timeline_value = 3}};
    struct xe_exec e = {.exec_queue_id = q, .num_syncs = 2, .syncs = (uintptr_t)syncs, .address = BATCH_VA, .num_batch_buffer = 1};
    int sent = q && ioctl(fd, IOCTL_EXEC, &e) == 0;
    w.timeout_nsec = in_ns(2000000000ll);
    check("Auftrag meldet sein Ende über ein Synchronisationsobjekt", sent && ioctl(fd, IOCTL_SYNCOBJ_WAIT, &w) == 0 && g->data[128] == 0x51c0);
    uint64_t point = 3;
    struct drm_syncobj_timeline_wait tw = {.handles = (uintptr_t)&tl.handle, .points = (uintptr_t)&point, .count_handles = 1, .timeout_nsec = in_ns(2000000000ll)};
    check("und über einen Punkt einer Zeitleiste", ioctl(fd, IOCTL_SYNCOBJ_TIMELINE_WAIT, &tw) == 0);
    /* the next job waits for the first one's object before it runs */
    store_batch(g->batch, DATA_VA + 516, 0x51c1);
    struct xe_sync chain[2] = {{.type = 0, .flags = 0, .addr = so.handle}, {.type = 2, .flags = 1, .addr = (uintptr_t)&g->fence, .timeline_value = 0x51}};
    struct xe_exec e2 = {.exec_queue_id = q, .num_syncs = 2, .syncs = (uintptr_t)chain, .address = BATCH_VA, .num_batch_buffer = 1};
    g->fence = 0;
    struct xe_wait_user_fence uw = {.addr = (uintptr_t)&g->fence, .value = 0x51, .mask = ~0ull, .timeout = 2000000000ll};
    check("ein Auftrag, der auf ein Synchronisationsobjekt wartet, läuft danach", ioctl(fd, IOCTL_EXEC, &e2) == 0 && ioctl(fd, IOCTL_WAIT_USER_FENCE, &uw) == 0 && g->data[129] == 0x51c1);
    check("Synchronisationsobjekte beendet", ioctl(fd, IOCTL_SYNCOBJ_DESTROY, &so) == 0 && ioctl(fd, IOCTL_SYNCOBJ_DESTROY, &tl) == 0);
    struct xe_exec_queue_destroy d = {.exec_queue_id = q};
    ioctl(fd, IOCTL_QUEUE_DESTROY, &d);
}

static void many_and_rebind(struct gpu *g) {
    uint32_t q = queue(g->fd, g->vm, 4);
    int bad = q ? 0 : 1;
    /* 600 jobs: more than one ring holds, each with its own value */
    for (uint32_t i = 0; i < 600 && !bad; i++) {
        store_batch(g->batch, DATA_VA + 256, 0xab000000u + i);
        g->fence = 0;
        if (run(g->fd, q, &g->fence, i + 1) != 0 || g->data[64] != 0xab000000u + i) {
            printf("    Auftrag %u: errno %d, Marke %llu, Wert %#x\n", i, errno, (unsigned long long)g->fence, g->data[64]);
            bad = 1;
        }
    }
    check("600 Aufträge nacheinander auf einer Warteschlange", !bad);
    /* the second page of the data buffer moves to another address; the old one is gone */
    uint64_t moved = 0x400000000ull;
    check("Daten ausbinden und zweite Seite woanders einbinden", bind(g->fd, g->vm, 0, 0, 8192, DATA_VA, 1) == 0 && bind(g->fd, g->vm, g->data_h, 4096, 4096, moved, 0) == 0);
    store_batch(g->batch, moved + 8, 0x5eed);
    g->fence = 0;
    check("Auftrag schreibt über die neue Adresse in die zweite Seite", q && run(g->fd, q, &g->fence, 77) == 0 && g->data[1024 + 2] == 0x5eed);
    /* a fence that is never stored: the wait ends with ETIME */
    volatile uint64_t never = 0;
    struct xe_wait_user_fence w = {.addr = (uintptr_t)&never, .value = 1, .mask = ~0ull, .timeout = 20000000ll};
    check("Warten ohne Ergebnis endet mit ETIME", ioctl(g->fd, IOCTL_WAIT_USER_FENCE, &w) == -1 && errno == ETIME);
    struct xe_vm_destroy vd = {.vm_id = g->vm};
    check("Adressraum mit Warteschlange: EBUSY", ioctl(g->fd, IOCTL_VM_DESTROY, &vd) == -1 && errno == EBUSY);
    struct xe_exec_queue_destroy d = {.exec_queue_id = q};
    check("Warteschlange beendet, Adressraum beendet", ioctl(g->fd, IOCTL_QUEUE_DESTROY, &d) == 0 && ioctl(g->fd, IOCTL_VM_DESTROY, &vd) == 0);
}

static void gpu_refusals(struct gpu *g) {
    int fd = g->fd;
    check("Puffergröße 0 und krumme Größe: EINVAL", gem(fd, 0) == 0 && errno == EINVAL && gem(fd, 100) == 0 && errno == EINVAL);
    struct xe_gem_mmap_offset o = {.handle = 9999};
    check("Einblenden eines fremden Puffers: ENOENT", ioctl(fd, IOCTL_GEM_MMAP_OFFSET, &o) == -1 && errno == ENOENT);
    check("mmap ohne gültigen Puffer: Fehler", mmap(NULL, 4096, PROT_READ, MAP_SHARED, fd, 0x7777ull << 32) == MAP_FAILED);
    check("mmap über das Ende des Puffers: Fehler", gem_map(fd, g->batch_h, 8192) == MAP_FAILED);
    struct xe_vm_create v = {0};
    ioctl(fd, IOCTL_VM_CREATE, &v);
    check("Einbinden über das Pufferende: EINVAL", bind(fd, v.vm_id, g->batch_h, 0, 8192, BATCH_VA, 0) == -1 && errno == EINVAL);
    check("Einbinden in einen fremden Adressraum: ENOENT", bind(fd, 9999, g->batch_h, 0, 4096, BATCH_VA, 0) == -1 && errno == ENOENT);
    check("Warteschlange auf einer Engine, die es nicht gibt: EINVAL", queue(fd, v.vm_id, 2) == 0 && errno == EINVAL);
    struct xe_exec e = {.exec_queue_id = 9999, .num_batch_buffer = 1};
    check("Ausführen auf fremder Warteschlange: ENOENT", ioctl(fd, IOCTL_EXEC, &e) == -1 && errno == ENOENT);
    /* another open of the node has its own handles */
    int fd2 = open(NODE, O_RDWR | O_CLOEXEC);
    struct xe_gem_mmap_offset o2 = {.handle = g->batch_h};
    check("zweites Öffnen kennt die Puffer des ersten nicht: ENOENT", fd2 >= 0 && ioctl(fd2, IOCTL_GEM_MMAP_OFFSET, &o2) == -1 && errno == ENOENT);
    close(fd2);
    struct drm_gem_close c = {.handle = g->batch_h};
    check("Puffer schließen, zweimal: EINVAL", ioctl(fd, IOCTL_GEM_CLOSE, &c) == 0 && ioctl(fd, IOCTL_GEM_CLOSE, &c) == -1 && errno == EINVAL);
    check("eingeblendeter Puffer bleibt nach dem Schließen lesbar", g->batch[4] == 0xau << 23);
}

/* a process that creates everything, runs a job and exits without cleaning up; afterwards the
 * GPU still works for the next one */
static void exit_without_cleanup(void) {
    for (int round = 0; round < 2; round++) {
        pid_t c = fork();
        if (c == 0) {
            struct gpu g = {0};
            fails = 0;
            int fd = open(NODE, O_RDWR | O_CLOEXEC);
            if (fd < 0 || setup(&g, fd) != 0) _exit(1);
            uint32_t q = queue(fd, g.vm, 4);
            store_batch(g.batch, DATA_VA, 0x77 + round);
            int ok = q && run(fd, q, &g.fence, 9) == 0 && g.data[0] == 0x77u + round;
            _exit(ok && !fails ? 0 : 1);
        }
        int st = -1;
        waitpid(c, &st, 0);
        check(round ? "nach einem Prozess, der nicht aufgeräumt hat, arbeitet die Grafik weiter" : "Prozess rechnet und endet ohne aufzuräumen", WIFEXITED(st) && WEXITSTATUS(st) == 0);
    }
}

static double seconds(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return t.tv_sec + t.tv_nsec / 1e9;
}

/* a batch that jumps to itself forever: the kernel stops the queue after its job time is over */
static void hung_job(int fd) {
    struct gpu g = {0};
    if (setup(&g, fd) != 0) return;
    uint32_t q = queue(fd, g.vm, 4);
    g.batch[0] = 0x31u << 23 | 1u << 8 | 1; /* MI_BATCH_BUFFER_START in this address space */
    g.batch[1] = (uint32_t)BATCH_VA;
    g.batch[2] = (uint32_t)(BATCH_VA >> 32);
    struct xe_sync sync = {.type = 2, .flags = 1, .addr = (uintptr_t)&g.fence, .timeline_value = 0xdead};
    struct xe_exec e = {.exec_queue_id = q, .num_syncs = 1, .syncs = (uintptr_t)&sync, .address = BATCH_VA, .num_batch_buffer = 1};
    double t0 = seconds();
    int sent = q && ioctl(fd, IOCTL_EXEC, &e) == 0;
    struct xe_wait_user_fence w = {.addr = (uintptr_t)&g.fence, .value = 0xdead, .mask = ~0ull, .timeout = 15000000000ll};
    int waited = ioctl(fd, IOCTL_WAIT_USER_FENCE, &w);
    double took = seconds() - t0;
    printf("    hängender Auftrag: Warten kehrte nach %.1f s zurück (Rückgabe %d, errno %d)\n", took, waited, errno);
    check("hängender Auftrag: der Wartende wird nach der Auftragszeit entlassen", sent && waited == 0 && took > 3.0 && took < 12.0);
    struct xe_queue_get_property p = {.exec_queue_id = q};
    check("Warteschlange ist als gestoppt gemeldet", ioctl(fd, IOCTL_QUEUE_GET_PROPERTY, &p) == 0 && p.value == 1);
    check("gestoppte Warteschlange nimmt nichts mehr an: ECANCELED", ioctl(fd, IOCTL_EXEC, &e) == -1 && errno == ECANCELED);
    struct xe_exec_queue_destroy d = {.exec_queue_id = q};
    check("gestoppte Warteschlange beendet", ioctl(fd, IOCTL_QUEUE_DESTROY, &d) == 0);
    /* the engine was reset: it must work again, and so must the others */
    static const uint16_t classes[] = {4, 0, 1};
    for (unsigned i = 0; i < 3; i++) {
        uint32_t nq = queue(fd, g.vm, classes[i]);
        store_batch(g.batch, DATA_VA + 64, 0x600d0000u + i);
        g.fence = 0;
        int ok = nq && run(fd, nq, &g.fence, 5) == 0 && g.data[16] == 0x600d0000u + i;
        check(i == 0 ? "danach arbeitet die Rechen-Engine wieder" : i == 1 ? "danach arbeitet die Render-Engine" : "danach arbeitet die Kopier-Engine", ok);
    }
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
        struct gpu g = {0};
        struct xe_gem_create probe = {.size = 4096, .placement = 1, .cpu_caching = 1};
        if (ioctl(fd, IOCTL_GEM_CREATE, &probe) == -1 && errno == ENODEV) {
            printf("    Grafik nicht in Betrieb: nur die Abfragen geprüft\n");
        } else if (setup(&g, fd) == 0) {
            engines(&g);
            sync_objects(&g);
            many_and_rebind(&g);
            gpu_refusals(&g);
            exit_without_cleanup();
            hung_job(fd);
        }
        close(fd);
    }
    without_right();
    printf(fails ? "DRITEST: FEHLER (%d)\n" : "DRITEST: ALLES OK\n", fails);
    return fails != 0;
}
