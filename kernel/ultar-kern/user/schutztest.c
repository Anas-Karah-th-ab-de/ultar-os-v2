/* schutztest: mprotect of the Ultar kernel – page rights change for mapped and reserved pages, a copy
 * of a file below a noexec mount never becomes executable. */
#define _GNU_SOURCE
#include <errno.h>
#include <pthread.h>
#include <sched.h>
#include <sys/stat.h>
#include <fcntl.h>
#include <signal.h>
#include <stdio.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/mount.h>
#include <sys/syscall.h>
#include <sys/wait.h>
#include <unistd.h>

static int fails;

static void check(const char *name, int ok) {
    printf("%s %s\n", ok ? "ok    " : "FEHLER", name);
    if (!ok) fails++;
}

/* runs fn in a child: 0 when it ended normally, else the signal that ended it */
static int in_child(void (*fn)(void *), void *arg) {
    pid_t c = fork();
    if (c == 0) {
        fn(arg);
        _exit(0);
    }
    int st = 0;
    waitpid(c, &st, 0);
    return WIFSIGNALED(st) ? WTERMSIG(st) : 0;
}

static void write_to(void *p) { *(volatile char *)p = 1; }
static void call(void *p) { ((void (*)(void))p)(); }

/* a thread that writes the page again and again (on another core when there is one) */
static void *writer(void *p) {
    for (volatile unsigned i = 0;; i++) *(volatile unsigned *)p = i;
    return NULL;
}

/* the main thread takes the right away while the writer runs: the writer must fault at once
 * (TLB shootdown), not go on through the translation its core cached */
static void shootdown(void *unused) {
    (void)unused;
    unsigned *p = mmap(NULL, 4096, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    pthread_t t;
    if (p == MAP_FAILED || pthread_create(&t, NULL, writer, p) != 0) _exit(2);
    unsigned before = 0;
    while ((before = *p) < 1000) sched_yield();
    mprotect(p, 4096, PROT_READ);
    /* the writer is on another core: no switch of address space flushes its TLB */
    for (long i = 0; i < 2000000000L; i++) __asm__ volatile("pause");
    _exit(0);
}

int main(void) {
    printf("SCHUTZTEST: mprotect\n");
    long pg = 4096;
    char *a = mmap(NULL, 2 * pg, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    check("anonyme Seiten RW", a != MAP_FAILED);
    a[0] = 0x5a;
    check("mprotect(PROT_READ)", mprotect(a, 2 * pg, PROT_READ) == 0);
    check("lesen geht weiter", a[0] == 0x5a);
    check("schreiben -> SIGSEGV (belegte Seite)", in_child(write_to, a) == SIGSEGV);
    check("schreiben -> SIGSEGV (noch leere Seite)", in_child(write_to, a + pg) == SIGSEGV);
    check("mprotect(RW) zurück, schreiben geht", mprotect(a, 2 * pg, PROT_READ | PROT_WRITE) == 0 && in_child(write_to, a + pg) == 0);
    a[100] = (char)0xc3; /* ret */
    check("Code in RW-Seite ausführen -> SIGSEGV", in_child(call, a + 100) == SIGSEGV);
    check("mprotect(RX), ausführen geht", mprotect(a, pg, PROT_READ | PROT_EXEC) == 0 && in_child(call, a + 100) == 0);
    check("RX: schreiben -> SIGSEGV", in_child(write_to, a) == SIGSEGV);
    errno = 0;
    check("unbekannte Bits -> EINVAL", mprotect(a, pg, 0x1000) == -1 && errno == EINVAL);
    check("schiefe Adresse -> EINVAL", mprotect(a + 1, pg, PROT_READ) == -1 && errno == EINVAL);
    check("PROT_NONE: schreiben -> SIGSEGV", mprotect(a, 2 * pg, PROT_NONE) == 0 && in_child(write_to, a + pg) == SIGSEGV);
    munmap(a, 2 * pg);
    /* a copy of a file below a noexec mount (needs the right MOUNT: the test runs as root on the image) */
    /* own mount namespace: in the initial one the Ultar kernel accepts mount without effect */
    int ok = unshare(CLONE_NEWNS) == 0 && (mkdir("/tmp/nx", 0755) == 0 || errno == EEXIST);
    ok = ok && mount("tmpfs", "/tmp/nx", "tmpfs", MS_NOEXEC, NULL) == 0;
    int fd = ok ? open("/tmp/nx/f", O_RDWR | O_CREAT | O_TRUNC, 0755) : -1;
    char ret = (char)0xc3;
    ok = fd >= 0 && write(fd, &ret, 1) == 1;
    char *f = ok ? mmap(NULL, pg, PROT_READ, MAP_PRIVATE, fd, 0) : MAP_FAILED;
    if (f == MAP_FAILED) {
        printf("       (noexec-Teil übersprungen: kein eigener Namensraum)\n");
    } else {
        errno = 0;
        check("noexec-Datei: mprotect(PROT_EXEC) -> EACCES", mprotect(f, pg, PROT_READ | PROT_EXEC) == -1 && errno == EACCES);
        pid_t c = fork();
        if (c == 0) _exit(mprotect(f, pg, PROT_READ | PROT_EXEC) == -1 && errno == EACCES ? 0 : 1);
        int st = -1;
        check("noexec-Datei: auch die Kopie im Kind (fork)", waitpid(c, &st, 0) == c && WIFEXITED(st) && WEXITSTATUS(st) == 0);
    }
    if (fd >= 0) close(fd);
    check("Faden auf anderem Kern schreibt -> mprotect trifft ihn sofort (SIGSEGV)", in_child(shootdown, NULL) == SIGSEGV);
    errno = 0;
    check("clone(CLONE_THREAD) ohne CLONE_VM -> EINVAL", syscall(SYS_clone, 0x10000 | 0x800 | SIGCHLD, 0, 0, 0, 0) == -1 && errno == EINVAL);
    printf("SCHUTZTEST: %d Fehler\n", fails);
    printf(fails ? "SCHUTZTEST: FEHLER\n" : "SCHUTZTEST: ALLES OK\n");
    return fails != 0;
}
