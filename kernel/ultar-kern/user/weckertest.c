/* weckertest – who wakes whom on the Ultar kernel: posix_spawn/vfork release the parent at execve (not at
 * the child's exit) and report a failed execve; shutdown() and the peer's close wake readers and writers
 * blocked on an AF_UNIX socket in another thread (read 0, write EPIPE), also for poll and epoll; accept()
 * blocked on a listening socket ends at shutdown; a signal for the whole process is read from a signalfd
 * by whichever thread waits on it; a connection still queued at a listener that closes (or whose process
 * ends) reads end of file instead of waiting forever; SO_RCVTIMEO bounds a blocking read and recvmsg on a
 * Unix socket (EAGAIN after the time, a fresh limit for the next read). One line per check ("ok" / "FEHLER"); the last line is
 * "WECKERTEST: ALLES OK" when every check passed.
 *   weckertest              all checks
 *   weckertest --sleep <s>  helper: sleep, then exit 0 (the long-running child of the spawn checks) */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <pthread.h>
#include <signal.h>
#include <spawn.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/epoll.h>
#include <sys/signalfd.h>
#include <sys/socket.h>
#include <sys/syscall.h>
#include <sys/un.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

extern char **environ;
static int fails;
static char self[256] = "/bin/weckertest";

static void say(const char *fmt, ...) {
    char b[512];
    va_list ap;
    va_start(ap, fmt);
    int n = vsnprintf(b, sizeof b, fmt, ap);
    va_end(ap);
    if (n > (int)sizeof b - 1) n = sizeof b - 1;
    if (n > 0 && write(1, b, n) < 0) fails++;
}

static void check(const char *name, int ok) {
    if (!ok) fails++;
    say("%s %s\n", ok ? "ok    " : "FEHLER", name);
}

static long ms(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return t.tv_sec * 1000 + t.tv_nsec / 1000000;
}

/* ---------------------------------------------------------------- posix_spawn / vfork */

static void spawn_checks(void) {
    char *argv[] = {self, "--sleep", "3", NULL};
    pid_t pid = -1;
    long t0 = ms();
    int r = posix_spawn(&pid, self, NULL, NULL, argv, environ);
    long took = ms() - t0;
    check("posix_spawn: kehrt zurück, sobald das Kind execve gemacht hat (nicht erst an seinem Ende)", r == 0 && took < 2000);
    int st = -1;
    check("posix_spawn: das Kind läuft weiter und endet mit 0", r == 0 && waitpid(pid, &st, 0) == pid && WIFEXITED(st) && WEXITSTATUS(st) == 0);
    char *bad[] = {"/bin/gibt-es-nicht", NULL};
    r = posix_spawn(&pid, bad[0], NULL, NULL, bad, environ);
    check("posix_spawn: fehlgeschlagenes execve wird als ENOENT gemeldet", r == ENOENT);
    /* plain vfork: the child shares the memory until it calls execve or _exit */
    volatile int shared = 0;
    pid = vfork();
    if (pid == 0) {
        shared = 42;
        _exit(7);
    }
    check("vfork: Kind teilt den Speicher bis _exit", pid > 0 && shared == 42 && waitpid(pid, &st, 0) == pid && WEXITSTATUS(st) == 7);
    t0 = ms();
    pid = vfork();
    if (pid == 0) {
        execve(self, argv, environ);
        _exit(127);
    }
    took = ms() - t0;
    check("vfork + execve: Elternprozess läuft weiter, während das Kind noch schläft", pid > 0 && took < 2000);
    check("vfork + execve: Kind endet später mit 0", waitpid(pid, &st, 0) == pid && WIFEXITED(st) && WEXITSTATUS(st) == 0);
}

/* ---------------------------------------------------------------- AF_UNIX wake-ups */

struct op {
    int fd;
    int write;
    long ret;
    int err;
    long at;
};

static void *blocked(void *arg) {
    struct op *o = arg;
    char b[4096];
    memset(b, 'x', sizeof b);
    if (o->write) {
        /* fill the buffer until the write blocks */
        long r;
        do r = write(o->fd, b, sizeof b);
        while (r > 0);
        o->ret = r;
    } else {
        o->ret = read(o->fd, b, sizeof b);
    }
    o->err = o->ret < 0 ? errno : 0;
    o->at = ms();
    return NULL;
}

/* start a thread blocked on fd, wait until it really blocks, run `act`, then join with a deadline */
static int woken(struct op *o, void (*act)(int, int), int a, int b) {
    pthread_t t;
    o->at = 0;
    if (pthread_create(&t, NULL, blocked, o) != 0) return 0;
    usleep(300000);
    if (o->at != 0) return pthread_join(t, NULL), 0; /* it did not block at all */
    act(a, b);
    for (int i = 0; i < 300 && o->at == 0; i++) usleep(10000);
    if (o->at == 0) return 0; /* still blocked: the thread is left behind, the check fails */
    pthread_join(t, NULL);
    return 1;
}

static void do_shutdown(int fd, int how) { shutdown(fd, how); }
static void do_close(int fd, int unused) { (void)unused; close(fd); }

static void unix_checks(void) {
    int sv[2];
    struct op o;
    signal(SIGPIPE, SIG_IGN);
    socketpair(AF_UNIX, SOCK_STREAM, 0, sv);
    o = (struct op){.fd = sv[0]};
    check("Unix: Leser in einem anderen Faden wird von shutdown(SHUT_RD) geweckt, read = 0", woken(&o, do_shutdown, sv[0], SHUT_RD) && o.ret == 0);
    close(sv[0]);
    close(sv[1]);
    socketpair(AF_UNIX, SOCK_STREAM, 0, sv);
    o = (struct op){.fd = sv[0]};
    check("Unix: Leser wird von shutdown(SHUT_RDWR) geweckt, read = 0", woken(&o, do_shutdown, sv[0], SHUT_RDWR) && o.ret == 0);
    check("Unix: nach SHUT_RDWR schreibt niemand mehr (EPIPE)", write(sv[0], "a", 1) == -1 && errno == EPIPE);
    close(sv[0]);
    close(sv[1]);
    socketpair(AF_UNIX, SOCK_STREAM, 0, sv);
    o = (struct op){.fd = sv[0]};
    check("Unix: Leser wird vom Schließen der Gegenseite geweckt, read = 0", woken(&o, do_close, sv[1], 0) && o.ret == 0);
    close(sv[0]);
    socketpair(AF_UNIX, SOCK_STREAM, 0, sv);
    check("Unix: SHUT_WR – Gegenseite liest die Daten, dann 0", write(sv[0], "abc", 3) == 3 && shutdown(sv[0], SHUT_WR) == 0 && ({
        char b[8];
        read(sv[1], b, sizeof b) == 3 && read(sv[1], b, sizeof b) == 0;
    }));
    check("Unix: nach SHUT_WR eigenes write -> EPIPE", write(sv[0], "a", 1) == -1 && errno == EPIPE);
    close(sv[0]);
    close(sv[1]);
    socketpair(AF_UNIX, SOCK_STREAM, 0, sv);
    o = (struct op){.fd = sv[0], .write = 1};
    check("Unix: blockierter Schreiber wird von shutdown(SHUT_WR) geweckt, EPIPE", woken(&o, do_shutdown, sv[0], SHUT_WR) && o.ret == -1 && o.err == EPIPE);
    close(sv[0]);
    close(sv[1]);
    socketpair(AF_UNIX, SOCK_STREAM, 0, sv);
    o = (struct op){.fd = sv[0], .write = 1};
    check("Unix: blockierter Schreiber wird vom Schließen der Gegenseite geweckt, EPIPE", woken(&o, do_close, sv[1], 0) && o.ret == -1 && o.err == EPIPE);
    close(sv[0]);
}

static void readiness_checks(void) {
    int sv[2];
    socketpair(AF_UNIX, SOCK_STREAM, 0, sv);
    struct pollfd p = {sv[0], POLLIN | POLLRDHUP, 0};
    check("poll: vor shutdown nichts bereit", poll(&p, 1, 0) == 0);
    shutdown(sv[0], SHUT_RD);
    p.revents = 0;
    check("poll: nach shutdown(SHUT_RD) lesbar mit POLLRDHUP", poll(&p, 1, 1000) == 1 && (p.revents & POLLIN) && (p.revents & POLLRDHUP));
    int ep = epoll_create1(0);
    struct epoll_event e = {.events = EPOLLIN | EPOLLRDHUP, .data.fd = sv[0]}, got;
    epoll_ctl(ep, EPOLL_CTL_ADD, sv[0], &e);
    check("epoll: nach shutdown(SHUT_RD) bereit (EPOLLIN|EPOLLRDHUP)", epoll_wait(ep, &got, 1, 1000) == 1 && (got.events & EPOLLIN) && (got.events & EPOLLRDHUP));
    close(ep);
    close(sv[0]);
    close(sv[1]);
    socketpair(AF_UNIX, SOCK_STREAM, 0, sv);
    close(sv[1]);
    p = (struct pollfd){sv[0], POLLIN | POLLOUT, 0};
    check("poll: Gegenseite zu -> POLLIN und POLLHUP", poll(&p, 1, 1000) == 1 && (p.revents & POLLIN) && (p.revents & POLLHUP));
    close(sv[0]);
}

struct acc {
    int fd;
    int ret, err;
    long at;
};

static void *accepting(void *arg) {
    struct acc *a = arg;
    a->ret = accept(a->fd, NULL, NULL);
    a->err = errno;
    a->at = ms();
    return NULL;
}

static void accept_checks(void) {
    struct sockaddr_un a = {.sun_family = AF_UNIX};
    snprintf(a.sun_path, sizeof a.sun_path, "/tmp/weckertest-%d.sock", getpid());
    unlink(a.sun_path);
    int l = socket(AF_UNIX, SOCK_STREAM, 0);
    int ok = bind(l, (void *)&a, sizeof a) == 0 && listen(l, 4) == 0;
    struct acc x = {.fd = l};
    pthread_t t;
    pthread_create(&t, NULL, accepting, &x);
    usleep(300000);
    shutdown(l, SHUT_RDWR);
    for (int i = 0; i < 300 && x.at == 0; i++) usleep(10000);
    check("Unix: accept in einem anderen Faden endet bei shutdown des Lauschers (EINVAL)", ok && x.at != 0 && x.ret == -1 && x.err == EINVAL);
    if (x.at != 0) pthread_join(t, NULL);
    close(l);
    unlink(a.sun_path);
}

/* ---------------------------------------------------------------- listener goes away, read time limit */

static int listen_at(struct sockaddr_un *a, const char *tag) {
    a->sun_family = AF_UNIX;
    snprintf(a->sun_path, sizeof a->sun_path, "/tmp/weckertest-%s-%d.sock", tag, getpid());
    unlink(a->sun_path);
    int l = socket(AF_UNIX, SOCK_STREAM, 0);
    if (bind(l, (void *)a, sizeof *a) != 0 || listen(l, 4) != 0) return -1;
    return l;
}

static void gone_listener_checks(void) {
    struct sockaddr_un a;
    char b[8];
    /* 1. the listener is closed while a connection waits in its queue */
    int l = listen_at(&a, "zu");
    int c = socket(AF_UNIX, SOCK_STREAM, 0);
    int ok = l >= 0 && connect(c, (void *)&a, sizeof a) == 0;
    close(l);
    struct pollfd p = {.fd = c, .events = POLLIN};
    int pr = poll(&p, 1, 3000);
    errno = 0;
    ssize_t r = read(c, b, sizeof b);
    /* the Ultar kernel: end of file; Linux: ECONNRESET – either way the reader does not wait */
    check("Unix: Verbindung in der Warteschlange eines geschlossenen Lauschers liest Dateiende", ok && pr == 1 && (r == 0 || (r == -1 && errno == ECONNRESET)));
    close(c);
    unlink(a.sun_path);
    /* 2. the listener's process ends while a connection waits in its queue */
    int pipefd[2];
    if (pipe(pipefd) != 0) pipefd[0] = pipefd[1] = -1;
    pid_t kid = fork();
    if (kid == 0) {
        struct sockaddr_un k;
        int lk = listen_at(&k, "ende");
        char go = lk >= 0 ? 'y' : 'n';
        if (write(pipefd[1], &go, 1) != 1) _exit(2);
        usleep(500000);
        _exit(0); /* never accepts */
    }
    char go = 0;
    ok = read(pipefd[0], &go, 1) == 1 && go == 'y';
    snprintf(a.sun_path, sizeof a.sun_path, "/tmp/weckertest-ende-%d.sock", kid);
    c = socket(AF_UNIX, SOCK_STREAM, 0);
    ok = ok && connect(c, (void *)&a, sizeof a) == 0;
    long t0 = ms();
    errno = 0;
    r = read(c, b, sizeof b); /* blocks until the listener's process ended */
    long took = ms() - t0;
    waitpid(kid, NULL, 0);
    check("Unix: Verbindung in der Warteschlange liest Dateiende, wenn der Prozess des Lauschers endet",
          ok && (r == 0 || (r == -1 && errno == ECONNRESET)) && took < 5000);
    close(c);
    close(pipefd[0]);
    close(pipefd[1]);
    unlink(a.sun_path);
    /* 3. SO_RCVTIMEO: read and recvmsg end with EAGAIN after the limit; data before it is read; the next
     *    read gets the whole limit again */
    int sv[2];
    ok = socketpair(AF_UNIX, SOCK_STREAM, 0, sv) == 0;
    struct timeval tv = {.tv_sec = 0, .tv_usec = 400000}, back = {0};
    socklen_t bl = sizeof back;
    ok = ok && setsockopt(sv[0], SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof tv) == 0;
    ok = ok && getsockopt(sv[0], SOL_SOCKET, SO_RCVTIMEO, &back, &bl) == 0 && back.tv_usec == 400000 && back.tv_sec == 0;
    t0 = ms();
    errno = 0;
    r = read(sv[0], b, sizeof b);
    took = ms() - t0;
    check("Unix: SO_RCVTIMEO – read endet nach der Frist mit EAGAIN", ok && r == -1 && errno == EAGAIN && took >= 350 && took < 3000);
    if (write(sv[1], "x", 1) != 1) ok = 0;
    r = read(sv[0], b, sizeof b);
    t0 = ms();
    errno = 0;
    struct iovec io = {.iov_base = b, .iov_len = sizeof b};
    struct msghdr m = {.msg_iov = &io, .msg_iovlen = 1};
    ssize_t r2 = recvmsg(sv[0], &m, 0);
    took = ms() - t0;
    check("Unix: SO_RCVTIMEO – Daten vor der Frist, danach recvmsg wieder mit voller Frist (EAGAIN)",
          ok && r == 1 && r2 == -1 && errno == EAGAIN && took >= 350 && took < 3000);
    close(sv[0]);
    close(sv[1]);
}

/* ---------------------------------------------------------------- signalfd in a threaded process */

struct sig {
    int fd;
    int signo;
    long at;
};

static void *sigreader(void *arg) {
    struct sig *s = arg;
    struct signalfd_siginfo si;
    if (read(s->fd, &si, sizeof si) == sizeof si) s->signo = si.ssi_signo;
    s->at = ms();
    return NULL;
}

static void signal_checks(void) {
    sigset_t m;
    sigemptyset(&m);
    sigaddset(&m, SIGTERM);
    sigaddset(&m, SIGUSR2);
    pthread_sigmask(SIG_BLOCK, &m, NULL);
    int fd = signalfd(-1, &m, 0);
    struct sig s = {.fd = fd};
    pthread_t t;
    pthread_create(&t, NULL, sigreader, &s);
    usleep(300000);
    kill(getpid(), SIGUSR2);
    for (int i = 0; i < 300 && s.at == 0; i++) usleep(10000);
    check("signalfd: ein Signal an den Prozess weckt den lesenden Nebenfaden", s.at != 0 && s.signo == SIGUSR2);
    if (s.at != 0) pthread_join(t, NULL);
    kill(getpid(), SIGTERM);
    struct pollfd p = {fd, POLLIN, 0};
    struct signalfd_siginfo si;
    check("signalfd: poll im Hauptfaden sieht das Signal, read liefert SIGTERM",
          poll(&p, 1, 1000) == 1 && read(fd, &si, sizeof si) == sizeof si && si.ssi_signo == SIGTERM);
    close(fd);
}

/* ---------------------------------------------------------------- whole kernel lines from several cores */

/* 4 processes run execve on a file that is no program, 40 times each, at the same time: every attempt makes
 * the kernel print "[exec] <path>: <reason>" on whichever core runs it. In the kernel log every such line
 * must be whole – starting at a line start, with its path and reason, nothing of another line inside. */
static void console_lines(void) {
    const char *bad = "/tmp/weckertest-kein-programm";
    int fd = open(bad, O_WRONLY | O_CREAT | O_TRUNC, 0755);
    if (fd >= 0) {
        if (write(fd, "kein ELF, nur Text\n", 19) != 19) fails++;
        close(fd);
    }
    pid_t kids[4];
    for (int k = 0; k < 4; k++) {
        kids[k] = fork();
        if (kids[k] == 0) {
            char *av[] = {(char *)bad, NULL};
            for (int i = 0; i < 40; i++) execve(bad, av, environ);
            _exit(0);
        }
    }
    for (int k = 0; k < 4; k++) waitpid(kids[k], NULL, 0);
    unlink(bad);
    static char log[65536 * 2];
    int lf = open("/proc/ultar/protokoll", O_RDONLY);
    long n = 0, k;
    while (lf >= 0 && n < (long)sizeof log - 1 && (k = read(lf, log + n, sizeof log - 1 - n)) > 0) n += k;
    if (lf >= 0) close(lf);
    log[n] = 0;
    int seen = 0, broken = 0;
    char want[128];
    snprintf(want, sizeof want, "[exec] %s: ", bad);
    for (char *p = strstr(log, bad); p; p = strstr(p + 1, bad)) {
        seen++;
        char *start = p - 7; /* "[exec] " */
        char *end = strchr(p, '\n');
        if (start < log || strncmp(start, want, strlen(want)) != 0 || (start > log && start[-1] != '\n')) broken++;
        else if (!end || (strchr(p + strlen(bad) + 2, '[') && strchr(p + strlen(bad) + 2, '[') < end)) broken++;
    }
    say("--     Konsole: %d Zeilen [exec] gefunden, %d zerrissen\n", seen, broken);
    check("Konsole: Kernzeilen von mehreren Kernen zugleich bleiben ganz (Protokoll)", seen >= 40 && broken == 0);
}

/* ---------------------------------------------------------------- SIGKILL while the victim is about to block */

/* 300 times: a child tells its parent it is there and goes straight into pause(); the parent sends SIGKILL at
 * once, while the child is still on its way into the kernel's wait on another core. Every child must end. */
static void kill_race(void) {
    int stuck = 0, rounds = 300;
    for (int i = 0; i < rounds; i++) {
        int sync[2];
        if (pipe(sync)) break;
        pid_t c = fork();
        if (c == 0) {
            if (write(sync[1], "x", 1) != 1) _exit(1);
            for (;;) pause();
        }
        char b;
        if (read(sync[0], &b, 1) != 1) stuck++;
        kill(c, SIGKILL);
        int st = 0, gone = 0;
        for (int w = 0; w < 200 && !gone; w++) {
            if (waitpid(c, &st, WNOHANG) == c) gone = 1;
            else usleep(10000);
        }
        if (!gone) stuck++;
        close(sync[0]);
        close(sync[1]);
    }
    say("--     SIGKILL beim Blockieren: %d von %d Kindern blieben hängen\n", stuck, rounds);
    check("SIGKILL: ein Kind, das gerade in pause() geht, endet immer (300 Runden)", stuck == 0);
}

int main(int argc, char **argv) {
    if (argc > 2 && strcmp(argv[1], "--sleep") == 0) {
        sleep(atoi(argv[2]));
        return 0;
    }
    ssize_t k = readlink("/proc/self/exe", self, sizeof self - 1);
    if (k > 0) self[k] = 0;
    say("WECKERTEST: posix_spawn/vfork, Unix-Sockel aufwecken, signalfd mit Fäden\n");
    spawn_checks();
    unix_checks();
    readiness_checks();
    accept_checks();
    gone_listener_checks();
    signal_checks();
    console_lines();
    kill_race();
    say("WECKERTEST: %d Fehler\n", fails);
    say("WECKERTEST: %s\n", fails == 0 ? "ALLES OK" : "SIEHE OBEN");
    return fails != 0;
}
