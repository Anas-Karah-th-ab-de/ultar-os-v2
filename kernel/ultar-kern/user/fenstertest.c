/* fenstertest – what a compositor needs from the Ultar kernel (docs/COMPOSITOR.md §3, K1–K13):
 * memfd_create with seals, MAP_SHARED of a memory object across fork and across processes, descriptor
 * passing (SCM_RIGHTS) and MSG_DONTWAIT on Unix sockets, POLLOUT wake-ups, poll timeouts below a second,
 * mremap, mmap and the vertical blank of /dev/fb0, owner/group/mode of device nodes (the seat group),
 * evdev capability ioctls and /sys/class/input. One line per check ("ok" / "FEHLER"); the last line is
 * "FENSTERTEST: ALLES OK" when every check passed. Runs as root with all rights. */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <linux/fb.h>
#include <linux/input.h>
#include <linux/netlink.h>
#include <poll.h>
#include <pthread.h>
#include <signal.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <sys/prctl.h>
#include <sys/select.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/sysmacros.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>
#include "fenstertest.h"

int fails;

void say(const char *fmt, ...) {
    char b[512];
    va_list ap;
    va_start(ap, fmt);
    int n = vsnprintf(b, sizeof b, fmt, ap);
    va_end(ap);
    if (n > (int)sizeof b - 1) n = sizeof b - 1;
    if (n > 0 && write(1, b, n) < 0) fails++;
}

void check(const char *name, int ok) {
    if (!ok) fails++;
    say("%s %s\n", ok ? "ok    " : "FEHLER", name);
}

void denied(const char *name, long r, int want) {
    int e = errno, ok = r == -1 && e == want;
    if (!ok) fails++;
    say("%s %s -> %s\n", ok ? "ok    " : "FEHLER", name, r == -1 ? strerror(e) : "angenommen");
}

long now_us(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return t.tv_sec * 1000000L + t.tv_nsec / 1000;
}

/* a memory object of `size` bytes, sealable */
static int object(size_t size) {
    int fd = memfd_create("fenster", MFD_CLOEXEC | MFD_ALLOW_SEALING);
    if (fd >= 0 && ftruncate(fd, size) != 0) {
        close(fd);
        return -1;
    }
    return fd;
}

/* ---------------------------------------------------------------- K1–K3: memory objects */

static void memfd_checks(void) {
    int fd = memfd_create("fenster", MFD_CLOEXEC | MFD_ALLOW_SEALING);
    check("K1 memfd_create liefert einen Deskriptor mit close-on-exec", fd >= 0 && (fcntl(fd, F_GETFD) & FD_CLOEXEC));
    denied("K1 memfd_create: unbekannte Flags", memfd_create("x", 0x100), EINVAL);
    struct stat st;
    check("K1 ftruncate 10000, fstat: reguläre Datei, Größe 10000", ftruncate(fd, 10000) == 0 && fstat(fd, &st) == 0 && S_ISREG(st.st_mode) && st.st_size == 10000);
    check("K1 write/pread/lseek", pwrite(fd, "Fenster", 7, 4096) == 7 && lseek(fd, 4096, SEEK_SET) == 4096);
    char b[16] = {0};
    check("K1 fallocate vergrößert (Modus 0), KEEP_SIZE ändert nichts", fallocate(fd, 0, 0, 12288) == 0 && fallocate(fd, FALLOC_FL_KEEP_SIZE, 0, 65536) == 0 && fstat(fd, &st) == 0 && st.st_size == 12288 && st.st_blocks * 512 >= 12288 && ftruncate(fd, 10000) == 0);
    check("K1 read liest das Geschriebene", read(fd, b, 7) == 7 && memcmp(b, "Fenster", 7) == 0 && lseek(fd, 0, SEEK_END) == 10000);
    char *m = mmap(NULL, 12288, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    check("K3 mmap MAP_SHARED", m != MAP_FAILED);
    if (m == MAP_FAILED) return;
    check("K3 die Abbildung zeigt den Inhalt", memcmp(m + 4096, "Fenster", 7) == 0);
    memcpy(m + 100, "geteilt", 7);
    check("K3 ein Schreiben in die Abbildung steht in der Datei", pread(fd, b, 7, 100) == 7 && memcmp(b, "geteilt", 7) == 0);
    check("K3 pwrite erscheint in der Abbildung", pwrite(fd, "XY", 2, 200) == 2 && m[200] == 'X' && m[201] == 'Y');
    char *m2 = mmap(NULL, 4096, PROT_READ, MAP_SHARED, fd, 4096);
    check("K3 zweite Abbildung mit Versatz sieht dieselben Seiten", m2 != MAP_FAILED && memcmp(m2, "Fenster", 7) == 0);
    pid_t c = fork();
    if (c == 0) {
        _exit(memcmp(m + 100, "geteilt", 7) == 0 && (memcpy(m + 300, "vom Kind", 8), 1) ? 0 : 1);
    }
    int ws = -1;
    check("K3 nach fork teilen Eltern und Kind die Seiten", c > 0 && waitpid(c, &ws, 0) == c && ws == 0 && memcmp(m + 300, "vom Kind", 8) == 0);
    char *p = mmap(NULL, 4096, PROT_READ | PROT_WRITE, MAP_PRIVATE, fd, 0);
    check("K3 MAP_PRIVATE: eigene Kopie", p != MAP_FAILED && memcmp(p + 100, "geteilt", 7) == 0 && (p[100] = 'G', m[100] == 'g'));
    if (p != MAP_FAILED) munmap(p, 4096);
    /* seals */
    check("K2 F_GET_SEALS: anfangs keine", fcntl(fd, F_GET_SEALS) == 0);
    check("K2 F_ADD_SEALS SHRINK|GROW", fcntl(fd, F_ADD_SEALS, F_SEAL_SHRINK | F_SEAL_GROW) == 0 && fcntl(fd, F_GET_SEALS) == (F_SEAL_SHRINK | F_SEAL_GROW));
    denied("K2 versiegelt: verkleinern", ftruncate(fd, 4096), EPERM);
    denied("K2 versiegelt: vergrößern", ftruncate(fd, 1 << 20), EPERM);
    denied("K2 versiegelt: über das Ende hinaus schreiben", pwrite(fd, "x", 1, 20000), EPERM);
    denied("K2 SEAL_WRITE bei beschreibbarer Abbildung", fcntl(fd, F_ADD_SEALS, F_SEAL_WRITE), EBUSY);
    munmap(m, 12288);
    check("K2 SEAL_WRITE ohne beschreibbare Abbildung", fcntl(fd, F_ADD_SEALS, F_SEAL_WRITE) == 0);
    denied("K2 versiegelt: write", pwrite(fd, "x", 1, 0), EPERM);
    check("K2 versiegelt: Abbildung MAP_SHARED schreibbar abgewiesen", mmap(NULL, 4096, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0) == MAP_FAILED && errno == EPERM);
    m = mmap(NULL, 4096, PROT_READ, MAP_SHARED, fd, 0);
    check("K2 versiegelt: nur lesende Abbildung geht", m != MAP_FAILED && m[100] == 'g');
    check("K2 SEAL_SEAL", fcntl(fd, F_ADD_SEALS, F_SEAL_SEAL) == 0);
    denied("K2 nach SEAL_SEAL: keine weiteren Siegel", fcntl(fd, F_ADD_SEALS, F_SEAL_FUTURE_WRITE), EPERM);
    int plain = memfd_create("ohne", 0);
    check("K2 ohne MFD_ALLOW_SEALING: F_SEAL_SEAL gesetzt", plain >= 0 && fcntl(plain, F_GET_SEALS) == F_SEAL_SEAL);
    denied("K2 ohne MFD_ALLOW_SEALING: F_ADD_SEALS", fcntl(plain, F_ADD_SEALS, F_SEAL_WRITE), EPERM);
    denied("K2 F_ADD_SEALS auf einem Rohr", fcntl(1, F_ADD_SEALS, F_SEAL_WRITE), EINVAL);
    close(plain);
    close(fd);
    check("K3 die Abbildung hält das Objekt nach close", m != MAP_FAILED && m[100] == 'g');
    if (m != MAP_FAILED) munmap(m, 4096);
    if (m2 != MAP_FAILED) munmap(m2, 4096);
}

/* ---------------------------------------------------------------- K13: mremap */

static void mremap_checks(void) {
    size_t small = 64 << 10, big = 256 << 10;
    unsigned char *a = mmap(NULL, small, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    for (size_t i = 0; i < small; i += 4096) a[i] = (unsigned char)(i >> 12);
    unsigned char *b = mremap(a, small, big, MREMAP_MAYMOVE);
    int same = b != MAP_FAILED;
    for (size_t i = 0; same && i < small; i += 4096) same = b[i] == (unsigned char)(i >> 12);
    check("K13 mremap MAYMOVE vergrößert, der Inhalt bleibt", same);
    check("K13 der neue Teil ist genullt und beschreibbar", same && b[big - 1] == 0 && (b[big - 1] = 7) == 7);
    check("K13 verkleinern bleibt an der Stelle", same && mremap(b, big, small, 0) == b);
    // without MAYMOVE as Linux: in place when the space behind is free, else ENOMEM
    check("K13 vergrößern ohne MAYMOVE: an der Stelle, wenn dahinter frei", same && mremap(b, small, big, 0) == b);
    unsigned char *wall = same ? mmap(b + big, 4096, PROT_READ, MAP_PRIVATE | MAP_ANONYMOUS | 0x100000 /* MAP_FIXED_NOREPLACE */, -1, 0) : MAP_FAILED;
    errno = 0;
    check("K13 vergrößern ohne MAYMOVE: ENOMEM, wenn dahinter belegt", wall == b + big && mremap(b, big, big + 4096, 0) == MAP_FAILED && errno == ENOMEM);
    if (wall != MAP_FAILED) munmap(wall, 4096);
    denied("K13 unbekannte Flags", (long)mremap(b, small, big, 0x80), EINVAL);
    if (same) munmap(b, big);
    char *r = malloc(1 << 20);
    memset(r, 5, 1 << 20);
    r = realloc(r, 8 << 20);
    check("K13 realloc eines großen Blocks (glibc über mremap) behält den Inhalt", r && r[0] == 5 && r[(1 << 20) - 1] == 5);
    free(r);
    /* a shared mapping grows with its object */
    int fd = object(8192);
    char *m = mmap(NULL, 8192, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    memcpy(m, "Pool", 4);
    char *g = ftruncate(fd, 65536) == 0 ? mremap(m, 8192, 65536, MREMAP_MAYMOVE) : MAP_FAILED;
    char t[4] = {0};
    check("K13 geteilte Abbildung wächst mit dem Objekt (wl_shm_pool resize)", g != MAP_FAILED && memcmp(g, "Pool", 4) == 0 && (g[60000] = 'Z', pread(fd, t, 1, 60000) == 1 && t[0] == 'Z'));
    if (g != MAP_FAILED) munmap(g, 65536);
    close(fd);
}

/* ---------------------------------------------------------------- K11/K12: wake-ups and timeouts */

static int drain_fd = -1;
static void *drain_later(void *arg) {
    (void)arg;
    usleep(200000);
    char b[65536];
    while (read(drain_fd, b, sizeof b) > 0) {}
    return NULL;
}

static void wake_checks(void) {
    int sv[2];
    socketpair(AF_UNIX, SOCK_STREAM, 0, sv);
    fcntl(sv[0], F_SETFL, O_NONBLOCK);
    fcntl(sv[1], F_SETFL, O_NONBLOCK);
    char b[4096] = {0};
    while (write(sv[0], b, sizeof b) > 0) {}
    struct pollfd pf = {sv[0], POLLOUT, 0};
    check("K11 voller Socket: kein POLLOUT", poll(&pf, 1, 0) == 0);
    drain_fd = sv[1];
    pthread_t t;
    pthread_create(&t, NULL, drain_later, NULL);
    long t0 = now_us();
    int r = poll(&pf, 1, 5000);
    long took = (now_us() - t0) / 1000;
    check("K11 POLLOUT weckt, sobald der Leser Platz schafft", r == 1 && (pf.revents & POLLOUT) && took < 2000);
    pthread_join(t, NULL);
    close(sv[0]);
    close(sv[1]);
    int p[2];
    check("K12 Rohr für die Wartezeiten", pipe(p) == 0);
    struct pollfd q = {p[0], POLLIN, 0};
    struct timespec ts = {0, 16000000};
    t0 = now_us();
    r = ppoll(&q, 1, &ts, NULL);
    took = now_us() - t0;
    check("K12 ppoll 16 ms (nur Nanosekunden) wartet 16 ms", r == 0 && took >= 15000 && took < 1000000);
    ts.tv_nsec = 1;
    t0 = now_us();
    r = ppoll(&q, 1, &ts, NULL);
    check("K12 ppoll 1 ns kehrt bald zurück", r == 0 && now_us() - t0 < 500000);
    fd_set rs;
    FD_ZERO(&rs);
    FD_SET(p[0], &rs);
    struct timeval tv = {0, 16000};
    t0 = now_us();
    r = select(p[0] + 1, &rs, NULL, NULL, &tv);
    took = now_us() - t0;
    check("K12 select 16000 µs wartet 16 ms", r == 0 && took >= 15000 && took < 1000000);
    close(p[0]);
    close(p[1]);
}

int main(void) {
    say("FENSTERTEST: Speicherobjekte, Deskriptoren, Wecken, mremap, Bildspeicher, Geräteknoten, Eingabe\n");
    memfd_checks();
    mremap_checks();
    rights_checks();
    wake_checks();
    fb_checks();
    devnode_checks();
    input_checks();
    say("FENSTERTEST: %d Fehler\n", fails);
    say("FENSTERTEST: %s\n", fails == 0 ? "ALLES OK" : "SIEHE OBEN");
    return fails != 0;
}
