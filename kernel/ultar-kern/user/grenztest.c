/* grenztest – resource groups of the Ultar kernel: limits, dynamic protection, no panic without memory.
 * One line per check ("ok" / "FEHLER"); the last line is "GRENZTEST: ALLES OK" when every check passed.
 *   grenztest            everything
 *   grenztest <section>  positiv | grenzen | angriffe | dynamik
 * Runs as a program started by the kernel (group `session`, every right). */
#include "grenztest.h"

int fails;
unsigned char block[1 << 20], back[1 << 20];
const char *bin_dir = "/bin";

void say(const char *fmt, ...) {
    char b[512];
    va_list ap;
    va_start(ap, fmt);
    int n = vsnprintf(b, sizeof b, fmt, ap);
    va_end(ap);
    if (n > (int)sizeof b - 1) n = sizeof b - 1;
    if (n > 0 && write(1, b, n) < 0) fails++;
}

const char *pf(const char *fmt, ...) {
    static char bufs[4][256];
    static int next;
    char *b = bufs[next++ & 3];
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(b, sizeof bufs[0], fmt, ap);
    va_end(ap);
    return b;
}

void check(const char *name, int ok) {
    if (!ok) fails++;
    say("%s %s\n", ok ? "ok    " : "FEHLER", name);
}

void denied(const char *name, long r, int want) {
    int e = errno;
    int ok = r == -1 && e == want;
    if (!ok) fails++;
    say("%s %s -> %s\n", ok ? "ok    " : "FEHLER", name, r == -1 ? strerror(e) : "angenommen");
}

static int slurp(const char *path, char *buf, size_t size) {
    int fd = open(path, O_RDONLY);
    if (fd < 0) return -1;
    size_t n = 0;
    for (;;) {
        ssize_t r = read(fd, buf + n, size - 1 - n);
        if (r <= 0) break;
        n += r;
    }
    close(fd);
    buf[n] = 0;
    return (int)n;
}

long num_at(const char *path) {
    char b[128];
    if (slurp(path, b, sizeof b) < 0) return -1;
    if (!strncmp(b, "max", 3)) return -2;
    return strtol(b, NULL, 10);
}

long keyed_num(const char *path, const char *key) {
    static char b[16384];
    if (slurp(path, b, sizeof b) < 0) return -1;
    size_t k = strlen(key);
    for (char *p = b; (p = strstr(p, key)); p += k) {
        if ((p == b || p[-1] == '\n' || p[-1] == ' ') && p[k] == ' ') return strtol(p + k, NULL, 10);
    }
    return -1;
}

int put_text(const char *path, const char *text) {
    int fd = open(path, O_WRONLY);
    if (fd < 0) return -errno;
    ssize_t r = write(fd, text, strlen(text));
    int e = errno;
    close(fd);
    return r < 0 ? -e : 0;
}

long free_kb(void) {
    return keyed_num("/proc/meminfo", "MemFree:");
}

long now_ms(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return t.tv_sec * 1000 + t.tv_nsec / 1000000;
}

void nap_ms(long ms) {
    struct timespec t = {ms / 1000, ms % 1000 * 1000000};
    nanosleep(&t, NULL);
}

char *eat(long mb) {
    char *p = mmap(NULL, mb * MB, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (p == MAP_FAILED) return NULL;
    for (long i = 0; i < mb * MB; i += 4096) p[i] = (char)(i >> 12) | 1;
    return p;
}

int wait_ms(pid_t pid, int *status, long ms) {
    long end = now_ms() + ms;
    for (;;) {
        pid_t r = waitpid(pid, status, WNOHANG);
        if (r == pid) return 1;
        if (r < 0 || now_ms() > end) return 0;
        nap_ms(10);
    }
}

pid_t cell(int (*fn)(long), long arg) {
    pid_t c = syscall(SYS_clone, CLONE_NEWNS | CLONE_NEWPID | SIGCHLD, 0, 0, 0, 0);
    if (c == 0) _exit(fn(arg));
    return c;
}

int grab_mb(int fd) {
    return mmap(NULL, MB, PROT_READ | PROT_WRITE, MAP_PRIVATE, fd, 0) != MAP_FAILED;
}

int enter_root(const char *small) {
    mkdir("/tmp", 0755);
    int ok = mount("tmpfs", "/tmp", "tmpfs", 0, NULL) == 0;
    ok = ok && mkdir("/tmp/w", 0755) == 0 && mount("tmpfs", "/tmp/w", "tmpfs", 0, NULL) == 0;
    ok = ok && mkdir("/tmp/w/alt", 0755) == 0 && mkdir("/tmp/w/bin", 0755) == 0 && mkdir("/tmp/w/proc", 0755) == 0 && mkdir("/tmp/w/dev", 0755) == 0 && mkdir("/tmp/w/tmp", 0755) == 0;
    ok = ok && mount(bin_dir, "/tmp/w/bin", NULL, MS_BIND | MS_RDONLY, NULL) == 0;
    if (small) ok = ok && mkdir("/tmp/w/klein", 0755) == 0 && mount("tmpfs", "/tmp/w/klein", "tmpfs", 0, small) == 0;
    ok = ok && syscall(SYS_pivot_root, "/tmp/w", "/tmp/w/alt") == 0 && chdir("/") == 0;
    ok = ok && umount2("/alt", MNT_DETACH) == 0;
    ok = ok && mount("proc", "/proc", "proc", 0, NULL) == 0 && mount("dev", "/dev", "devtmpfs", 0, NULL) == 0;
    /* every right, including ones this program does not know yet */
    prctl(PR_ULTAR_RIGHTS_DROP, ~0UL, 0, 0, 0);
    return ok && prctl(PR_ULTAR_RIGHTS_GET, 0, 0, 0, 0) == 0;
}

long group_mem(const char *group) {
    return num_at(pf(CG "%s/memory.current", group));
}

int settled(long base) {
    long end = now_ms() + 3000;
    for (;;) {
        long cells = group_mem("/cells"), left = keyed_num("/proc/ultar/gruppen", "reste");
        if (cells == base && left == 0) return 1;
        if (now_ms() > end) {
            say("       (cells: %ld statt %ld Bytes, entfernte Gruppen mit Rest: %ld)\n", cells, base, left);
            return 0;
        }
        nap_ms(20);
    }
}

/* bytes charged to the kernel itself: everything minus the four branches */
static long kernel_own(void) {
    return group_mem("") - group_mem("/system") - group_mem("/session") - group_mem("/apps") - group_mem("/cells");
}

int normal_program_runs(void) {
    pid_t c = fork();
    if (c == 0) {
        int nul = open("/dev/null", O_WRONLY);
        if (nul >= 0) dup2(nul, 1);
        execl(pf("%s/hallo-glibc", bin_dir), "hallo-glibc", (char *)NULL);
        _exit(127);
    }
    int st = -1;
    return c > 0 && wait_ms(c, &st, 30000) && WIFEXITED(st) && WEXITSTATUS(st) == 0;
}

/* ------------------------------------------------------------------ normal work is not disturbed */

static void memory_in_bulk(void) {
    long before = free_kb();
    long mb = before * 40 / 100 / 1024;
    if (mb > 512) mb = 512;
    long t0 = now_ms();
    char *p = eat(mb);
    int ok = p != NULL;
    for (long i = 0; ok && i < mb * MB; i += 997 * 4096) ok = p[i] == (char)((char)(i >> 12) | 1);
    long held = free_kb();
    if (p) munmap(p, mb * MB);
    long after = free_kb();
    say("       (%ld MiB belegt und geprüft in %ld ms; frei vorher %ld, belegt %ld, danach %ld KiB)\n", mb, now_ms() - t0, before, held, after);
    check("Normal: grosser Speicherblock belegt, geprüft, freigegeben", ok && mb >= 64);
    check("Normal: belegter Speicher fehlte im freien Speicher und ist danach wieder frei", before - held >= mb * 1024 * 9 / 10 && after >= before - 4096);
}

static void many_processes(void) {
    int ok = 1;
    for (int i = 0; i < 200 && ok; i++) {
        pid_t c = fork();
        if (c == 0) _exit(i & 0x7f);
        int st = -1;
        ok = c > 0 && waitpid(c, &st, 0) == c && WIFEXITED(st) && WEXITSTATUS(st) == (i & 0x7f);
    }
    check("Normal: 200 kurzlebige Prozesse nacheinander", ok);
    for (int round = 0; round < 3 && ok; round++) {
        pid_t kids[40];
        for (int i = 0; i < 40; i++) {
            kids[i] = fork();
            if (kids[i] == 0) {
                char *p = eat(1);
                _exit(p ? 7 : 1);
            }
            ok = ok && kids[i] > 0;
        }
        for (int i = 0; i < 40; i++) {
            int st = -1;
            ok = waitpid(kids[i], &st, 0) == kids[i] && WIFEXITED(st) && WEXITSTATUS(st) == 7 && ok;
        }
    }
    check("Normal: 3 × 40 Prozesse gleichzeitig, jeder belegt 1 MiB", ok);
}

/* a reader child counts what arrives on `fd` and reports the byte count on `report`
 * (the content is checked by content_survives) */
static void reader(int fd, int report) {
    static unsigned char buf[65536];
    unsigned long sum[2] = {0, 0};
    ssize_t r;
    while ((r = read(fd, buf, sizeof buf)) > 0) sum[0] += r;
    _exit(write(report, sum, sizeof sum) == sizeof sum ? 0 : 1);
}

/* send `mb` MiB through `out` to a reader on `in` with blocking writes of `chunk` bytes */
static int transfer(int in, int out, long mb, size_t chunk) {
    int rep[2];
    if (pipe(rep)) return 0;
    for (size_t i = 0; i < sizeof block; i++) block[i] = (unsigned char)(i * 31 + (i >> 9));
    pid_t c = fork();
    if (c == 0) {
        close(out);
        reader(in, rep[1]);
    }
    close(in);
    unsigned long want[2] = {0, 0}, got[2] = {0, 0};
    int ok = 1;
    for (long sent = 0; sent < mb * MB && ok; sent += chunk) {
        ok = write(out, block, chunk) == (ssize_t)chunk;
        want[0] += chunk;
    }
    close(out);
    int st = -1;
    ok = ok && read(rep[0], got, sizeof got) == sizeof got && waitpid(c, &st, 0) == c && st == 0;
    close(rep[0]);
    close(rep[1]);
    return ok && got[0] == want[0];
}

/* content check: a pattern goes through a pipe in odd-sized pieces and must come back unchanged */
static int content_survives(void) {
    int p[2];
    if (pipe(p)) return 0;
    pid_t c = fork();
    if (c == 0) {
        close(p[0]);
        unsigned char b[7001];
        unsigned v = 0;
        for (int k = 0; k < 300; k++) {
            for (size_t i = 0; i < sizeof b; i++) b[i] = (unsigned char)(v++ * 7 + 3);
            if (write(p[1], b, sizeof b) != sizeof b) _exit(1);
        }
        _exit(0);
    }
    close(p[1]);
    unsigned char b[4099];
    unsigned v = 0;
    long total = 0;
    int ok = 1;
    ssize_t r;
    while ((r = read(p[0], b, sizeof b)) > 0) {
        for (ssize_t i = 0; i < r; i++) ok = ok && b[i] == (unsigned char)(v++ * 7 + 3);
        total += r;
    }
    close(p[0]);
    int st = -1;
    waitpid(c, &st, 0);
    return ok && st == 0 && total == 300L * 7001;
}

static void pipe_rules(void) {
    int p[2];
    check("Rohr: anlegen, Grösse 64 KiB", pipe2(p, O_NONBLOCK) == 0 && fcntl(p[1], F_GETPIPE_SZ) == 65536);
    long n = 0;
    ssize_t r;
    while ((r = write(p[1], block, 4096)) > 0) n += r;
    check("Rohr: ohne Leser nimmt es genau 64 KiB, dann EAGAIN", n == 65536 && r == -1 && errno == EAGAIN);
    struct pollfd pfd = {.fd = p[1], .events = POLLOUT};
    check("Rohr: voll -> nicht schreibbereit", poll(&pfd, 1, 0) == 0);
    check("Rohr: 100 Bytes gelesen -> 200 Bytes passen nicht (unteilbar bis 4096)", read(p[0], block, 100) == 100 && write(p[1], block, 200) == -1 && errno == EAGAIN);
    check("Rohr: 4 KiB gelesen -> wieder schreibbereit, 4096 Bytes passen", read(p[0], block, 4096) == 4096 && poll(&pfd, 1, 0) == 1 && write(p[1], block, 4096) == 4096);
    check("Rohr: grosser Schreibversuch nimmt nur den Rest (kurzes Ergebnis)", read(p[0], block, 30000) == 30000 && write(p[1], block, sizeof block) == 30000 + 100);
    close(p[0]);
    close(p[1]);
    check("Rohr: F_SETPIPE_SZ 1 MiB, füllt sich bis 1 MiB", pipe2(p, O_NONBLOCK) == 0 && fcntl(p[1], F_SETPIPE_SZ, 1 << 20) == 1 << 20 && write(p[1], block, sizeof block) == sizeof block && write(p[1], block, 1) == -1);
    check("Rohr: verkleinern unter den Inhalt -> EBUSY", fcntl(p[1], F_SETPIPE_SZ, 4096) == -1 && errno == EBUSY);
    check("Rohr: Höchstgrösse lesbar (/proc/sys/fs/pipe-max-size = 1 MiB)", num_at("/proc/sys/fs/pipe-max-size") == 1 << 20);
    close(p[0]);
    close(p[1]);
    check("Normal: 300 × 7001 Bytes in ungeraden Stücken kommen unverändert an", content_survives());
    check("Normal: 24 MiB durch ein Rohr mit Leser (1-MiB-Schreibaufrufe, blockierend)", pipe(p) == 0 && transfer(p[0], p[1], 24, 1 << 20));
    int sv[2];
    check("Normal: 8 MiB durch ein Unix-Socketpaar mit Leser", socketpair(AF_UNIX, SOCK_STREAM, 0, sv) == 0 && transfer(sv[0], sv[1], 8, 256 << 10));
}

static int file_ok(const char *path, long mb, int seed) {
    int fd = open(path, O_WRONLY | O_CREAT | O_TRUNC, 0644);
    if (fd < 0) return 0;
    int ok = 1;
    for (long i = 0; i < mb && ok; i++) {
        memset(block, seed + (int)i, sizeof block);
        ok = write(fd, block, sizeof block) == sizeof block;
    }
    ok = close(fd) == 0 && ok;
    fd = open(path, O_RDONLY);
    for (long i = 0; i < mb && ok; i++) {
        memset(block, seed + (int)i, sizeof block);
        ok = read(fd, back, sizeof back) == sizeof back && !memcmp(block, back, sizeof block);
    }
    if (fd >= 0) close(fd);
    return ok;
}

/* cell: a tmpfs with a quota of 4 MiB and one with the default quota */
static int tmpfs_cell(long unused) {
    (void)unused;
    mkdir("/tmp", 0755);
    check("tmpfs: size=abc wird abgelehnt", mount("tmpfs", "/tmp", "tmpfs", 0, "size=abc") == -1 && errno == EINVAL);
    check("tmpfs: einhängen mit size=4m", mount("tmpfs", "/tmp", "tmpfs", 0, "mode=755,size=4m") == 0);
    check("tmpfs: 3 MiB innerhalb des Kontingents, Inhalt stimmt", file_ok("/tmp/a", 3, 11));
    int fd = open("/tmp/b", O_WRONLY | O_CREAT, 0644);
    ssize_t r1 = write(fd, block, sizeof block - 65536), r2 = write(fd, block, sizeof block);
    int e = errno;
    check("tmpfs: über das Kontingent hinaus -> ENOSPC beim Schreiben", r1 == (ssize_t)sizeof block - 65536 && r2 == -1 && e == ENOSPC);
    close(fd);
    check("tmpfs: nach dem Löschen ist wieder Platz", unlink("/tmp/a") == 0 && unlink("/tmp/b") == 0 && file_ok("/tmp/c", 3, 42));
    int made = 0;
    while (made < 100000 && mkdir(pf("/tmp/d%d", made), 0755) == 0) made++;
    e = errno;
    say("       (%d leere Verzeichnisse passten noch hinein)\n", made);
    check("tmpfs: auch leere Einträge zählen -> ENOSPC", made > 100 && made < 100000 && e == ENOSPC);
    check("tmpfs: Verknüpfung in vollem Baum -> ENOSPC", symlink("ziel", "/tmp/link") == -1 && errno == ENOSPC);
    int ok = umount("/tmp") == 0 && mount("tmpfs", "/tmp", "tmpfs", 0, NULL) == 0;
    for (int i = 0; i < 16 && ok; i++) ok = file_ok(pf("/tmp/f%d", i), 1, i);
    check("tmpfs: Standard-Kontingent – 16 Dateien zu 1 MiB, Inhalt stimmt", ok);
    return fails ? 1 : 0;
}

void positive_tests(void) {
    say("== Normale Arbeit wird nicht gestört\n");
    /* created here, by the host: a directory a cell creates in the host's tree would stay charged to it */
    mkdir("/tmp", 0755);
    long cells = group_mem("/cells");
    memory_in_bulk();
    many_processes();
    pipe_rules();
    int st = -1;
    pid_t c = cell(tmpfs_cell, 0);
    check("tmpfs: Zelle mit zwei tmpfs lief durch", c > 0 && waitpid(c, &st, 0) == c && WIFEXITED(st));
    if (WIFEXITED(st) && WEXITSTATUS(st)) fails++;
    mkdir("/tmp", 0755);
    check("Normal: 4 MiB in der RAM-Schicht des Wirts (/tmp), Inhalt stimmt, gelöscht", file_ok("/tmp/grenz-wirt", 4, 5) && unlink("/tmp/grenz-wirt") == 0);
    check("Abrechnung: nach der Zelle ist `cells` wieder beim alten Stand, kein Rest", settled(cells));
}

int main(int argc, char **argv) {
    const char *only = argc > 1 ? argv[1] : "";
    if (access("/bin/hallo-glibc", F_OK) != 0) bin_dir = "/boot/efi/bin";
    long kernel_before = kernel_own(), free_before = free_kb();
    say("GRENZTEST: Gruppen, Grenzen, Speichermangel (frei %ld KiB)\n", free_before);
    check("Vorbereitung: alle Rechte, Gruppenbaum lesbar", (unsigned long)prctl(PR_ULTAR_RIGHTS_GET, 0, 0, 0, 0) == R_ALL && num_at(CG "/memory.current") > 0 && num_at(CG "/session/pids.current") >= 1);
    if (!*only || !strcmp(only, "positiv")) positive_tests();
    if (!*only || !strcmp(only, "grenzen")) limit_tests();
    if (!*only || !strcmp(only, "angriffe")) attack_tests();
    if (!*only || !strcmp(only, "dynamik")) dynamic_tests();
    long bad = keyed_num("/proc/ultar/gruppen", "fehlfreigaben"), emergency = keyed_num("/proc/ultar/gruppen", "benutzt");
    say("       (Notvorrat benutzt: %ld, vom Kern beendet: %ld)\n", emergency, keyed_num("/proc/ultar/gruppen", "beendet"));
    check("Kern: keine fehlerhafte Freigabe im Seitenverwalter", bad == 0);
    /* what the kernel keeps for itself (root minus the four branches; this program is in `session`):
     * the tests – thousands of processes, files, pipes, cells – must not have left anything there */
    long kernel_after = kernel_own();
    say("       (Kern selbst: vorher %ld, jetzt %ld Bytes; apps %ld, cells %ld; frei vorher %ld, jetzt %ld KiB)\n", kernel_before, kernel_after, group_mem("/apps"), group_mem("/cells"), free_before, free_kb());
    check("Abrechnung: der Kern selbst hält am Ende nicht mehr als am Anfang (± 64 KiB), apps und cells sind leer", kernel_after <= kernel_before + 65536 && group_mem("/apps") == 0 && group_mem("/cells") == 0);
    say("GRENZTEST: %d Fehler\n", fails);
    say("GRENZTEST: %s\n", fails == 0 ? "ALLES OK" : "SIEHE OBEN");
    return fails ? 1 : 0;
}
