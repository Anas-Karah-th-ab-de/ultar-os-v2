/* ausbruch – additional escape / privilege-regain attacks against the Ultar kernel's isolation stage 1.
 * Written by the security reviewer; same style as zelltest. One line per check:
 *   "ok"      attack failed as it must
 *   "BEFUND"  attack succeeded or behaviour is a defect (finding)
 * Modes:  ausbruch            main run (host side + cell)
 *         ausbruch --nachexec rights after execve
 *         ausbruch --dos <x>  one destructive test inside a rights-less cell (own boot each) */
#define _GNU_SOURCE
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <pthread.h>
#include <sched.h>
#include <signal.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <sys/mount.h>
#include <sys/prctl.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/syscall.h>
#include <sys/un.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#define PR_ULTAR_RIGHTS_GET 0x554c5200
#define PR_ULTAR_RIGHTS_DROP 0x554c5201
#define R_MANAGE_GROUPS 0x80
#define R_ALL 0x3ff
#define SECRET "/tmp/sec-geheim"
#define SECRET_TEXT "STRENG-GEHEIM"

static int fails, findings;
static int host_root_fd = -1, host_tmp_fd = -1, host_file_fd = -1, host_ns_fd = -1;
static pid_t host_pid, host_bystander, host_pgid;

static void say(const char *fmt, ...) {
    char b[320];
    va_list ap;
    va_start(ap, fmt);
    int n = vsnprintf(b, sizeof b, fmt, ap);
    va_end(ap);
    if (n > (int)sizeof b - 1) n = sizeof b - 1;
    if (write(1, b, n) < 0) fails++;
}
static void check(const char *name, int ok) {
    say("%s %s\n", ok ? "ok    " : "FEHLER", name);
    if (!ok) fails++;
}
/* vulnerable != 0: the attack worked */
static void finding(const char *name, int vulnerable) {
    say("%s %s\n", vulnerable ? "BEFUND" : "ok    ", name);
    if (vulnerable) findings++;
}
/* the call must fail (any error); success is a finding */
static void must_fail(const char *name, long r) {
    int e = errno;
    if (r < 0) say("ok     %s -> %s\n", name, strerror(e));
    else {
        say("BEFUND %s: gelang (r=%ld)\n", name, r);
        findings++;
    }
}
static long rights(void) { return prctl(PR_ULTAR_RIGHTS_GET, 0, 0, 0, 0); }
static int put_file(const char *path, const char *text) {
    int fd = open(path, O_WRONLY | O_CREAT | O_TRUNC, 0644);
    if (fd < 0) return -1;
    int ok = write(fd, text, strlen(text)) == (ssize_t)strlen(text);
    return close(fd) == 0 && ok ? 0 : -1;
}
/* 1 if the file can be read and holds the secret */
static int reads_secret(const char *path) {
    char b[64] = {0};
    int fd = open(path, O_RDONLY);
    if (fd < 0) return 0;
    int n = read(fd, b, sizeof b - 1);
    close(fd);
    return n > 0 && strstr(b, SECRET_TEXT) != NULL;
}
static void no_secret(const char *path) {
    char n[200];
    int got = reads_secret(path);
    snprintf(n, sizeof n, "Ausbruch Pfad: %s%s", path, got ? " LIEST DAS GEHEIMNIS" : "");
    finding(n, got);
}

/* build the cell root exactly as the design recipe says; keep_cwd: do not chdir afterwards */
static int enter_root(int keep_cwd, int with_proc_dev) {
    int ok = mount("tmpfs", "/tmp", "tmpfs", 0, NULL) == 0;
    ok = ok && mkdir("/tmp/w", 0755) == 0 && mount("tmpfs", "/tmp/w", "tmpfs", 0, NULL) == 0;
    ok = ok && mkdir("/tmp/w/alt", 0755) == 0 && mkdir("/tmp/w/bin", 0755) == 0 && mkdir("/tmp/w/proc", 0755) == 0 && mkdir("/tmp/w/dev", 0755) == 0 && mkdir("/tmp/w/tmp", 0755) == 0;
    ok = ok && mount("/bin", "/tmp/w/bin", NULL, MS_BIND | MS_RDONLY, NULL) == 0;
    ok = ok && syscall(SYS_pivot_root, "/tmp/w", "/tmp/w/alt") == 0;
    if (!keep_cwd) ok = ok && chdir("/") == 0;
    ok = ok && umount2("/alt", MNT_DETACH) == 0;
    if (with_proc_dev) ok = ok && mount("proc", "/proc", "proc", 0, NULL) == 0 && mount("dev", "/dev", "devtmpfs", 0, NULL) == 0;
    return ok;
}

/* ------------------------------------------------------------------ paths */
static void attack_paths(void) {
    static const char *p[] = {
        "/../../../tmp/sec-geheim", "//..//..//tmp//sec-geheim", "/./.././tmp/sec-geheim/", "/tmp/../../tmp/sec-geheim",
        "bin/../../tmp/sec-geheim", "../tmp/sec-geheim", "/..", "/alt/tmp/sec-geheim", "/proc/self/root/tmp/sec-geheim",
        "/proc/self/cwd/tmp/sec-geheim", "/proc/1/root/tmp/sec-geheim", "/proc/1/cwd/../tmp/sec-geheim", "/proc/self/fd/3/tmp/sec-geheim",
        "/proc/thread-self/root/tmp/sec-geheim", "/proc/../tmp/sec-geheim", "/dev/../tmp/sec-geheim", "/bin/../tmp/sec-geheim",
        "/bin/../../../tmp/sec-geheim", "/tmp/sec-geheim", "tmp/sec-geheim", "/\\..\\..\\tmp\\sec-geheim", "/.../tmp/sec-geheim", "/.. /tmp/sec-geheim",
    };
    for (unsigned i = 0; i < sizeof p / sizeof *p; i++) no_secret(p[i]);
    /* symbolic link chains */
    mkdir("/l", 0755);
    mkdir("/l/a", 0755);
    mkdir("/l/a/b", 0755);
    symlink("/../../tmp/sec-geheim", "/l/s1");
    symlink("../../../../../tmp/sec-geheim", "/l/a/b/s2");
    symlink("s1", "/l/s3");
    symlink("/l/s3", "/l/s4");
    symlink("..", "/l/a/b/hoch");
    symlink("/proc/self/root", "/l/wurzel");
    symlink("/alt", "/l/alt");
    symlink("/l/a/b/hoch/hoch/hoch/hoch/hoch/tmp", "/l/kette");
    symlink("/bin/..", "/l/binhoch");
    symlink("/tmp/sec-geheim/", "/l/schraeg");
    no_secret("/l/s1");
    no_secret("/l/a/b/s2");
    no_secret("/l/s4");
    no_secret("/l/a/b/hoch/hoch/hoch/hoch/hoch/hoch/tmp/sec-geheim");
    no_secret("/l/wurzel/tmp/sec-geheim");
    no_secret("/l/alt/tmp/sec-geheim");
    no_secret("/l/kette/sec-geheim");
    no_secret("/l/binhoch/tmp/sec-geheim");
    no_secret("/l/binhoch/../tmp/sec-geheim");
    no_secret("/l/schraeg");
    /* a chain of 45 links must end in ELOOP, not in a crash */
    char a[64], b[64];
    symlink("/bin/hallo", "/l/k0");
    for (int i = 1; i <= 45; i++) {
        snprintf(a, sizeof a, "/l/k%d", i - 1);
        snprintf(b, sizeof b, "/l/k%d", i);
        symlink(a, b);
    }
    check("Verknüpfungskette 45 -> ELOOP", open("/l/k45", O_RDONLY) == -1 && errno == ELOOP);
    check("Verknüpfungskette 30 geht", access("/l/k30", F_OK) == 0);
    /* inherited directory descriptors of the host (root and /tmp) */
    struct stat st;
    char buf[64];
    must_fail("Ausbruch fd: openat(Wirt-/, tmp/sec-geheim)", openat(host_root_fd, "tmp/sec-geheim", O_RDONLY));
    must_fail("Ausbruch fd: openat(Wirt-/tmp, sec-geheim)", openat(host_tmp_fd, "sec-geheim", O_RDONLY));
    must_fail("Ausbruch fd: openat(Wirt-/tmp, .)", openat(host_tmp_fd, ".", O_RDONLY));
    must_fail("Ausbruch fd: openat(Wirt-/tmp, sec-geheim, O_PATH)", openat(host_tmp_fd, "sec-geheim", O_PATH));
    must_fail("Ausbruch fd: fstatat(Wirt-/tmp, sec-geheim)", fstatat(host_tmp_fd, "sec-geheim", &st, 0));
    must_fail("Ausbruch fd: faccessat(Wirt-/tmp, sec-geheim)", faccessat(host_tmp_fd, "sec-geheim", F_OK, 0));
    must_fail("Ausbruch fd: unlinkat(Wirt-/tmp, sec-geheim)", unlinkat(host_tmp_fd, "sec-geheim", 0));
    must_fail("Ausbruch fd: mkdirat(Wirt-/tmp, boese)", mkdirat(host_tmp_fd, "sec-boese-dir", 0755));
    must_fail("Ausbruch fd: readlinkat(Wirt-/tmp, sec-geheim)", readlinkat(host_tmp_fd, "sec-geheim", buf, sizeof buf));
    must_fail("Ausbruch fd: symlinkat(x, Wirt-/tmp, boese)", symlinkat("/x", host_tmp_fd, "sec-boese-link"));
    must_fail("Ausbruch fd: renameat(Wirt-/tmp/sec-geheim -> /beute)", renameat(host_tmp_fd, "sec-geheim", AT_FDCWD, "/beute"));
    put_file("/eigen", "zelle");
    must_fail("Ausbruch fd: renameat(/eigen -> Wirt-/tmp/sec-boese)", renameat(AT_FDCWD, "/eigen", host_tmp_fd, "sec-boese"));
    must_fail("Ausbruch fd: fchdir(Wirt-/tmp)", fchdir(host_tmp_fd));
    must_fail("Ausbruch fd: openat(Wirt-/tmp, ../tmp/sec-geheim)", openat(host_tmp_fd, "../tmp/sec-geheim", O_RDONLY));
    must_fail("Ausbruch fd: execveat gibt es nicht", syscall(SYS_execveat, host_root_fd, "bin/hallo", NULL, NULL, 0));
    must_fail("Ausbruch fd: openat2 gibt es nicht", syscall(437, host_tmp_fd, "sec-geheim", buf, 24));
    must_fail("Ausbruch fd: name_to_handle_at", syscall(SYS_name_to_handle_at, host_tmp_fd, "sec-geheim", buf, buf, 0));
    /* read-only bind */
    must_fail("Nur-lesen: Datei in /bin anlegen", open("/bin/sec-boese", O_WRONLY | O_CREAT, 0644));
    must_fail("Nur-lesen: /bin/hallo zum Schreiben öffnen", open("/bin/hallo", O_WRONLY));
    must_fail("Nur-lesen: /bin/hallo O_RDWR|O_TRUNC", open("/bin/hallo", O_RDWR | O_TRUNC));
    must_fail("Nur-lesen: /bin/hallo mit Zugriffsart 3", open("/bin/hallo", 3));
    must_fail("Nur-lesen: /bin/hallo O_WRONLY|O_APPEND", open("/bin/hallo", O_WRONLY | O_APPEND));
    must_fail("Nur-lesen: mkdir /bin/x", mkdir("/bin/sec-x", 0755));
    must_fail("Nur-lesen: unlink /bin/hallo", unlink("/bin/hallo"));
    must_fail("Nur-lesen: rename /bin/hallo -> /hallo", rename("/bin/hallo", "/hallo-beute"));
    must_fail("Nur-lesen: rename /eigen -> /bin/sec-boese", rename("/eigen", "/bin/sec-boese"));
    must_fail("Nur-lesen: symlink in /bin", symlink("/x", "/bin/sec-link"));
    must_fail("Nur-lesen: link /bin/hallo /h", link("/bin/hallo", "/h"));
    symlink("/bin/hallo", "/l/zuhallo");
    must_fail("Nur-lesen: Schreiben durch Verknüpfung auf /bin/hallo", open("/l/zuhallo", O_WRONLY | O_TRUNC));
    symlink("/bin/sec-neu", "/l/zuneu");
    must_fail("Nur-lesen: Anlegen durch Verknüpfung nach /bin/sec-neu", open("/l/zuneu", O_WRONLY | O_CREAT, 0644));
    must_fail("Nur-lesen: rename über Verknüpfungsordner", rename("/eigen", "/l/binhoch/bin/sec-boese2"));
}

/* ------------------------------------------------------------------ /proc and /dev */
static void attack_proc(void) {
    char p[64];
    int seen = 0;
    for (int pid = 2; pid <= 400; pid++) {
        if (pid == getpid()) continue;
        snprintf(p, sizeof p, "/proc/%d/status", pid);
        if (access(p, F_OK) == 0) seen++;
        snprintf(p, sizeof p, "/proc/%d/ns/mnt", pid);
        if (open(p, O_RDONLY) >= 0) seen++;
        snprintf(p, sizeof p, "/proc/%d/exe", pid);
        if (open(p, O_RDONLY) >= 0) seen++;
    }
    finding("Ausbruch /proc: Einträge fremder Prozesse (2..400) sichtbar", seen != 0);
    must_fail("Ausbruch /proc: /proc/ultar/protokoll", open("/proc/ultar/protokoll", O_RDONLY));
    must_fail("Ausbruch /proc: /proc/ultar/firewall schreiben", open("/proc/ultar/firewall", O_WRONLY | O_CREAT, 0644));
    must_fail("Ausbruch /proc: /proc/ultar anlegen", mkdir("/proc/ultar", 0755));
    must_fail("Ausbruch /proc: rename nach /proc/ultar/firewall", rename("/eigen2", "/proc/ultar/firewall"));
    /* files created below the proc/devtmpfs mounts go where? (checked again by the host afterwards) */
    int fd = open("/proc/sec-kanal", O_WRONLY | O_CREAT, 0644);
    if (fd >= 0) {
        if (write(fd, "AUS-DER-ZELLE", 13) != 13) fails++;
        close(fd);
    }
    finding("Zelle legt Datei unter /proc an (landet im Baum des Wirts?)", fd >= 0);
    fd = open("/dev/sec-kanal", O_WRONLY | O_CREAT, 0644);
    if (fd >= 0) {
        if (write(fd, "AUS-DER-ZELLE", 13) != 13) fails++;
        close(fd);
    }
    finding("Zelle legt Datei unter /dev an (landet im Baum des Wirts?)", fd >= 0);
    finding("Zelle legt Verzeichnis unter /dev an", mkdir("/dev/sec-ordner", 0755) == 0);
    int pg = -1;
    int r = ioctl(0, TIOCGPGRP, &pg);
    say("       (TIOCGPGRP: r=%d pgrp=%d, getpgrp()=%d, echte Gruppe des Wirts=%d)\n", r, pg, getpgrp(), host_pgid);
    finding("Leck: TIOCGPGRP zeigt die echte (Wirts-)Prozessgruppe", r == 0 && pg == host_pgid && pg != 0);
    char b[256] = {0};
    fd = open("/proc/meminfo", O_RDONLY);
    r = fd >= 0 ? read(fd, b, sizeof b - 1) : -1;
    finding("Leck (gering): /proc/meminfo zeigt den Speicher der ganzen Maschine", r > 0);
}

/* ------------------------------------------------------------------ pids and signals */
static void attack_pids(void) {
    check("Zelle: pid 1, Elternprozess unsichtbar", getpid() == 1 && getppid() == 0);
    int hit = 0;
    pid_t real[] = {host_pid, host_bystander, host_pgid};
    for (unsigned i = 0; i < 3; i++) {
        if (kill(real[i], 0) == 0 && real[i] != 1) hit++;
        if (kill(-real[i], 0) == 0) hit++;
        if (syscall(SYS_tkill, real[i], 0) == 0 && real[i] != 1) hit++;
        if (syscall(SYS_tgkill, real[i], real[i], 0) == 0 && real[i] != 1) hit++;
        if (waitpid(real[i], NULL, WNOHANG) >= 0 && real[i] != 1) hit++;
    }
    finding("Ausbruch pid: echte Nummern des Wirts erreichbar (kill/tkill/tgkill/wait)", hit != 0);
    must_fail("Ausbruch pid: kill(INT_MIN)", kill(INT_MIN, SIGKILL));
    must_fail("Ausbruch pid: kill(-2147483647)", kill(-2147483647, SIGKILL));
    must_fail("Ausbruch pid: waitpid(-1) ohne Kinder", waitpid(-1, NULL, WNOHANG));
    must_fail("Ausbruch pid: ptrace", syscall(SYS_ptrace, 16 /*ATTACH*/, host_bystander, 0, 0));
    must_fail("Ausbruch pid: process_vm_readv", syscall(SYS_process_vm_readv, host_bystander, 0, 0, 0, 0, 0));
    must_fail("Ausbruch pid: pidfd_open", syscall(434, host_bystander, 0));
    must_fail("Ausbruch pid: kcmp", syscall(SYS_kcmp, host_bystander, 1, 0, 0, 0));
    must_fail("Ausbruch pid: rt_sigqueueinfo", syscall(SYS_rt_sigqueueinfo, host_bystander, SIGKILL, 0));
    /* broadcast kills from a child of the cell: must stay inside */
    pid_t c = fork();
    if (c == 0) {
        kill(-1, SIGKILL);
        kill(0, SIGUSR2);
        _exit(0);
    }
    int st;
    waitpid(c, &st, 0);
    check("Zelle: pid 1 überlebt kill(-1) und kill(0) aus der Zelle", 1);
}

/* ------------------------------------------------------------------ rights */
static int thread_rights;
static void *thread_fn(void *x) {
    (void)x;
    thread_rights = rights();
    return NULL;
}
static int probe_pipe[2];
static int probe_child(void *x) {
    (void)x;
    long r = rights();
    /* try everything that could bring a right back */
    prctl(PR_ULTAR_RIGHTS_DROP, 0, 0, 0, 0);
    prctl(PR_ULTAR_RIGHTS_GET + 2, R_ALL, 0, 0, 0);
    prctl(PR_CAP_AMBIENT, PR_CAP_AMBIENT_RAISE, 21, 0, 0);
    prctl(PR_SET_KEEPCAPS, 1, 0, 0, 0);
    prctl(PR_SET_SECUREBITS, 0x2f, 0, 0, 0);
    syscall(SYS_setuid, 0);
    syscall(SYS_setresuid, 0, 0, 0);
    r |= rights();
    if (mount("tmpfs", "/tmp", "tmpfs", 0, NULL) == 0) r |= 0x1000;
    if (unshare(CLONE_NEWNS) == 0) r |= 0x2000;
    if (setns(host_ns_fd, 0) == 0) r |= 0x4000;
    if (chroot("/") == 0) r |= 0x8000;
    if (write(probe_pipe[1], &r, sizeof r) != sizeof r) syscall(SYS_exit, 1);
    syscall(SYS_exit, 0);
    return 0;
}
static int clone_child_rights(unsigned long flags) {
    static char stack[1 << 16];
    if (pipe(probe_pipe)) return -1;
    long c = clone(probe_child, stack + sizeof stack, flags, NULL);
    long r = -1;
    if (c < 0) return -2;
    if (!(flags & CLONE_VM)) close(probe_pipe[1]);
    struct timespec ts = {0, 200 * 1000 * 1000};
    if (flags & CLONE_VM) nanosleep(&ts, NULL);
    if (read(probe_pipe[0], &r, sizeof r) != sizeof r) r = -3;
    close(probe_pipe[0]);
    if (flags & CLONE_VM) close(probe_pipe[1]);
    if (!(flags & CLONE_THREAD)) waitpid(c, NULL, __WALL);
    return (int)r;
}

static void attack_rights(void) {
    /* a "thread" in the same process number but with its own state: CLONE_THREAD without CLONE_VM */
    int pfd[2], go[2];
    if (pipe(pfd) || pipe(go)) fails++;
    long t = syscall(SYS_clone, CLONE_THREAD | CLONE_SIGHAND, 0, 0, 0, 0);
    if (t == 0) {
        char x;
        long v[3];
        if (read(go[0], &x, 1) != 1) syscall(SYS_exit, 1);
        v[0] = rights();
        v[1] = getpid();
        v[2] = mount("tmpfs", "/tmp", "tmpfs", MS_RDONLY, NULL) == 0;
        if (v[2]) umount2("/tmp", 0);
        if (write(pfd[1], v, sizeof v) != sizeof v) syscall(SYS_exit, 1);
        syscall(SYS_exit, 0);
    }
    check("Rechte: alle abgeben", prctl(PR_ULTAR_RIGHTS_DROP, ~0UL, 0, 0, 0) == 0 && rights() == 0);
    if (t > 0) {
        long v[3] = {-1, -1, -1};
        if (write(go[1], "x", 1) != 1 || read(pfd[0], v, sizeof v) != sizeof v) fails++;
        say("       (Faden ohne CLONE_VM: Rechte=%#lx, getpid=%ld, mount gelang=%ld; Prozess selbst: Rechte=%#lx, getpid=%d)\n", v[0], v[1], v[2], rights(), getpid());
        finding("Rechte: zwei Fäden derselben Prozessnummer mit verschiedenen Rechten (CLONE_THREAD ohne CLONE_VM)", v[1] == getpid() && v[0] != rights());
    } else {
        say("       (clone(CLONE_THREAD ohne CLONE_VM) -> %s)\n", strerror(errno));
    }
    must_fail("Rechte: mount nach Abgabe", mount("tmpfs", "/tmp", "tmpfs", 0, NULL));
    must_fail("Rechte: setns in den geerbten Namensraum des Wirts", setns(host_ns_fd, 0));
    must_fail("Rechte: setns mit CLONE_NEWNS", setns(host_ns_fd, CLONE_NEWNS));
    must_fail("Rechte: unshare(CLONE_NEWPID)", unshare(CLONE_NEWPID));
    must_fail("Rechte: clone(CLONE_NEWNS)", syscall(SYS_clone, CLONE_NEWNS | SIGCHLD, 0, 0, 0, 0));
    must_fail("Rechte: clone(CLONE_NEWUSER)", syscall(SYS_clone, CLONE_NEWUSER | SIGCHLD, 0, 0, 0, 0));
    must_fail("Rechte: reboot", syscall(SYS_reboot, 0xfee1dead, 672274793, 0x01234567, 0));
    must_fail("Rechte: /dev/fb0", open("/dev/fb0", O_RDWR));
    must_fail("Rechte: /dev/input/event0", open("/dev/input/event0", O_RDWR));
    must_fail("Rechte: /dev/input/../input/event1", open("/dev/input/../input/event1", O_RDONLY));
    static const struct { const char *n; unsigned long f; } v[] = {
        {"fork", SIGCHLD}, {"vfork-artig (CLONE_VM|CLONE_VFORK)", CLONE_VM | CLONE_VFORK | SIGCHLD}, {"CLONE_FILES|CLONE_FS", CLONE_FILES | CLONE_FS | SIGCHLD},
        {"CLONE_PARENT", CLONE_PARENT | SIGCHLD}, {"CLONE_THREAD ohne VM", CLONE_THREAD | CLONE_SIGHAND}, {"CLONE_VM (Faden)", CLONE_VM | CLONE_THREAD | CLONE_SIGHAND},
    };
    for (unsigned i = 0; i < sizeof v / sizeof *v; i++) {
        char n[120];
        int r = clone_child_rights(v[i].f);
        snprintf(n, sizeof n, "Rechte zurück über clone(%s): Ergebnis %#x", v[i].n, r);
        finding(n, r != 0);
    }
    pthread_t th;
    thread_rights = -1;
    if (pthread_create(&th, NULL, thread_fn, NULL) == 0) pthread_join(th, NULL);
    finding("Rechte: neuer Faden hat Rechte", thread_rights != 0);
    /* descriptor passing: SCM_RIGHTS */
    int sv[2];
    if (socketpair(AF_UNIX, SOCK_STREAM, 0, sv) == 0) {
        char cbuf[CMSG_SPACE(sizeof(int))] = {0}, d = 'x';
        struct iovec io = {&d, 1};
        struct msghdr m = {0};
        m.msg_iov = &io;
        m.msg_iovlen = 1;
        m.msg_control = cbuf;
        m.msg_controllen = sizeof cbuf;
        struct cmsghdr *c = CMSG_FIRSTHDR(&m);
        c->cmsg_level = SOL_SOCKET;
        c->cmsg_type = SCM_RIGHTS;
        c->cmsg_len = CMSG_LEN(sizeof(int));
        memcpy(CMSG_DATA(c), &host_tmp_fd, sizeof(int));
        long s = sendmsg(sv[0], &m, 0);
        memset(cbuf, 0, sizeof cbuf);
        m.msg_controllen = sizeof cbuf;
        long r = recvmsg(sv[1], &m, 0);
        c = CMSG_FIRSTHDR(&m);
        say("       (SCM_RIGHTS: sendmsg=%ld recvmsg=%ld controllen=%lu)\n", s, r, (unsigned long)m.msg_controllen);
        finding("Rechte: Deskriptor über Unix-Socket weitergereicht (SCM_RIGHTS)", r >= 0 && m.msg_controllen >= CMSG_LEN(sizeof(int)) && c && c->cmsg_type == SCM_RIGHTS);
    }
    /* host sockets */
    struct sockaddr_un a = {.sun_family = AF_UNIX};
    int s = socket(AF_UNIX, SOCK_STREAM, 0);
    strcpy(a.sun_path, "/tmp/sec-sock");
    must_fail("Ausbruch Socket: Pfad-Socket des Wirts /tmp/sec-sock", connect(s, (struct sockaddr *)&a, sizeof a));
    memset(a.sun_path, 0, sizeof a.sun_path);
    strcpy(a.sun_path + 1, "sec-abstrakt");
    must_fail("Ausbruch Socket: abstrakter Socket des Wirts", connect(s, (struct sockaddr *)&a, sizeof(sa_family_t) + 1 + 12));
    close(s);
    /* execve keeps the empty set */
    pid_t c = fork();
    if (c == 0) {
        char *argv[] = {"/bin/ausbruch", "--nachexec", NULL};
        execve(argv[0], argv, NULL);
        _exit(99);
    }
    int st = -1;
    waitpid(c, &st, 0);
    finding("Rechte zurück über execve", !(WIFEXITED(st) && WEXITSTATUS(st) == 0));
}

static int after_exec(void) {
    int bad = rights() != 0;
    bad |= mount("tmpfs", "/tmp", "tmpfs", 0, NULL) == 0;
    bad |= chroot("/") == 0;
    bad |= unshare(CLONE_NEWNS) == 0;
    return bad;
}

/* limits while the cell still holds MOUNT */
static void limits_with_mount(void) {
    int n = 0, e = 0;
    mkdir("/m", 0755);
    for (; n < 400; n++) {
        if (mount("tmpfs", "/m", "tmpfs", 0, NULL) != 0) {
            e = errno;
            break;
        }
    }
    say("       (mount-Tabelle: %d weitere Einhängungen, dann %s)\n", n, e ? strerror(e) : "kein Fehler");
    check("Grenze: Einhänge-Tabelle endet mit ENOSPC", e == ENOSPC && n < 300);
    while (umount2("/m", 0) == 0) {}
    /* mount over /proc inside the cell must not change the host's /proc (host checks later) */
    check("Zelle: tmpfs über /proc und wieder weg", mount("tmpfs", "/proc", "tmpfs", 0, NULL) == 0 && access("/proc/self", F_OK) == -1 && umount2("/proc", 0) == 0 && access("/proc/self", F_OK) == 0);
    int depth = 0;
    /* nested pid namespaces: limit */
    pid_t c = fork();
    if (c == 0) {
        int d = 0;
        for (; d < 20; d++) {
            if (unshare(CLONE_NEWPID) != 0) break;
            pid_t k = fork();
            if (k < 0) break;
            if (k > 0) {
                int st;
                waitpid(k, &st, 0);
                _exit(WIFEXITED(st) ? WEXITSTATUS(st) : 77);
            }
        }
        _exit(d);
    }
    int st = 0;
    waitpid(c, &st, 0);
    depth = WIFEXITED(st) ? WEXITSTATUS(st) : -1;
    say("       (pid-Namensräume: Tiefe %d erreicht)\n", depth);
    check("Grenze: Verschachtelung der pid-Namensräume endet", depth > 0 && depth < 20);
}

static int cell_main(void) {
    check("Zelle: Wurzel wie im Rezept (tmpfs, /bin nur lesbar, pivot_root, alt abgehängt, proc, dev)", enter_root(0, 1));
    check("Zelle: Geheimnis des Wirts ist nicht da", access(SECRET, F_OK) == -1);
    limits_with_mount();
    put_file("/eigen2", "zelle");
    attack_rights(); /* drops every right first */
    attack_paths();
    attack_proc();
    attack_pids();
    say("ZELLE FERTIG: %d Fehler, %d Befunde\n", fails, findings);
    return 0;
}

/* a cell that keeps the working directory it had outside (never calls chdir) */
static int cwd_cell(void) {
    char b[128] = "?";
    if (chdir("/tmp") != 0) return 50;
    if (!enter_root(1, 0)) return 51;
    prctl(PR_ULTAR_RIGHTS_DROP, ~0UL, 0, 0, 0);
    getcwd(b, sizeof b);
    say("       (Arbeitsverzeichnis von aussen behalten: getcwd=%s)\n", b);
    int got = reads_secret("sec-geheim") | reads_secret("./sec-geheim") | reads_secret("../tmp/sec-geheim") | reads_secret("../../tmp/sec-geheim");
    int fd = open(".", O_RDONLY | O_DIRECTORY);
    if (fd >= 0) got |= openat(fd, "sec-geheim", O_RDONLY) >= 0;
    finding("Ausbruch cwd: von aussen behaltenes Arbeitsverzeichnis liest das Geheimnis", got);
    return findings ? 1 : 0;
}

/* ------------------------------------------------------------------ destructive tests (one boot each) */
static int dos(const char *what) {
    mkdir("/tmp", 0755);
    pid_t c = syscall(SYS_clone, CLONE_NEWNS | CLONE_NEWPID | SIGCHLD, 0, 0, 0, 0);
    if (c != 0) {
        int st = 0;
        waitpid(c, &st, 0);
        say("DOS %s: Wirt lebt noch, Zelle endete mit Status %#x\n", what, st);
        return 0;
    }
    if (!enter_root(0, 1)) _exit(9);
    prctl(PR_ULTAR_RIGHTS_DROP, ~0UL, 0, 0, 0);
    say("DOS %s: Zelle ohne Rechte (Rechte=%#lx) beginnt\n", what, rights());
    if (!strcmp(what, "tmpfs")) {
        static char block[1 << 20];
        memset(block, 'A', sizeof block);
        int fd = open("/gross", O_WRONLY | O_CREAT, 0644);
        long mb = 0;
        while (write(fd, block, sizeof block) == sizeof block) {
            if (++mb % 64 == 0) say("  tmpfs: %ld MB geschrieben\n", mb);
            if (mb > 8192) break;
        }
        say("  tmpfs: write endete nach %ld MB mit %s\n", mb, strerror(errno));
    } else if (!strcmp(what, "fork")) {
        long n = 0;
        for (;;) {
            pid_t k = fork();
            if (k < 0) break;
            if (k == 0) {
                for (;;) pause();
            }
            if (++n % 100 == 0) say("  fork: %ld Prozesse\n", n);
            if (n > 20000) break;
        }
        say("  fork: endete nach %ld Prozessen mit %s\n", n, strerror(errno));
    } else if (!strcmp(what, "trunc")) {
        int fd = open("/t", O_RDWR | O_CREAT, 0644);
        long r = ftruncate(fd, 1L << 40);
        say("  ftruncate(1 TiB) -> %ld %s\n", r, r ? strerror(errno) : "");
    } else if (!strcmp(what, "dupfd")) {
        long r = fcntl(0, F_DUPFD, 1L << 40);
        say("  fcntl(F_DUPFD, 2^40) -> %ld %s\n", r, r < 0 ? strerror(errno) : "");
    } else if (!strcmp(what, "seek")) {
        int fd = open("/s", O_RDWR | O_CREAT, 0644);
        lseek(fd, 1L << 40, SEEK_SET);
        long r = write(fd, "x", 1);
        say("  lseek(1 TiB)+write -> %ld %s\n", r, r < 0 ? strerror(errno) : "");
    } else if (!strcmp(what, "mem")) {
        long len = 3L << 30;
        char *p = mmap(NULL, len, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
        say("  mmap(3 GiB) -> %p\n", (void *)p);
        for (long i = 0; i < len; i += 4096) {
            p[i] = 1;
            if (i % (128L << 20) == 0) say("  berührt: %ld MB\n", i >> 20);
        }
        say("  alles berührt?!\n");
    } else if (!strcmp(what, "munmap")) {
        say("  munmap(0x400000, 2^46) ...\n");
        long r = syscall(SYS_munmap, 0x40000000UL, 1UL << 46);
        say("  munmap -> %ld\n", r);
    } else if (!strcmp(what, "mmapfix")) {
        /* MAP_FIXED at a non-canonical address: the page-table index wraps into the kernel's identity map */
        long done = 0;
        for (unsigned long target = 0; target < (4UL << 30); target += 2UL << 20) {
            if (target >= 0x400000 && target < 0x10000000) continue; /* the ordinary user window */
            void *p = (void *)syscall(SYS_mmap, 0x0001000000000000UL | target, 4096, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED, -1, 0);
            if (p != MAP_FAILED) done++;
            if (done == 1) say("  mmap(MAP_FIXED, 0x1000000000000|%#lx) -> %p\n", target, p);
        }
        say("  %ld Abbildungen über Kern-Adressen angenommen; jetzt lässt die Zelle den Kern dorthin schreiben (read aus /dev/zero)\n", done);
        int z = open("/dev/zero", O_RDONLY);
        long wrote = 0;
        for (unsigned long target = 0; target < (4UL << 30); target += 2UL << 20) {
            if (target >= 0x400000 && target < 0x10000000) continue;
            if (syscall(SYS_read, z, 0x0001000000000000UL | target, 1) == 1) wrote++;
            if (wrote % 64 == 1) say("  read() nach 0x1000000000000|%#lx angenommen (%ld)\n", target, wrote);
        }
        say("  %ld Schreibzugriffe des Kerns über Kern-Adressen\n", wrote);
    } else if (!strcmp(what, "pipe")) {
        int p[2];
        static char block[1 << 20];
        long mb = 0;
        if (pipe(p)) _exit(3);
        while (write(p[1], block, sizeof block) == sizeof block) {
            if (++mb % 64 == 0) say("  pipe: %ld MB im Kern gepuffert\n", mb);
            if (mb > 8192) break;
        }
        say("  pipe: endete nach %ld MB mit %s\n", mb, strerror(errno));
    }
    say("DOS %s: Zelle am Ende\n", what);
    _exit(0);
}

int main(int argc, char **argv) {
    if (argc > 1 && !strcmp(argv[1], "--nachexec")) return after_exec();
    if (argc > 2 && !strcmp(argv[1], "--dos")) return dos(argv[2]);
    say("AUSBRUCH: zusätzliche Angriffe auf die Isolation\n");
    mkdir("/tmp", 0755);
    host_pid = getpid();
    host_pgid = getpgrp();
    check("Wirt: Vorbereitung", rights() == R_ALL && put_file(SECRET, SECRET_TEXT) == 0 && (host_root_fd = open("/", O_RDONLY | O_DIRECTORY)) >= 0 && (host_tmp_fd = open("/tmp", O_RDONLY | O_DIRECTORY)) >= 0 &&
                                    (host_file_fd = open(SECRET, O_RDONLY)) >= 0 && (host_ns_fd = open("/proc/self/ns/mnt", O_RDONLY)) >= 0);
    say("       (Wirt: pid=%d pgrp=%d, fds: / = %d, /tmp = %d, ns = %d)\n", host_pid, host_pgid, host_root_fd, host_tmp_fd, host_ns_fd);
    host_bystander = fork();
    if (host_bystander == 0) {
        for (;;) pause();
    }
    /* host services on Unix sockets */
    struct sockaddr_un a = {.sun_family = AF_UNIX};
    int s1 = socket(AF_UNIX, SOCK_STREAM, 0), s2 = socket(AF_UNIX, SOCK_STREAM, 0);
    strcpy(a.sun_path, "/tmp/sec-sock");
    int ok = bind(s1, (struct sockaddr *)&a, sizeof a) == 0 && listen(s1, 4) == 0;
    memset(a.sun_path, 0, sizeof a.sun_path);
    strcpy(a.sun_path + 1, "sec-abstrakt");
    ok = ok && bind(s2, (struct sockaddr *)&a, sizeof(sa_family_t) + 1 + 12) == 0 && listen(s2, 4) == 0;
    check("Wirt: lauscht auf /tmp/sec-sock und abstraktem Socket", ok);

    pid_t cell = syscall(SYS_clone, CLONE_NEWNS | CLONE_NEWPID | SIGCHLD, 0, 0, 0, 0);
    if (cell == 0) {
        fails = findings = 0;
        cell_main();
        _exit((fails ? 1 : 0) | (findings ? 2 : 0));
    }
    int st = -1;
    check("Wirt: Zelle lief durch", waitpid(cell, &st, 0) == cell && WIFEXITED(st));
    if (WIFEXITED(st)) {
        if (WEXITSTATUS(st) & 1) fails++;
        if (WEXITSTATUS(st) & 2) findings++;
    }
    pid_t c2 = syscall(SYS_clone, CLONE_NEWNS | CLONE_NEWPID | SIGCHLD, 0, 0, 0, 0);
    if (c2 == 0) _exit(cwd_cell());
    st = -1;
    waitpid(c2, &st, 0);
    check("Wirt: Zelle mit behaltenem Arbeitsverzeichnis lief durch", WIFEXITED(st) && WEXITSTATUS(st) < 50);
    if (WIFEXITED(st) && WEXITSTATUS(st) == 1) findings++;

    /* what is left in the host tree */
    struct stat sb;
    check("Wirt: Geheimnis unverändert", reads_secret(SECRET));
    check("Wirt: Zuschauer lebt", kill(host_bystander, 0) == 0);
    check("Wirt: /proc des Wirts unberührt", access("/proc/self/status", F_OK) == 0 && access("/proc/ultar/firewall", F_OK) == 0);
    check("Wirt: nichts in /tmp und /bin entstanden", lstat("/tmp/sec-boese", &sb) == -1 && lstat("/tmp/sec-boese-dir", &sb) == -1 && lstat("/tmp/sec-boese-link", &sb) == -1 && lstat("/bin/sec-boese", &sb) == -1 &&
                                                              lstat("/bin/sec-x", &sb) == -1 && lstat("/bin/sec-neu", &sb) == -1 && lstat("/bin/sec-boese2", &sb) == -1 && access("/bin/hallo", F_OK) == 0);
    int in_proc = stat("/proc/sec-kanal", &sb) == 0;
    long proc_size = in_proc ? (long)sb.st_size : -1;
    int in_dev = stat("/dev/sec-kanal", &sb) == 0;
    char b[32] = {0};
    int fd = open("/dev/sec-kanal", O_RDONLY);
    if (fd >= 0 && read(fd, b, sizeof b - 1) < 0) b[0] = 0;
    say("       (Wirt nach der Zelle: stat /proc/sec-kanal=%d Grösse=%ld, /dev/sec-kanal=%d Inhalt='%s', /dev/sec-ordner=%d)\n", in_proc, proc_size, in_dev, b, stat("/dev/sec-ordner", &sb) == 0);
    finding("Wirt: Datei der Zelle liegt nach deren Ende im Baum des Wirts unter /proc (stat)", in_proc);
    finding("Wirt: Datei der Zelle liegt nach deren Ende im Baum des Wirts unter /dev und ist lesbar", in_dev && !strcmp(b, "AUS-DER-ZELLE"));
    kill(host_bystander, SIGKILL);
    waitpid(host_bystander, NULL, 0);
    unlink(SECRET);
    say("AUSBRUCH: %d Fehler, %d Befunde\n", fails, findings);
    say("AUSBRUCH: %s\n", fails == 0 && findings == 0 ? "ALLES DICHT" : "SIEHE OBEN");
    return 0;
}
