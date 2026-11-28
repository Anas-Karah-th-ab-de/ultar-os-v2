/* Escape tests for the Ultar kernel's process isolation, stage 1 (mount + pid namespaces, rights).
 *
 * A cell (new mount + pid namespace, own root via pivot_root) tries to get out: read files outside its
 * root ("..", symbolic links, directory descriptors), see or signal processes outside, change the host
 * tree by mounting, regain dropped rights, reach /proc entries of the parent. Every attempt must fail
 * with the stated error; normal work inside the cell must succeed. One line per check;
 * "ZELLTEST: ALLES OK" only if every check passed. */
#include "zelltest.h"

int fails;
const char *bin_dir = "/bin", *outside_path = "/liesmich.txt";
int outside_dir = -1, outside_file = -1, host_mnt_ns = -1;
pid_t host_parent, host_bystander;
int to_parent[2], to_cell[2];

void say(const char *fmt, ...) {
    char b[256];
    va_list ap;
    va_start(ap, fmt);
    int n = vsnprintf(b, sizeof b, fmt, ap);
    va_end(ap);
    if (n > (int)sizeof b - 1) n = sizeof b - 1;
    if (write(1, b, n) < 0) fails++;
}

void check(const char *name, int ok) {
    say("%s %s\n", ok ? "ok    " : "FEHLER", name);
    if (!ok) fails++;
}

void denied(const char *name, long r, int want) {
    int e = errno;
    if (r == -1 && e == want) {
        say("ok     %s -> %s\n", name, strerror(e));
    } else {
        say("FEHLER %s: r=%ld errno=%d, erwartet errno=%d\n", name, r, r == -1 ? e : 0, want);
        fails++;
    }
}

int put_file(const char *path, const char *text) {
    int fd = open(path, O_WRONLY | O_CREAT | O_TRUNC, 0644);
    if (fd < 0) return -1;
    int ok = write(fd, text, strlen(text)) == (ssize_t)strlen(text);
    return close(fd) == 0 && ok ? 0 : -1;
}

int file_is(const char *path, const char *text) {
    char b[64] = {0};
    int fd = open(path, O_RDONLY);
    if (fd < 0) return 0;
    int n = read(fd, b, sizeof b - 1);
    close(fd);
    return n == (int)strlen(text) && !strcmp(b, text);
}

void sub(int (*fn)(void)) {
    pid_t c = fork();
    if (c == 0) {
        fails = 0;
        int r = fn();
        _exit(fails + r > 100 ? 100 : fails + r);
    }
    int st = 0;
    if (c < 0 || waitpid(c, &st, 0) != c || !WIFEXITED(st)) fails++;
    else fails += WEXITSTATUS(st);
}

const char *pf(const char *fmt, ...) {
    static char bufs[4][160];
    static int next;
    char *b = bufs[next++ & 3];
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(b, sizeof bufs[0], fmt, ap);
    va_end(ap);
    return b;
}

int idle(void) {
    for (;;) pause();
    return 0;
}

long rights(void) { return prctl(PR_ULTAR_RIGHTS_GET, 0, 0, 0, 0); }

/* ---------------------------------------------------------------- host side */

static int ns_name(const char *path, char *out, int size) {
    int n = readlink(path, out, size - 1);
    if (n < 0) return -1;
    out[n] = 0;
    return 0;
}

static pid_t child_of(pid_t parent) {
    DIR *d = opendir("/proc");
    struct dirent *e;
    pid_t found = 0;
    char p[300], b[256], want[32];
    snprintf(want, sizeof want, "PPid:\t%d\n", parent);
    while (d && (e = readdir(d))) {
        if (e->d_name[0] < '0' || e->d_name[0] > '9') continue;
        snprintf(p, sizeof p, "/proc/%s/status", e->d_name);
        int fd = open(p, O_RDONLY);
        if (fd < 0) continue;
        int n = read(fd, b, sizeof b - 1);
        close(fd);
        b[n > 0 ? n : 0] = 0;
        if (strstr(b, want) && strstr(b, "Name:\tzelltest")) found = atoi(e->d_name);
    }
    if (d) closedir(d);
    return found;
}

static pid_t cell_pid;

/* a host process with the right may enter the cell's mount namespace */
static int enter_cell(void) {
    char p[64];
    snprintf(p, sizeof p, "/proc/%d/ns/mnt", cell_pid);
    int fd = open(p, O_RDONLY);
    check("Wirt: setns in den Namensraum der Zelle", fd >= 0 && setns(fd, CLONE_NEWNS) == 0);
    check("Wirt: sieht danach den Baum der Zelle", access("/ordner", F_OK) == 0 && access(outside_path, F_OK) == -1);
    denied("Wirt: setns mit falscher Art", setns(fd, CLONE_NEWPID), EINVAL);
    return 0;
}

static int host_rights(void) {
    check("Wirt: mount im Ausgangs-Namensraum wie bisher angenommen", mount("proc", "/proc", "proc", 0, NULL) == 0);
    check("Wirt: Brandmauer lesbar", access("/proc/ultar/firewall", R_OK) == 0);
    check("Rechte: NET_ADMIN und MOUNT abgeben", prctl(PR_ULTAR_RIGHTS_DROP, R_NET_ADMIN | R_MOUNT, 0, 0, 0) == 0 && rights() == (R_ALL & ~(R_NET_ADMIN | R_MOUNT)));
    denied("Rechte: Brandmauer schreiben ohne NET_ADMIN", open("/proc/ultar/firewall", O_WRONLY), EACCES);
    denied("Rechte: mount im Ausgangs-Namensraum ohne Recht", mount("proc", "/proc", "proc", 0, NULL), EPERM);
    return 0;
}

static int first_in_new_pid_ns(void) { return getpid() == 1 ? 0 : 1; }

/* unshare(CLONE_NEWPID): the caller keeps its pid, its next child is pid 1; chroot narrows the root */
static int host_unshare(void) {
    pid_t me = getpid();
    char b[64];
    check("Wirt: unshare(CLONE_NEWPID), eigene Nummer bleibt", unshare(CLONE_NEWPID) == 0 && getpid() == me);
    sub(first_in_new_pid_ns);
    denied("Wirt: zweites unshare(CLONE_NEWPID)", unshare(CLONE_NEWPID), EINVAL);
    denied("Wirt: UTS-Namensraum gibt es nicht (Netz: zellnetz)", unshare(CLONE_NEWUTS), EINVAL);
    check("Wirt: chroot in den Programm-Ordner", chroot(bin_dir) == 0 && getcwd(b, sizeof b) && !strcmp(b, "/"));
    denied("Ausbruch chroot: /../liesmich.txt", open("/../liesmich.txt", O_RDONLY), ENOENT);
    denied("Ausbruch chroot: Pfad der Datei ausserhalb", open(outside_path, O_RDONLY), ENOENT);
    check("Wirt: im chroot liegt /hallo", access("/hallo", X_OK) == 0);
    denied("Ausbruch chroot: geerbter Verzeichnis-fd", openat(outside_dir, "kerntest", O_RDONLY), EXDEV);
    return 0;
}

static void wait_gone(const char *name, pid_t pid) {
    struct timespec ts = {0, 10 * 1000 * 1000};
    int gone = 0;
    for (int i = 0; i < 300 && !gone; i++) {
        gone = kill(pid, 0) == -1 && errno == ESRCH;
        if (!gone) nanosleep(&ts, NULL);
    }
    check(name, gone);
}

/* the private trees of namespaces must not show up in the host's root listing */
static int root_is_clean(void) {
    DIR *d = opendir("/");
    struct dirent *e;
    int clean = d != NULL;
    while (d && (e = readdir(d))) {
        if (e->d_name[0] == 0 || strstr(e->d_name, "tmpfs")) clean = 0;
    }
    if (d) closedir(d);
    return clean;
}

/* several cells at once: namespaces must not leak into each other */
static void host_parallel_cells(void) {
    pid_t c[6];
    int good = 0, st;
    for (int i = 0; i < 6; i++) {
        c[i] = syscall(SYS_clone, CLONE_NEWNS | CLONE_NEWPID | SIGCHLD, 0, 0, 0, 0);
        if (c[i] == 0) _exit(mini_cell(i));
    }
    for (int i = 0; i < 6; i++) {
        st = -1;
        if (c[i] > 1 && waitpid(c[i], &st, 0) == c[i] && WIFEXITED(st) && WEXITSTATUS(st) == 0) good++;
    }
    check("Wirt: sechs Zellen gleichzeitig, jede sieht nur sich", good == 6);
}

static void host_serve_cell(void) {
    char tag = 0, mine[64], theirs[64], p[64];
    pid_t idle_child = 0;
    while (read(to_parent[0], &tag, 1) == 1) {
        if (tag == 'm') check("Wirt: /bin unberührt, während die Zelle es überdeckt", access(pf("%s/kerntest", bin_dir), F_OK) == 0);
        if (tag == 'p') {
            snprintf(p, sizeof p, "/proc/%d/ns/pid", cell_pid);
            check("Wirt: sieht die Zelle unter ihrer echten Nummer", cell_pid > 1 && kill(cell_pid, 0) == 0 && ns_name(p, theirs, sizeof theirs) == 0);
            check("Wirt: Zelle hat einen anderen pid-Namensraum", ns_name("/proc/self/ns/pid", mine, sizeof mine) == 0 && strcmp(mine, theirs) && !strncmp(theirs, "pid:[", 5));
            idle_child = child_of(cell_pid);
            check("Wirt: sieht die Kinder der Zelle", idle_child > 1 && idle_child != cell_pid);
            sub(enter_cell);
            check("Wirt: private Bäume der Zelle erscheinen nicht in /", root_is_clean());
        }
        if (write(to_cell[1], &tag, 1) != 1) fails++;
        if (tag == 'p') break;
    }
    int st = 0;
    check("Wirt: Zelle beendet, alle Prüfungen in der Zelle bestanden", waitpid(cell_pid, &st, 0) == cell_pid && WIFEXITED(st) && WEXITSTATUS(st) == 0);
    if (idle_child > 1) wait_gone("Wirt: Ende von pid 1 beendet die übrigen Prozesse der Zelle", idle_child);
    else check("Wirt: Ende von pid 1 beendet die übrigen Prozesse der Zelle", 0);
}

static void host_after_cell(const char *mnt_before) {
    char now[64];
    struct stat st;
    int s = 0;
    check("Wirt: /bin/kerntest und /liesmich.txt unverändert", access(pf("%s/kerntest", bin_dir), F_OK) == 0 && access(pf("%s/hallo", bin_dir), F_OK) == 0 && file_is(SECRET, "GEHEIM") && access(outside_path, F_OK) == 0);
    check("Wirt: eigene Datei in /tmp unverändert", file_is(SECRET, "GEHEIM"));
    check("Wirt: Dateien der Zelle sind im Wirt nicht entstanden", lstat("/tmp/nur-zelle", &st) == -1 && lstat("/flucht", &st) == -1 && lstat("/daten.txt", &st) == -1 && lstat("/tmp/wurzel", &st) == -1);
    check("Wirt: Zuschauer-Prozess lebt (kill aus der Zelle traf ihn nicht)", kill(host_bystander, 0) == 0);
    check("Wirt: Zuschauer beenden", kill(host_bystander, SIGKILL) == 0 && waitpid(host_bystander, &s, 0) == host_bystander);
    check("Wirt: Rechte und Namensraum des Wirts unverändert", rights() == R_ALL && ns_name("/proc/self/ns/mnt", now, sizeof now) == 0 && !strcmp(now, mnt_before));
    unlink(SECRET);
}

/* ---------------------------------------------------------------- ext4 (extra run with a prepared disk)
 * Disk made by tools/kern-test/zell-ext4.sh: /etc/geheim, /zelle/{drin.txt, innen -> /drin.txt,
 * flucht -> /etc/geheim, flucht2 -> ../etc/geheim, ordner -> /etc}. ext4 follows links itself, relative to
 * the disk; inside a narrowed root the kernel must resolve them relative to that root instead. */
static int ext4_inside_root(void) {
    char b[64] = {0};
    struct stat st;
    check("ext4: chroot /zelle", chroot("/zelle") == 0);
    check("ext4: Verknüpfung innerhalb der Wurzel wird gefolgt", file_is("/innen", "drinnen\n"));
    denied("Ausbruch ext4: absolute Verknüpfung nach /etc/geheim", open("/flucht", O_RDONLY), ENOENT);
    denied("Ausbruch ext4: relative Verknüpfung mit ..", open("/flucht2", O_RDONLY), ENOENT);
    denied("Ausbruch ext4: Verzeichnis-Verknüpfung nach /etc", open("/ordner/geheim", O_RDONLY), ENOENT);
    denied("Ausbruch ext4: stat durch die Verknüpfung", stat("/flucht", &st), ENOENT);
    check("ext4: lstat und readlink zeigen die Verknüpfung selbst", lstat("/flucht", &st) == 0 && S_ISLNK(st.st_mode) && readlink("/flucht", b, sizeof b - 1) == 11 && !strcmp(b, "/etc/geheim"));
    return 0;
}

static int ext4_main(void) {
    say("ZELLTEST: ext4-Verknüpfungen\n");
    check("ext4: Wirt liest /etc/geheim über die Verknüpfung (Verhalten wie bisher)", file_is("/zelle/flucht", "GEHEIM-EXT4\n") && file_is("/zelle/ordner/geheim", "GEHEIM-EXT4\n"));
    sub(ext4_inside_root);
    say("ZELLTEST-EXT4: %s\n", fails == 0 ? "ALLES OK" : "FEHLER");
    return fails != 0;
}

int main(int argc, char **argv) {
    char mnt_before[64] = "";
    if (argc > 1 && !strcmp(argv[1], "--rechte")) return after_exec();
    if (argc > 1 && !strcmp(argv[1], "--ext4")) return ext4_main();
    if (access("/bin/kerntest", F_OK) != 0 && access("/boot/efi/bin/kerntest", F_OK) == 0) {
        bin_dir = "/boot/efi/bin"; /* a disk system is the root, the test programs are on the start partition */
        outside_path = "/boot/efi/liesmich.txt";
    }
    say("ZELLTEST: Ausbruchs-Prüfung (mount- und pid-Namensraum, Rechte)\n");
    mkdir("/tmp", 0755);
    host_parent = getpid();
    check("Wirt: Rechte = alle, Namensraum lesbar", rights() == R_ALL && ns_name("/proc/self/ns/mnt", mnt_before, sizeof mnt_before) == 0 && !strncmp(mnt_before, "mnt:[", 5));
    check("Wirt: Vorbereitung (Datei, Verzeichnis-fd, Namensraum-fd)", put_file(SECRET, "GEHEIM") == 0 && (outside_dir = open(bin_dir, O_RDONLY | O_DIRECTORY)) >= 0 && (outside_file = open(outside_path, O_RDONLY)) >= 0 && (host_mnt_ns = open("/proc/self/ns/mnt", O_RDONLY)) >= 0);
    check("Wirt: symlink ausserhalb von tmpfs wie bisher verweigert", symlink(outside_path, "/tmp/zell-link") == -1 && errno == EPERM);
    check("Wirt: Uhr stellen mit Recht: wie bisher nicht umgesetzt", settimeofday(&(struct timeval){1000, 0}, NULL) == -1 && errno == ENOSYS);
    host_bystander = fork();
    if (host_bystander == 0) _exit(idle());
    sub(host_rights);
    sub(host_unshare);
    if (pipe(to_parent) || pipe(to_cell)) return 1;
    cell_pid = syscall(SYS_clone, CLONE_NEWNS | CLONE_NEWPID | SIGCHLD, 0, 0, 0, 0);
    if (cell_pid == 0) {
        fails = 0;
        close(to_parent[0]);
        close(to_cell[1]);
        cell_main();
        _exit(fails > 100 ? 100 : fails);
    }
    close(to_parent[1]);
    close(to_cell[0]);
    check("Wirt: Zelle gestartet (clone mit CLONE_NEWNS|CLONE_NEWPID)", cell_pid > 1);
    host_serve_cell();
    host_parallel_cells();
    host_after_cell(mnt_before);
    say("ZELLTEST: %s\n", fails == 0 ? "ALLES OK" : "FEHLER");
    return fails != 0;
}
