// zellnetz – network isolation of cells on the Ultar kernel (docs/integration/zell-netz.md).
//
//   zellnetz                 every check; verdict line "ZELLNETZ: ALLES OK"
//   zellnetz --guard-check   (internal) exits 0 if the caller carries the barrier – run after execve
//
// QEMU user network with a public test range (tools/kern-test/zell-netz-e2e.sh):
//   net=198.51.100.0/24 (device .15, router .2, DNS .3), guestfwd 198.51.100.100:8080 -> "INTERNET-OK"
// so "the internet" is an address the barrier lets through, without leaving the test machine.
#define _GNU_SOURCE
#include <arpa/inet.h>
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <netinet/in.h>
#include <poll.h>
#include <sched.h>
#include <stddef.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/prctl.h>
#include <sys/socket.h>
#include <sys/syscall.h>
#include <sys/un.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#define PR_ULTAR_RIGHTS_DROP 0x554c5201UL
#define PR_ULTAR_NET_GUARD 0x554c5202UL
#define PR_ULTAR_NET_GUARD_GET 0x554c5203UL
#define PR_ULTAR_NET_NS_GET 0x554c5204UL
#define RIGHT_MOUNT 1UL
#define HOST_TCP 7001   // a service on the host's loopback
#define HOST_UDP 7002
#define CELL_TCP 7003   // a service inside a cell
#define NET_IP "198.51.100.100"
#define NET_PORT 8080
#define ABSTRACT "zellnetz-host"

static int fails;
static char own_ip[32], dns_ip[32];

#define CHECK(c, ...)                                                                                                   \
    do {                                                                                                                \
        if (c) {                                                                                                        \
            printf("ok    ");                                                                                           \
        } else {                                                                                                        \
            printf("FEHLER ");                                                                                          \
            fails++;                                                                                                    \
        }                                                                                                               \
        printf(__VA_ARGS__);                                                                                            \
        printf("\n");                                                                                                   \
        fflush(stdout);                                                                                                 \
    } while (0)

// ------------------------------------------------------------------ helpers

static struct sockaddr_in in4(const char *ip, int port) {
    struct sockaddr_in a = {.sin_family = AF_INET, .sin_port = htons(port)};
    inet_pton(AF_INET, ip, &a.sin_addr);
    return a;
}

// connect a TCP socket: 0, or the errno; the socket stays open in *out (if given)
static int tcp_to(const char *ip, int port, int *out) {
    int s = socket(AF_INET, SOCK_STREAM, 0);
    struct sockaddr_in a = in4(ip, port);
    int r = connect(s, (void *)&a, sizeof a) == 0 ? 0 : errno;
    if (out && r == 0) *out = s;
    else close(s);
    return r;
}

// connect within `ms`: 0, the errno, or ETIMEDOUT (a dropped SYN – the address is closed, but silently)
static int tcp_within(const char *ip, int port, int ms) {
    int s = socket(AF_INET, SOCK_STREAM | SOCK_NONBLOCK, 0);
    struct sockaddr_in a = in4(ip, port);
    int r = connect(s, (void *)&a, sizeof a) == 0 ? 0 : errno;
    if (r == EINPROGRESS) {
        struct pollfd p = {.fd = s, .events = POLLOUT};
        socklen_t len = sizeof r;
        r = poll(&p, 1, ms) > 0 && getsockopt(s, SOL_SOCKET, SO_ERROR, &r, &len) == 0 ? r : ETIMEDOUT;
    }
    close(s);
    return r;
}

// one UDP datagram: 0, or the errno
static int udp_to(const char *ip, int port) {
    int s = socket(AF_INET, SOCK_DGRAM, 0);
    struct sockaddr_in a = in4(ip, port);
    int r = sendto(s, "x", 1, 0, (void *)&a, sizeof a) == 1 ? 0 : errno;
    close(s);
    return r;
}

// bind (and listen, for TCP) a socket at ip:port; the socket, or -errno
static int serve(int type, const char *ip, int port) {
    int s = socket(AF_INET, type, 0);
    struct sockaddr_in a = in4(ip, port);
    if (bind(s, (void *)&a, sizeof a) != 0 || (type == SOCK_STREAM && listen(s, 4) != 0)) {
        int e = errno;
        close(s);
        return -e;
    }
    return s;
}

// read a line within `ms`; returns the length or -1
static int read_line(int fd, char *buf, size_t n, int ms) {
    struct pollfd p = {.fd = fd, .events = POLLIN};
    if (poll(&p, 1, ms) <= 0) return -1;
    ssize_t k = read(fd, buf, n - 1);
    if (k <= 0) return -1;
    buf[k] = 0;
    return (int)k;
}

static const char *err(int e) {
    return e ? strerror(e) : "ok";
}

// the internet test service answers "INTERNET-OK"
static int reach_internet(void) {
    int s = -1;
    if (tcp_to(NET_IP, NET_PORT, &s) != 0) return 0;
    char b[64];
    int k = read_line(s, b, sizeof b, 5000);
    close(s);
    return k > 0 && strstr(b, "INTERNET-OK") != NULL;
}

// an abstract Unix name: 0 on connect, else the errno
static int abstract_connect(void) {
    int s = socket(AF_UNIX, SOCK_STREAM, 0);
    struct sockaddr_un a = {.sun_family = AF_UNIX};
    memcpy(a.sun_path + 1, ABSTRACT, strlen(ABSTRACT));
    int r = connect(s, (void *)&a, offsetof(struct sockaddr_un, sun_path) + 1 + strlen(ABSTRACT)) == 0 ? 0 : errno;
    close(s);
    return r;
}

static int abstract_listen(void) {
    int s = socket(AF_UNIX, SOCK_STREAM, 0);
    struct sockaddr_un a = {.sun_family = AF_UNIX};
    memcpy(a.sun_path + 1, ABSTRACT, strlen(ABSTRACT));
    if (bind(s, (void *)&a, offsetof(struct sockaddr_un, sun_path) + 1 + strlen(ABSTRACT)) != 0 || listen(s, 4) != 0) return -1;
    return s;
}

// run fn in a child, add its failures
static void in_child(void (*fn)(void)) {
    fflush(stdout);
    pid_t c = fork();
    if (c == 0) {
        alarm(60);
        fails = 0;
        fn();
        fflush(stdout);
        _exit(fails > 100 ? 100 : fails);
    }
    int st = 0;
    if (c < 0 || waitpid(c, &st, 0) != c || !WIFEXITED(st)) {
        CHECK(0, "child ended normally (status %d)", st);
        return;
    }
    fails += WEXITSTATUS(st);
}

// ------------------------------------------------------------------ the device

static int lease(void) {
    DIR *d = opendir("/sys/class/net");
    struct dirent *e;
    char path[300], text[1024];
    for (int round = 0; round < 200; round++) {
        if (d) rewinddir(d);
        while (d && (e = readdir(d))) {
            if (e->d_name[0] == '.') continue;
            snprintf(path, sizeof path, "/sys/class/net/%s/ultar_ipv4", e->d_name);
            int fd = open(path, O_RDONLY);
            if (fd < 0) continue;
            ssize_t k = read(fd, text, sizeof text - 1);
            close(fd);
            text[k > 0 ? k : 0] = 0;
            char *a = strstr(text, "address="), *n = strstr(text, "dns=");
            if (strstr(text, "state=bound") && a && n) {
                sscanf(a + 8, "%31[0-9.]", own_ip);
                sscanf(n + 4, "%31[0-9.]", dns_ip);
                closedir(d);
                return 1;
            }
        }
        // a socket call drives the network poll
        int u = socket(AF_INET, SOCK_DGRAM | SOCK_NONBLOCK, 0);
        char b[1];
        recv(u, b, 1, 0);
        close(u);
        usleep(100000);
    }
    if (d) closedir(d);
    return 0;
}

// ------------------------------------------------------------------ cells

// a network namespace without the barrier: its own loopback, nothing else
static void cell_without_way_out(void) {
    CHECK(unshare(CLONE_NEWNET) == 0, "unshare(CLONE_NEWNET)");
    CHECK(prctl(PR_ULTAR_NET_NS_GET, 0, 0, 0, 0) > 0, "own network namespace number %ld", (long)prctl(PR_ULTAR_NET_NS_GET, 0, 0, 0, 0));
    int r = tcp_to("127.0.0.1", HOST_TCP, NULL);
    CHECK(r == ECONNREFUSED, "the host's loopback service is not reachable (%s)", err(r));
    r = tcp_to(NET_IP, NET_PORT, NULL);
    CHECK(r == ENETUNREACH, "no way out to the internet (%s)", err(r));
    r = udp_to(dns_ip, 53);
    CHECK(r == ENETUNREACH, "no DNS either (%s)", err(r));
    r = abstract_connect();
    CHECK(r == ECONNREFUSED, "the host's abstract Unix name is not reachable (%s)", err(r));
    r = udp_to("127.0.0.1", HOST_UDP);
    CHECK(r == 0, "a datagram to 127.0.0.1:%d stays in the cell's own loopback (%s)", HOST_UDP, err(r));
    // own loopback: the same port as the host's service is free here
    int l = serve(SOCK_STREAM, "127.0.0.1", HOST_TCP);
    CHECK(l >= 0, "own service on 127.0.0.1:%d (%s)", HOST_TCP, err(l < 0 ? -l : 0));
    int c = -1;
    r = tcp_to("127.0.0.1", HOST_TCP, &c);
    int a = r == 0 ? accept(l, NULL, NULL) : -1;
    char b[32] = {0};
    if (a >= 0 && write(c, "CELL-LO\n", 8) == 8) read_line(a, b, sizeof b, 3000);
    CHECK(a >= 0 && !strncmp(b, "CELL-LO", 7), "own loopback carries data (%s)", err(r));
    close(a);
    close(c);
    close(l);
}

// a network namespace with the barrier: own loopback, and the internet – nothing of the device or the LAN
static void cell_internet_only(void) {
    CHECK(unshare(CLONE_NEWNET) == 0 && prctl(PR_ULTAR_NET_GUARD, 0, 0, 0, 0) == 0, "own namespace and the barrier");
    CHECK(prctl(PR_ULTAR_NET_GUARD_GET, 0, 0, 0, 0) == 1, "the barrier is set");
    CHECK(reach_internet(), "the internet answers (%s:%d)", NET_IP, NET_PORT);
    int r = tcp_to("127.0.0.1", HOST_TCP, NULL);
    CHECK(r == ECONNREFUSED, "127.0.0.1 is the cell's own loopback, not the host's (%s)", err(r));
    r = tcp_to(own_ip, 22, NULL);
    CHECK(r == EACCES, "the device's own address %s is refused (%s)", own_ip, err(r));
    const char *lan[] = {"192.168.1.1", "10.1.2.3", "172.20.0.1", "169.254.1.1", "100.64.0.1"};
    for (unsigned i = 0; i < sizeof lan / sizeof *lan; i++) {
        r = tcp_to(lan[i], 80, NULL);
        CHECK(r == EACCES, "LAN address %s is refused (%s)", lan[i], err(r));
    }
    const char *bc[] = {"255.255.255.255", "224.0.0.251", "198.51.100.255"};
    for (unsigned i = 0; i < sizeof bc / sizeof *bc; i++) {
        r = udp_to(bc[i], 5353);
        CHECK(r == EACCES, "broadcast/multicast %s is refused (%s)", bc[i], err(r));
    }
    r = udp_to(dns_ip, 53);
    CHECK(r == 0, "DNS server %s:53 is allowed (%s)", dns_ip, err(r));
    // answers come back into the namespace (UDP: a DNS reply; TCP: the internet test above)
    int u = socket(AF_INET, SOCK_DGRAM, 0);
    unsigned char q[] = {0x5a, 0x4e, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 6, 'g', 'i', 't', 'h', 'u', 'b', 3, 'c', 'o', 'm', 0, 0, 1, 0, 1};
    struct sockaddr_in d = in4(dns_ip, 53);
    unsigned char ans[512];
    int got = 0;
    for (int i = 0; i < 3 && !got; i++) {
        sendto(u, q, sizeof q, 0, (void *)&d, sizeof d);
        struct pollfd p = {.fd = u, .events = POLLIN};
        if (poll(&p, 1, 3000) > 0 && recv(u, ans, sizeof ans, 0) > 12 && ans[0] == 0x5a && ans[1] == 0x4e) got = 1;
    }
    close(u);
    CHECK(got, "a DNS answer reaches the cell");
}

// a service inside a cell (with the barrier, listening on every address) is reachable neither through the
// host's loopback nor through the device's address
static void cell_service_hidden(void) {
    int up[2], go[2];
    CHECK(pipe(up) == 0 && pipe(go) == 0, "pipes");
    fflush(stdout);
    pid_t c = fork();
    if (c == 0) {
        alarm(30);
        // the service really listens: the cell itself reaches it
        int ok = unshare(CLONE_NEWNET) == 0 && prctl(PR_ULTAR_NET_GUARD, 0, 0, 0, 0) == 0 && serve(SOCK_STREAM, "0.0.0.0", CELL_TCP) >= 0 &&
                 tcp_to("127.0.0.1", CELL_TCP, NULL) == 0;
        char b = ok ? 'R' : 'F';
        if (write(up[1], &b, 1) != 1 || read(go[0], &b, 1) < 0) _exit(1);
        _exit(0);
    }
    char b = 0;
    CHECK(read(up[0], &b, 1) == 1 && b == 'R', "a cell serves on 0.0.0.0:%d in its own namespace (and reaches it itself)", CELL_TCP);
    int r = tcp_to("127.0.0.1", CELL_TCP, NULL);
    CHECK(r == ECONNREFUSED, "the cell's service is not on the host's loopback (%s)", err(r));
    // the host answers its own closed address silently (no reset for a non-loopback source): either way the
    // cell's listener is not reached
    r = tcp_within(own_ip, CELL_TCP, 3000);
    CHECK(r == ECONNREFUSED || r == ETIMEDOUT, "nor on the device's address %s (%s)", own_ip, err(r));
    if (write(go[1], "x", 1) != 1) fails++;
    waitpid(c, NULL, 0);
    close(up[0]), close(up[1]), close(go[0]), close(go[1]);
}

// the barrier in the host namespace: no host loopback, no listening, no fixed port
static void host_with_barrier(void) {
    CHECK(prctl(PR_ULTAR_NET_GUARD, 0, 0, 0, 0) == 0, "the barrier, staying in the host namespace");
    int r = tcp_to("127.0.0.1", HOST_TCP, NULL);
    CHECK(r == EACCES, "the host's loopback service is refused (%s)", err(r));
    r = tcp_to(own_ip, 22, NULL);
    CHECK(r == EACCES, "the device's own address is refused (%s)", err(r));
    int l = serve(SOCK_STREAM, "0.0.0.0", 7004);
    CHECK(l == -EACCES, "binding a fixed port is refused (%s)", err(l < 0 ? -l : 0));
    l = serve(SOCK_STREAM, "0.0.0.0", 0);
    CHECK(l == -EACCES, "listening is refused (%s)", err(l < 0 ? -l : 0));
    int s = socket(AF_INET, SOCK_DGRAM, 0);
    struct sockaddr_in any = in4("0.0.0.0", 0);
    CHECK(bind(s, (void *)&any, sizeof any) == 0, "an outgoing socket may bind 0.0.0.0:0");
    close(s);
    CHECK(reach_internet(), "the internet answers");
}

// the barrier cannot be lost: not by fork, not by execve
static void barrier_stays(void) {
    CHECK(prctl(PR_ULTAR_NET_GUARD, 0, 0, 0, 0) == 0, "set the barrier");
    pid_t c = fork();
    if (c == 0) {
        execl("/bin/zellnetz", "zellnetz", "--guard-check", (char *)NULL);
        _exit(99);
    }
    int st = 0;
    waitpid(c, &st, 0);
    CHECK(WIFEXITED(st) && WEXITSTATUS(st) == 0, "a child after fork and execve still carries it");
}

// creating a namespace needs the MOUNT right; clone(CLONE_NEWNET) gives the child its own
static void namespace_rules(void) {
    long host_ns = prctl(PR_ULTAR_NET_NS_GET, 0, 0, 0, 0);
    pid_t c = (pid_t)syscall(SYS_clone, CLONE_NEWNET | SIGCHLD, 0, 0, 0, 0);
    if (c == 0) _exit(prctl(PR_ULTAR_NET_NS_GET, 0, 0, 0, 0) != host_ns ? 0 : 1);
    int st = 0;
    waitpid(c, &st, 0);
    CHECK(c > 0 && WIFEXITED(st) && WEXITSTATUS(st) == 0, "clone(CLONE_NEWNET) starts the child in a new namespace");
    CHECK(prctl(PR_ULTAR_RIGHTS_DROP, RIGHT_MOUNT, 0, 0, 0) == 0, "drop the MOUNT right");
    errno = 0;
    CHECK(unshare(CLONE_NEWNET) == -1 && errno == EPERM, "unshare(CLONE_NEWNET) without it -> %s", strerror(errno));
}

// ------------------------------------------------------------------ PR_SET_PDEATHSIG

// parent -> child -> grandchild; the grandchild asks for `sig` (0: none) and reports on `w`
static pid_t orphan(int w, int sig) {
    pid_t mid = fork();
    if (mid == 0) {
        // the parent ends only once the grandchild asked for its signal (a signal asked for after the
        // parent's end never comes, as on Linux)
        int ready[2];
        if (pipe(ready) != 0) _exit(2);
        pid_t g = fork();
        if (g == 0) {
            int got = -1;
            close(ready[0]);
            if (sig) prctl(PR_SET_PDEATHSIG, sig, 0, 0, 0);
            prctl(PR_GET_PDEATHSIG, (unsigned long)&got, 0, 0, 0);
            char line[32];
            int n = snprintf(line, sizeof line, "%d %d\n", (int)getpid(), got);
            if (write(w, line, n) != n || write(ready[1], "r", 1) != 1) _exit(2);
            close(ready[1]);
            for (;;) pause();
        }
        close(ready[1]);
        char c;
        if (read(ready[0], &c, 1) != 1) _exit(3);
        _exit(0); // then at once: the grandchild is the orphan
    }
    waitpid(mid, NULL, 0);
    return mid;
}

static void parent_death_signal(void) {
    int p[2];
    CHECK(pipe(p) == 0, "pipe");
    orphan(p[1], SIGTERM);
    close(p[1]);
    char line[32];
    int pid = 0, got = -1;
    CHECK(read_line(p[0], line, sizeof line, 3000) > 0 && sscanf(line, "%d %d", &pid, &got) == 2 && got == SIGTERM, "PR_GET_PDEATHSIG reads SIGTERM back");
    // the grandchild held the only other write end: end of file = it ended
    char b[8];
    int eof = read_line(p[0], b, sizeof b, 5000) == -1;
    // its descriptors close before it leaves the process table: give the end a moment under load
    for (int i = 0; i < 40 && kill(pid, 0) == 0; i++) usleep(50000);
    CHECK(eof && kill(pid, 0) == -1, "the orphan ended with its parent (SIGTERM)");
    close(p[0]);
    CHECK(pipe(p) == 0, "pipe");
    orphan(p[1], 0);
    close(p[1]);
    pid = 0;
    CHECK(read_line(p[0], line, sizeof line, 3000) > 0 && sscanf(line, "%d %d", &pid, &got) == 2 && got == 0, "a child starts without a parent-death signal");
    usleep(500000);
    CHECK(pid > 0 && kill(pid, 0) == 0, "without it the orphan lives on");
    if (pid > 0) kill(pid, SIGKILL);
    close(p[0]);
}

// ------------------------------------------------------------------ main

int main(int argc, char **argv) {
    if (argc > 1 && !strcmp(argv[1], "--guard-check")) return prctl(PR_ULTAR_NET_GUARD_GET, 0, 0, 0, 0) == 1 ? 0 : 1;
    setvbuf(stdout, NULL, _IOLBF, 0);
    CHECK(lease(), "DHCP lease: device %s, DNS %s", own_ip, dns_ip);
    int host_tcp = serve(SOCK_STREAM, "127.0.0.1", HOST_TCP);
    int host_udp = serve(SOCK_DGRAM, "127.0.0.1", HOST_UDP);
    int host_abs = abstract_listen();
    CHECK(host_tcp >= 0 && host_udp >= 0 && host_abs >= 0, "host services on 127.0.0.1:%d/%d and @%s", HOST_TCP, HOST_UDP, ABSTRACT);
    CHECK(tcp_to("127.0.0.1", HOST_TCP, NULL) == 0 && abstract_connect() == 0, "the host itself reaches them");
    CHECK(reach_internet(), "the host reaches the internet test service");
    in_child(cell_without_way_out);
    in_child(cell_internet_only);
    cell_service_hidden();
    in_child(host_with_barrier);
    in_child(barrier_stays);
    CHECK(prctl(PR_ULTAR_NET_GUARD_GET, 0, 0, 0, 0) == 0, "the host itself never got the barrier");
    in_child(namespace_rules);
    in_child(parent_death_signal);
    // nothing from a cell arrived at the host's UDP service
    char b[8];
    CHECK(recv(host_udp, b, sizeof b, MSG_DONTWAIT) == -1 && errno == EAGAIN, "no datagram from a cell reached the host's UDP service");
    printf(fails ? "ZELLNETZ: FEHLER (%d)\n" : "ZELLNETZ: ALLES OK\n", fails);
    return fails != 0;
}
