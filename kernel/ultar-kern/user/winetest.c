// What Wine and many Linux programs need from the Ultar kernel (docs/integration/kern-wine.md):
// mmap with an address wish, MAP_FIXED_NOREPLACE, MAP_FIXED over mappings, partial munmap, mremap in
// place, MAP_32BIT, large MAP_NORESERVE reservations, the second low window (Windows shared user data at
// 0x7ffe0000), waitid, arch_prctl(ARCH_SET_GS) across task switches, sigaltstack with SA_ONSTACK and
// SS_AUTODISARM, siginfo of faults (si_addr, si_code, cr2) and shared file mappings (MAP_SHARED).
// Verdict line: "WINETEST: ALLES OK".
#define _GNU_SOURCE
#include <asm/prctl.h>
#include <errno.h>
#include <fcntl.h>
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
    check(kern == MAP_FAILED && errno == EEXIST, "MAP_FIXED_NOREPLACE: the kernel's range -> EEXIST");
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

int main(void) {
    setvbuf(stdout, NULL, _IONBF, 0);
    placement();
    partial_and_faults();
    waits();
    gs_base();
    alt_stack();
    shared_files("/tmp/wt-shared");
    if (access("/data", W_OK) == 0) shared_files("/data/wt-shared");
    if (failures == 0)
        printf("WINETEST: ALLES OK\n");
    else
        printf("WINETEST: %d FEHLER\n", failures);
    return failures != 0;
}
