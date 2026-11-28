// What Wine and many Linux programs need from the Ultar kernel (docs/integration/kern-wine.md):
// mmap with an address wish, MAP_FIXED_NOREPLACE, MAP_FIXED over mappings, partial munmap, mremap in
// place, MAP_32BIT, large MAP_NORESERVE reservations, the second low window (Windows shared user data at
// 0x7ffe0000), waitid, arch_prctl(ARCH_SET_GS) across task switches, sigaltstack with SA_ONSTACK and
// SS_AUTODISARM, siginfo of faults (si_addr, si_code, cr2), shared file mappings (MAP_SHARED),
// /proc/self (exe, cwd, fd/N, maps – also in a cell's own proc mount), fatal signals that end every
// thread (abort, blocked or ignored faults) and Unix sockets through SCM_RIGHTS.
// Verdict line: "WINETEST: ALLES OK".
#define _GNU_SOURCE
#include <asm/prctl.h>
#include <dirent.h>
#include <pthread.h>
#include <sys/mount.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/un.h>
#include <errno.h>
#include <fcntl.h>
#include <sched.h>
#include <setjmp.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/syscall.h>
#include <sys/ucontext.h>
#include <sys/wait.h>
#include <unistd.h>

#ifndef SS_AUTODISARM
#define SS_AUTODISARM (1U << 31)
#endif

#ifndef MAP_FIXED_NOREPLACE
#define MAP_FIXED_NOREPLACE 0x100000
#endif

static int failures;
static const long PG = 4096;

static void check(int ok, const char *what) {
    printf("%s %s\n", ok ? "ok    " : "FEHLER", what);
    if (!ok) failures++;
}

static void *map_at(void *addr, size_t len, int prot, int flags) {
    return mmap(addr, len, prot, flags | MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
}

// ------------------------------------------------------------------ placement

static void placement(void) {
    char *base = map_at(NULL, 16 * PG, PROT_READ | PROT_WRITE, 0);
    check(base != MAP_FAILED, "mmap: anonymous mapping");
    munmap(base, 16 * PG);
    char *hint = base + 4 * PG;
    char *got = map_at(hint, 2 * PG, PROT_READ | PROT_WRITE, 0);
    check(got == hint, "mmap: a free address wish is taken");
    char *again = map_at(hint, 2 * PG, PROT_READ | PROT_WRITE, 0);
    check(again != MAP_FAILED && again != hint, "mmap: an occupied wish goes elsewhere");
    char *nr = map_at(hint, PG, PROT_READ, MAP_FIXED_NOREPLACE);
    check(nr == MAP_FAILED && errno == EEXIST, "MAP_FIXED_NOREPLACE: occupied -> EEXIST");
    char *nr2 = map_at(hint + 8 * PG, PG, PROT_READ | PROT_WRITE, MAP_FIXED_NOREPLACE);
    check(nr2 == hint + 8 * PG, "MAP_FIXED_NOREPLACE: free -> that address");
    char *kern = map_at((void *)0x20000000, PG, PROT_READ, MAP_FIXED_NOREPLACE);
    // the Ultar kernel keeps 0x10000000..0x7c000000 for itself (EEXIST); Linux maps it there – never elsewhere
    check(kern == MAP_FAILED ? errno == EEXIST : kern == (char *)0x20000000,
          "MAP_FIXED_NOREPLACE: a range the kernel keeps -> EEXIST, never another address");
    if (kern != MAP_FAILED) munmap(kern, PG);
    got[0] = 42;
    char *fx = map_at(got, PG, PROT_READ | PROT_WRITE, MAP_FIXED);
    check(fx == got && fx[0] == 0, "MAP_FIXED replaces a mapping");
    munmap(got, 2 * PG);
    munmap(again, 2 * PG);
    munmap(nr2, PG);
}

static sigjmp_buf jump;
static volatile void *fault_addr;
static volatile int fault_code;
static volatile unsigned long fault_cr2;

static void on_segv(int sig, siginfo_t *si, void *uc) {
    (void)sig;
    fault_addr = si->si_addr;
    fault_code = si->si_code;
    fault_cr2 = ((ucontext_t *)uc)->uc_mcontext.gregs[REG_CR2];
    siglongjmp(jump, 1);
}

static int touch(volatile char *p, int write) {
    fault_addr = NULL;
    if (sigsetjmp(jump, 1) == 0) {
        if (write)
            *p = 1;
        else
            (void)*p;
        return 0;
    }
    return 1;
}

static void partial_and_faults(void) {
    struct sigaction sa = {0};
    sa.sa_sigaction = on_segv;
    sa.sa_flags = SA_SIGINFO;
    sigaction(SIGSEGV, &sa, NULL);
    char *m = map_at(NULL, 4 * PG, PROT_READ | PROT_WRITE, 0);
    memset(m, 7, 4 * PG);
    check(munmap(m + PG, PG) == 0, "munmap: one page out of the middle");
    int hole = touch(m + PG, 0);
    check(hole && fault_addr == m + PG && fault_code == SEGV_MAPERR && fault_cr2 == (unsigned long)(m + PG), "fault: si_addr, SEGV_MAPERR and cr2 of the hole");
    check(m[0] == 7 && m[2 * PG] == 7 && m[3 * PG] == 7, "munmap: the rest stays");
    mprotect(m, PG, PROT_READ);
    int ro = touch(m, 1);
    check(ro && fault_addr == m && fault_code == SEGV_ACCERR, "fault: writing a read-only page is SEGV_ACCERR");
    munmap(m, 4 * PG);
    // mremap grows where it is when the space behind is free
    char *r = map_at(NULL, 2 * PG, PROT_READ | PROT_WRITE, 0);
    munmap(r + 2 * PG, 4 * PG);
    r[0] = 5;
    char *g = mremap(r, 2 * PG, 4 * PG, 0);
    check(g == r && g[0] == 5, "mremap: grows in place");
    g[3 * PG] = 1;
    munmap(g, 4 * PG);
    char *low = map_at(NULL, PG, PROT_READ | PROT_WRITE, MAP_32BIT);
    check(low != MAP_FAILED && (uintptr_t)low + PG <= 0x80000000UL, "MAP_32BIT: below 2 GiB");
    if (low != MAP_FAILED) {
        low[0] = 3;
        munmap(low, PG);
    }
    char *big = map_at(NULL, 64UL << 30, PROT_NONE, MAP_NORESERVE);
    check(big != MAP_FAILED, "MAP_NORESERVE: 64 GiB reserved");
    if (big != MAP_FAILED) munmap(big, 64UL << 30);
    char *kusd = map_at((void *)0x7ffe0000, 0x10000, PROT_READ | PROT_WRITE, MAP_FIXED_NOREPLACE);
    check(kusd == (void *)0x7ffe0000, "second low window: Windows shared user data at 0x7ffe0000");
    if (kusd != MAP_FAILED) {
        kusd[0] = 1;
        munmap(kusd, 0x10000);
    }
    signal(SIGSEGV, SIG_DFL);
}

// ------------------------------------------------------------------ waitid

static void waits(void) {
    siginfo_t si;
    pid_t c = fork();
    if (c == 0) _exit(7);
    memset(&si, 0, sizeof si);
    int r = waitid(P_PID, c, &si, WEXITED | WNOWAIT);
    check(r == 0 && si.si_signo == SIGCHLD && si.si_code == CLD_EXITED && si.si_status == 7 && si.si_pid == c, "waitid: P_PID, WEXITED|WNOWAIT reports the exit");
    int st = 0;
    check(waitpid(c, &st, 0) == c && WIFEXITED(st) && WEXITSTATUS(st) == 7, "waitid: WNOWAIT left the child to wait for");
    c = fork();
    if (c == 0) {
        pause();
        _exit(0);
    }
    memset(&si, 1, sizeof si);
    r = waitid(P_ALL, 0, &si, WEXITED | WNOHANG);
    check(r == 0 && si.si_pid == 0, "waitid: WNOHANG with a running child");
    kill(c, SIGKILL);
    r = waitid(P_PGID, 0, &si, WEXITED);
    check(r == 0 && si.si_code == CLD_KILLED && si.si_status == SIGKILL && si.si_pid == c, "waitid: P_PGID, killed child");
    check(waitid(P_ALL, 0, &si, WEXITED) == -1 && errno == ECHILD, "waitid: no child -> ECHILD");
    check(waitid(P_ALL, 0, &si, 0) == -1 && errno == EINVAL, "waitid: no event -> EINVAL");
}

// ------------------------------------------------------------------ GS base

static unsigned long gs_word(void) {
    unsigned long v;
    __asm__ volatile("movq %%gs:0, %0" : "=r"(v));
    return v;
}

static unsigned long gs_block[4] = {0x1234abcd5678ef00UL, 0, 0, 0};

static void gs_base(void) {
    check(syscall(SYS_arch_prctl, ARCH_SET_GS, (unsigned long)gs_block) == 0, "arch_prctl: ARCH_SET_GS");
    unsigned long got = 0;
    syscall(SYS_arch_prctl, ARCH_GET_GS, &got);
    check(got == (unsigned long)gs_block, "arch_prctl: ARCH_GET_GS");
    check(gs_word() == gs_block[0], "GS: %gs:0 reads the block");
    for (int i = 0; i < 50; i++) {
        usleep(1000);
        sched_yield();
    }
    check(gs_word() == gs_block[0], "GS: kept across task switches");
    pid_t c = fork();
    if (c == 0) _exit(gs_word() == gs_block[0] ? 0 : 1);
    int st = 0;
    waitpid(c, &st, 0);
    check(WIFEXITED(st) && WEXITSTATUS(st) == 0, "GS: a forked child keeps it");
    check(syscall(SYS_arch_prctl, ARCH_SET_GS, 0x800000000000UL) == -1 && errno == EPERM, "arch_prctl: GS outside the user half -> EPERM");
    syscall(SYS_arch_prctl, ARCH_SET_GS, 0UL);
}

// ------------------------------------------------------------------ sigaltstack

static char alt[64 * 1024];
static volatile int on_alt, alt_flags_in_handler;

static void on_usr1(int s) {
    (void)s;
    char local;
    on_alt = &local >= alt && &local < alt + sizeof alt;
    stack_t now;
    sigaltstack(NULL, &now);
    alt_flags_in_handler = now.ss_flags;
}

static void alt_stack(void) {
    stack_t ss = {.ss_sp = alt, .ss_size = sizeof alt, .ss_flags = 0};
    check(sigaltstack(&ss, NULL) == 0, "sigaltstack: set");
    struct sigaction sa = {0};
    sa.sa_handler = on_usr1;
    sa.sa_flags = SA_ONSTACK;
    sigaction(SIGUSR1, &sa, NULL);
    raise(SIGUSR1);
    check(on_alt && alt_flags_in_handler == SS_ONSTACK, "SA_ONSTACK: the handler runs on the alternate stack");
    stack_t old;
    sigaltstack(NULL, &old);
    check(old.ss_sp == alt && old.ss_size == sizeof alt && old.ss_flags == 0, "sigaltstack: reads back, not on it any more");
    ss.ss_flags = SS_AUTODISARM;
    sigaltstack(&ss, NULL);
    raise(SIGUSR1);
    check(on_alt && alt_flags_in_handler == SS_DISABLE, "SS_AUTODISARM: disarmed in the handler");
    sigaltstack(NULL, &old);
    check(old.ss_sp == alt && (old.ss_flags & SS_DISABLE) == 0, "SS_AUTODISARM: back after the handler");
    stack_t small = {.ss_sp = alt, .ss_size = 512, .ss_flags = 0};
    check(sigaltstack(&small, NULL) == -1 && errno == ENOMEM, "sigaltstack: too small -> ENOMEM");
    ss.ss_flags = SS_DISABLE;
    sigaltstack(&ss, NULL);
    signal(SIGUSR1, SIG_DFL);
}

// ------------------------------------------------------------------ shared file mappings

static void shared_files(const char *path) {
    int fd = open(path, O_CREAT | O_RDWR | O_TRUNC, 0644);
    check(fd >= 0 && ftruncate(fd, 2 * PG) == 0, "MAP_SHARED: file of two pages");
    char *a = mmap(NULL, 2 * PG, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    char *b = mmap(NULL, 2 * PG, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    check(a != MAP_FAILED && b != MAP_FAILED && a != b, "MAP_SHARED: two mappings of one file");
    strcpy(a, "hallo");
    check(strcmp(b, "hallo") == 0, "MAP_SHARED: a write is seen by the other mapping");
    char buf[8] = {0};
    check(pread(fd, buf, 5, 0) == 5 && strcmp(buf, "hallo") == 0, "MAP_SHARED: read() sees the mapping's write");
    check(pwrite(fd, "welt", 4, PG) == 4 && memcmp(a + PG, "welt", 4) == 0, "MAP_SHARED: write() reaches the mapping");
    pid_t c = fork();
    if (c == 0) {
        strcpy(b + 100, "kind");
        _exit(0);
    }
    waitpid(c, NULL, 0);
    check(strcmp(a + 100, "kind") == 0, "MAP_SHARED: a forked child writes into the same pages");
    check(msync(a, 2 * PG, MS_SYNC) == 0, "msync: MS_SYNC");
    int fd2 = open(path, O_RDONLY);
    memset(buf, 0, sizeof buf);
    check(fd2 >= 0 && read(fd2, buf, 5) == 5 && strcmp(buf, "hallo") == 0, "msync: a new open reads the mapping's bytes");
    close(fd2);
    strcpy(a + 200, "ende");
    munmap(a, 2 * PG);
    munmap(b, 2 * PG);
    close(fd);
    fd = open(path, O_RDONLY);
    char tail[8] = {0};
    check(fd >= 0 && pread(fd, tail, 4, 200) == 4 && strcmp(tail, "ende") == 0, "munmap + close: the file keeps the last write");
    close(fd);
    unlink(path);
}

// ---- /proc/self: exe, cwd, fd/N (readlink and open), the pid, maps (ART, Wine) -------------------

static int link_is(const char *path, const char *want) {
    char b[256];
    ssize_t n = readlink(path, b, sizeof b - 1);
    if (n < 0) return 0;
    b[n] = 0;
    return want[strlen(want) - 1] == '[' ? strncmp(b, want, strlen(want)) == 0 : strcmp(b, want) == 0;
}

// the line of /proc/self/maps that holds `addr`: its rights and name (or 0)
static int maps_line(uintptr_t addr, char *perms, char *name) {
    FILE *f = fopen("/proc/self/maps", "r");
    if (!f) return 0;
    char line[512];
    int found = 0;
    while (fgets(line, sizeof line, f)) {
        unsigned long a, b;
        char pr[8] = "", nm[256] = "";
        if (sscanf(line, "%lx-%lx %7s %*s %*s %*s %255s", &a, &b, pr, nm) >= 3 && a <= addr && addr < b) {
            strcpy(perms, pr);
            strcpy(name, nm);
            found = 1;
            break;
        }
    }
    fclose(f);
    return found;
}

static void proc_self(const char *exe, const char *dir, const char *where) {
    char what[160], path[128], fdp[64];
    snprintf(what, sizeof what, "%s: readlink /proc/self/exe = %s", where, exe);
    check(link_is("/proc/self/exe", exe), what);
    char pid[32];
    snprintf(pid, sizeof pid, "%d", getpid());
    snprintf(what, sizeof what, "%s: readlink /proc/self = the pid", where);
    check(link_is("/proc/self", pid), what);
    check(chdir(dir) == 0, "chdir");
    snprintf(what, sizeof what, "%s: readlink /proc/self/cwd", where);
    check(link_is("/proc/self/cwd", dir), what);
    snprintf(path, sizeof path, "%s/wt-fd", strcmp(dir, "/") ? dir : "");
    int fd = open(path, O_CREAT | O_TRUNC | O_RDWR, 0644);
    if (fd >= 0 && write(fd, "ultar", 5) != 5) fd = -1;
    snprintf(fdp, sizeof fdp, "/proc/self/fd/%d", fd);
    snprintf(what, sizeof what, "%s: readlink /proc/self/fd/N of a file", where);
    check(fd >= 0 && link_is(fdp, path), what);
    int again = open(fdp, O_RDONLY);
    char b[8] = "";
    snprintf(what, sizeof what, "%s: open /proc/self/fd/N opens the file anew", where);
    check(again >= 0 && again != fd && read(again, b, 5) == 5 && memcmp(b, "ultar", 5) == 0, what);
    int pp[2];
    check(pipe(pp) == 0, "pipe");
    snprintf(fdp, sizeof fdp, "/proc/self/fd/%d", pp[0]);
    snprintf(what, sizeof what, "%s: readlink /proc/self/fd/N of a pipe", where);
    check(link_is(fdp, "pipe:["), what);
    int seen = 0;
    DIR *d = opendir("/proc/self/fd");
    struct dirent *e;
    while (d && (e = readdir(d)))
        if (atoi(e->d_name) == fd) seen = 1;
    if (d) closedir(d);
    snprintf(what, sizeof what, "%s: /proc/self/fd lists the descriptor", where);
    check(seen, what);
    close(pp[0]);
    close(pp[1]);
    close(fd);
    if (again >= 0) close(again);
    unlink(path);
    // maps: the stack, an anonymous mapping, the code
    int local = 0;
    char pr[8], nm[256];
    snprintf(what, sizeof what, "%s: /proc/self/maps: the stack is [stack], rw", where);
    check(maps_line((uintptr_t)&local, pr, nm) && strcmp(nm, "[stack]") == 0 && pr[1] == 'w', what);
    char *m = mmap(NULL, 3 * PG, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    snprintf(what, sizeof what, "%s: /proc/self/maps: an anonymous mapping, rw-p", where);
    check(m != MAP_FAILED && maps_line((uintptr_t)m + PG, pr, nm) && strcmp(pr, "rw-p") == 0, what);
    if (m != MAP_FAILED) munmap(m, 3 * PG);
    snprintf(what, sizeof what, "%s: /proc/self/maps: the code is executable", where);
    check(maps_line((uintptr_t)&proc_self, pr, nm) && pr[2] == 'x', what);
    pthread_attr_t at;
    void *st = NULL;
    size_t sz = 0;
    int r = pthread_getattr_np(pthread_self(), &at);
    if (r == 0) {
        pthread_attr_getstack(&at, &st, &sz);
        pthread_attr_destroy(&at);
    }
    snprintf(what, sizeof what, "%s: pthread_getattr_np finds the main stack", where);
    check(r == 0 && (char *)st <= (char *)&local && (char *)&local < (char *)st + sz, what);
}

// the same in a cell: own mount and pid namespace, a tmpfs root (pivot_root), /bin bound, proc mounted
static void proc_in_cell(void) {
    pid_t c = fork();
    if (c == 0) {
        if (unshare(CLONE_NEWNS | CLONE_NEWPID) != 0) { printf("FEHLER unshare: %s\n", strerror(errno)); _exit(9); }
        pid_t g = fork();
        if (g == 0) {
            const char *r = "/tmp/wt-cell";
            mkdir(r, 0755);
            int ok = mount("tmpfs", r, "tmpfs", 0, NULL) == 0 && mkdir("/tmp/wt-cell/bin", 0755) == 0 && mkdir("/tmp/wt-cell/proc", 0755) == 0 &&
                     mkdir("/tmp/wt-cell/old", 0755) == 0 && mount("/bin", "/tmp/wt-cell/bin", NULL, MS_BIND, NULL) == 0 &&
                     mount("proc", "/tmp/wt-cell/proc", "proc", 0, NULL) == 0 && syscall(SYS_pivot_root, r, "/tmp/wt-cell/old") == 0 && chdir("/") == 0;
            if (!ok) { printf("FEHLER cell set-up: %s\n", strerror(errno)); _exit(9); }
            umount2("/old", MNT_DETACH);
            execl("/bin/winetest", "winetest", "--zelle", (char *)NULL);
            _exit(8);
        }
        int st = 0;
        waitpid(g, &st, 0);
        _exit(WIFEXITED(st) ? WEXITSTATUS(st) : 7);
    }
    int st = 0;
    waitpid(c, &st, 0);
    check(WIFEXITED(st) && WEXITSTATUS(st) == 0, "cell: /proc/self in a cell's own proc mount (lines above)");
}

// ---- fatal signals end the whole process (ART kept running after abort()) ------------------------

static void *abort_thread(void *a) {
    (void)a;
    abort();
}
static void *null_thread(void *a) {
    (void)a;
    *(volatile int *)0 = 1;
    return NULL;
}
static void *sleeper(void *a) {
    (void)a;
    for (;;) pause();
    return NULL;
}
static void returning(int s) { (void)s; }

static int ends_by(int how) {
    pid_t c = fork();
    if (c == 0) {
        alarm(20);
        pthread_t t;
        sigset_t set;
        sigemptyset(&set);
        switch (how) {
        case 1:  // a second thread calls abort(), the main thread waits for it
            pthread_create(&t, NULL, abort_thread, NULL);
            pthread_join(t, NULL);
            break;
        case 2:  // SIGABRT blocked: abort() unblocks it
            sigaddset(&set, SIGABRT);
            sigprocmask(SIG_BLOCK, &set, NULL);
            abort();
        case 3:  // a handler that returns: abort() still ends the process
            signal(SIGABRT, returning);
            abort();
        case 4:  // SIGSEGV blocked: the fault still ends the process
            sigaddset(&set, SIGSEGV);
            sigprocmask(SIG_BLOCK, &set, NULL);
            *(volatile int *)0 = 1;
            break;
        case 5:  // SIGSEGV ignored: the same
            signal(SIGSEGV, SIG_IGN);
            *(volatile int *)0 = 1;
            break;
        case 6:  // a fault in a second thread ends all threads
            pthread_create(&t, NULL, null_thread, NULL);
            pthread_join(t, NULL);
            break;
        case 7:  // SIGTERM blocked in the main thread, not in a second one: the process ends
            pthread_create(&t, NULL, sleeper, NULL);
            sigaddset(&set, SIGTERM);
            pthread_sigmask(SIG_BLOCK, &set, NULL);
            for (;;) pause();
        }
        _exit(0);
    }
    if (how == 7) {
        usleep(300 * 1000);
        kill(c, SIGTERM);
    }
    int st = 0;
    waitpid(c, &st, 0);
    return WIFSIGNALED(st) ? WTERMSIG(st) : -1;
}

static void fatal_signals(void) {
    check(ends_by(1) == SIGABRT, "abort() in a second thread ends the process (SIGABRT)");
    check(ends_by(2) == SIGABRT, "abort() with SIGABRT blocked ends the process");
    check(ends_by(3) == SIGABRT, "abort() with a returning handler ends the process");
    check(ends_by(4) == SIGSEGV, "a fault with SIGSEGV blocked ends the process");
    check(ends_by(5) == SIGSEGV, "a fault with SIGSEGV ignored ends the process");
    check(ends_by(6) == SIGSEGV, "a fault in a second thread ends all threads");
    check(ends_by(7) == SIGTERM, "SIGTERM blocked in the main thread reaches another thread and ends the process");
}

// ---- Unix sockets travel through SCM_RIGHTS (Wine hands the server a socket per new process) ------

static int send_fd(int over, int fd) {
    char c = 'x';
    struct iovec v = {&c, 1};
    char ctl[CMSG_SPACE(sizeof(int))];
    memset(ctl, 0, sizeof ctl);
    struct msghdr m = {0};
    m.msg_iov = &v;
    m.msg_iovlen = 1;
    m.msg_control = ctl;
    m.msg_controllen = sizeof ctl;
    struct cmsghdr *h = CMSG_FIRSTHDR(&m);
    h->cmsg_level = SOL_SOCKET;
    h->cmsg_type = SCM_RIGHTS;
    h->cmsg_len = CMSG_LEN(sizeof(int));
    memcpy(CMSG_DATA(h), &fd, sizeof fd);
    return sendmsg(over, &m, 0) == 1 ? 0 : -errno;
}

static int recv_fd(int from) {
    char c;
    struct iovec v = {&c, 1};
    char ctl[CMSG_SPACE(sizeof(int))];
    struct msghdr m = {0};
    m.msg_iov = &v;
    m.msg_iovlen = 1;
    m.msg_control = ctl;
    m.msg_controllen = sizeof ctl;
    if (recvmsg(from, &m, 0) != 1) return -1;
    struct cmsghdr *h = CMSG_FIRSTHDR(&m);
    int fd = -1;
    if (h && h->cmsg_type == SCM_RIGHTS) memcpy(&fd, CMSG_DATA(h), sizeof fd);
    return fd;
}

static void socket_rights(void) {
    int a[2], b[2], c[2];
    check(socketpair(AF_UNIX, SOCK_STREAM, 0, a) == 0 && socketpair(AF_UNIX, SOCK_STREAM, 0, b) == 0 && socketpair(AF_UNIX, SOCK_STREAM, 0, c) == 0, "socketpairs");
    check(send_fd(a[0], b[1]) == 0, "SCM_RIGHTS: a connected Unix socket travels");
    int got = recv_fd(a[1]);
    char buf[4] = "";
    check(got >= 0 && write(got, "ok", 2) == 2 && read(b[0], buf, 2) == 2 && memcmp(buf, "ok", 2) == 0, "SCM_RIGHTS: the received socket is the same connection");
    check(send_fd(a[0], a[1]) == -EOPNOTSUPP, "SCM_RIGHTS: a socket never lands in itself (EOPNOTSUPP)");
    check(send_fd(a[0], c[1]) == 0, "SCM_RIGHTS: a socket waits in a queue");
    check(send_fd(c[0], a[1]) == -EOPNOTSUPP, "SCM_RIGHTS: a socket that carries a socket stays (no ring without a collector)");
    int l = socket(AF_UNIX, SOCK_STREAM, 0);
    struct sockaddr_un sa = {.sun_family = AF_UNIX};
    strcpy(sa.sun_path, "/tmp/wt-listen");
    unlink(sa.sun_path);
    check(l >= 0 && bind(l, (struct sockaddr *)&sa, sizeof sa) == 0 && listen(l, 1) == 0 && send_fd(b[0], l) == -EOPNOTSUPP, "SCM_RIGHTS: a listening socket stays");
    int got2 = recv_fd(a[1]);
    check(got2 >= 0, "SCM_RIGHTS: the waiting socket arrives");
    int all[] = {a[0], a[1], b[0], b[1], c[0], c[1], got, got2, l};
    for (unsigned i = 0; i < sizeof all / sizeof *all; i++)
        if (all[i] >= 0) close(all[i]);
    unlink(sa.sun_path);
}

int main(int argc, char **argv) {
    setvbuf(stdout, NULL, _IONBF, 0);
    if (argc > 1 && strcmp(argv[1], "--zelle") == 0) {
        proc_self("/bin/winetest", "/", "cell");
        return failures != 0;
    }
    placement();
    partial_and_faults();
    waits();
    gs_base();
    alt_stack();
    shared_files("/tmp/wt-shared");
    if (access("/data", W_OK) == 0) shared_files("/data/wt-shared");
    proc_self("/bin/winetest", "/tmp", "proc");
    proc_in_cell();
    fatal_signals();
    socket_rights();
    if (failures == 0)
        printf("WINETEST: ALLES OK\n");
    else
        printf("WINETEST: %d FEHLER\n", failures);
    return failures != 0;
}
