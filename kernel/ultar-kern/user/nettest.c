// nettest – the wired network on the Ultar kernel, as the network service sees it.
//
//   nettest <driver> <host-port> [dns-name]   detection, link, DHCP lease, DNS, TCP to the host,
//                                             renew, static address and back to DHCP
//   nettest link <0|1> [driver]               wait (20 s) until the interface's carrier is 0 / 1
//                                             (with 1 also until it holds a DHCP lease again)
//
// Interfaces come from /sys/class/net (Linux layout: address, carrier, operstate, speed, type) and
// /sys/class/net/<if>/ultar_ipv4 (address state; writes: dhcp, renew, static …). QEMU user network:
// router 10.0.2.2 (the host), DNS 10.0.2.3. Verdict line: "NETTEST: ALLES OK" / "NETTEST-LINK: ALLES OK".
#include <arpa/inet.h>
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <netinet/in.h>
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <time.h>
#include <unistd.h>

static int fails;
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

static int readf(const char *path, char *buf, size_t n) {
    int fd = open(path, O_RDONLY);
    if (fd < 0) return -1;
    ssize_t k = read(fd, buf, n - 1);
    close(fd);
    if (k < 0) return -1;
    buf[k] = 0;
    return (int)k;
}

static int writef(const char *path, const char *text) {
    int fd = open(path, O_WRONLY);
    if (fd < 0) return -errno;
    ssize_t k = write(fd, text, strlen(text));
    int e = errno;
    close(fd);
    return k < 0 ? -e : 0;
}

static void attr(const char *ifn, const char *name, char *out, size_t n) {
    char p[256];
    snprintf(p, sizeof p, "/sys/class/net/%s/%s", ifn, name);
    if (readf(p, out, n) < 0) out[0] = 0;
    char *nl = strchr(out, '\n');
    if (nl && nl[1] == 0) *nl = 0;
}

// value of `key=` in the ultar_ipv4 text
static int field(const char *text, const char *key, char *out, size_t n) {
    size_t kl = strlen(key);
    for (const char *l = text; l && *l;) {
        if (!strncmp(l, key, kl) && l[kl] == '=') {
            const char *v = l + kl + 1, *e = strchr(v, '\n');
            size_t len = e ? (size_t)(e - v) : strlen(v);
            if (len >= n) len = n - 1;
            memcpy(out, v, len);
            out[len] = 0;
            return 1;
        }
        l = strchr(l, '\n');
        if (l) l++;
    }
    out[0] = 0;
    return 0;
}

static double now_s(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return t.tv_sec + t.tv_nsec / 1e9;
}

// the interface whose driver is `drv` (NULL: the first with a carrier)
static int find_if(const char *drv, char *ifn, size_t n) {
    DIR *d = opendir("/sys/class/net");
    if (!d) return 0;
    struct dirent *e;
    int found = 0;
    while (!found && (e = readdir(d))) {
        if (e->d_name[0] == '.') continue;
        char v[64];
        attr(e->d_name, drv ? "ultar_driver" : "carrier", v, sizeof v);
        if (drv ? !strcmp(v, drv) : !strcmp(v, "1")) {
            snprintf(ifn, n, "%s", e->d_name);
            found = 1;
        }
    }
    closedir(d);
    return found;
}

static int wait_state(const char *ifn, const char *state, double secs, char *ipv4, size_t n) {
    char p[256], st[32];
    snprintf(p, sizeof p, "/sys/class/net/%s/ultar_ipv4", ifn);
    double end = now_s() + secs;
    do {
        if (readf(p, ipv4, n) > 0 && field(ipv4, "state", st, sizeof st) && !strcmp(st, state)) return 1;
        usleep(100000);
    } while (now_s() < end);
    return 0;
}

// a DNS question for an A record; returns the answer's rcode (>= 0), -1 without an answer
static int dns_query(const char *server, const char *name, int *answers, char *first, size_t fn) {
    unsigned char q[512], r[1500];
    int len = 12;
    memset(q, 0, sizeof q);
    q[0] = 0x55;
    q[1] = 0x4c;
    q[2] = 0x01; // recursion desired
    q[5] = 1;    // one question
    const char *s = name;
    while (*s) {
        const char *dot = strchr(s, '.');
        int l = dot ? (int)(dot - s) : (int)strlen(s);
        q[len++] = (unsigned char)l;
        memcpy(q + len, s, l);
        len += l;
        s += l + (dot ? 1 : 0);
    }
    q[len++] = 0;
    q[len++] = 0;
    q[len++] = 1; // A
    q[len++] = 0;
    q[len++] = 1; // IN
    int u = socket(AF_INET, SOCK_DGRAM, 0);
    struct sockaddr_in a = {.sin_family = AF_INET, .sin_port = htons(53)};
    inet_pton(AF_INET, server, &a.sin_addr);
    int rc = -1;
    for (int attempt = 0; attempt < 3 && rc < 0; attempt++) {
        sendto(u, q, len, 0, (void *)&a, sizeof a);
        struct pollfd pf = {.fd = u, .events = POLLIN};
        if (poll(&pf, 1, 3000) <= 0) continue;
        ssize_t k = recv(u, r, sizeof r, 0);
        if (k < 12 || r[0] != 0x55 || r[1] != 0x4c || !(r[2] & 0x80)) continue;
        rc = r[3] & 0x0f;
        *answers = r[6] << 8 | r[7];
        // first A record: skip the question, then walk the answers
        int p = 12;
        while (p < k && r[p]) p += r[p] + 1;
        p += 5;
        first[0] = 0;
        for (int i = 0; i < *answers && p + 12 <= k; i++) {
            if ((r[p] & 0xc0) == 0xc0) p += 2;
            else {
                while (p < k && r[p]) p += r[p] + 1;
                p++;
            }
            int type = r[p] << 8 | r[p + 1], rdlen = r[p + 8] << 8 | r[p + 9];
            p += 10;
            if (type == 1 && rdlen == 4 && p + 4 <= k) {
                snprintf(first, fn, "%d.%d.%d.%d", r[p], r[p + 1], r[p + 2], r[p + 3]);
                break;
            }
            p += rdlen;
        }
    }
    close(u);
    return rc;
}

// TCP to the host's test listener: send a line, expect "OK <line>"
static int tcp_host(int port, const char *line) {
    int s = socket(AF_INET, SOCK_STREAM, 0);
    struct sockaddr_in a = {.sin_family = AF_INET, .sin_port = htons(port)};
    inet_pton(AF_INET, "10.0.2.2", &a.sin_addr);
    if (connect(s, (void *)&a, sizeof a)) {
        printf("       connect: %s\n", strerror(errno));
        close(s);
        return 0;
    }
    char out[128], in[256] = {0};
    snprintf(out, sizeof out, "%s\n", line);
    write(s, out, strlen(out));
    size_t got = 0;
    double end = now_s() + 10;
    while (got < sizeof in - 1 && !strchr(in, '\n') && now_s() < end) {
        struct pollfd pf = {.fd = s, .events = POLLIN};
        if (poll(&pf, 1, 1000) <= 0) continue;
        ssize_t k = read(s, in + got, sizeof in - 1 - got);
        if (k <= 0) break;
        got += k;
    }
    close(s);
    char want[160];
    snprintf(want, sizeof want, "OK %s\n", line);
    return !strcmp(in, want);
}

static int link_mode(int want, const char *drv) {
    char ifn[64], v[32], ipv4[1024];
    if (!find_if(drv, ifn, sizeof ifn)) {
        CHECK(0, "interface with driver %s", drv ? drv : "(any)");
        return 1;
    }
    double t0 = now_s(), end = t0 + 20;
    do {
        attr(ifn, "carrier", v, sizeof v);
        if (atoi(v) == want) break;
        usleep(100000);
    } while (now_s() < end);
    char op[32], lc[32];
    attr(ifn, "operstate", op, sizeof op);
    attr(ifn, "ultar_link_changes", lc, sizeof lc);
    CHECK(atoi(v) == want, "%s: carrier %s operstate %s after %.1f s (link changes %s)", ifn, v, op, now_s() - t0, lc);
    if (want) {
        int bound = wait_state(ifn, "bound", 20, ipv4, sizeof ipv4);
        char addr[64];
        field(ipv4, "address", addr, sizeof addr);
        CHECK(bound && addr[0], "%s: DHCP lease again after the cable came back (%s)", ifn, addr);
    }
    printf("NETTEST-LINK: %s\n", fails ? "FEHLER" : "ALLES OK");
    return fails != 0;
}

int main(int argc, char **argv) {
    setvbuf(stdout, NULL, _IOLBF, 0);
    if (argc >= 3 && !strcmp(argv[1], "link")) return link_mode(atoi(argv[2]), argc > 3 ? argv[3] : NULL);
    if (argc < 3) {
        fprintf(stderr, "nettest <driver> <host-port> [dns-name] | nettest link <0|1> [driver]\n");
        return 2;
    }
    const char *drv = argv[1];
    int port = atoi(argv[2]);
    const char *name = argc > 3 ? argv[3] : "github.com";

    // 0. a device that was just plugged in (USB hot-plug) is bound at the kernel's next network poll:
    //    wait up to 15 s for the interface of the driver under test; a socket call drives the poll
    {
        char w[64];
        for (int i = 0; i < 75 && !find_if(drv, w, sizeof w); i++) {
            int u = socket(AF_INET, SOCK_DGRAM | SOCK_NONBLOCK, 0);
            if (u >= 0) {
                char b[4];
                recv(u, b, sizeof b, 0);
                close(u);
            }
            usleep(200000);
        }
    }

    // 1. every interface as Linux shows it
    DIR *d = opendir("/sys/class/net");
    CHECK(d != NULL, "/sys/class/net is listed");
    struct dirent *e;
    int count = 0;
    while (d && (e = readdir(d))) {
        if (e->d_name[0] == '.') continue;
        char mac[32], car[8], op[16], sp[16], ty[8], dr[32], pci[32];
        attr(e->d_name, "address", mac, sizeof mac);
        attr(e->d_name, "carrier", car, sizeof car);
        attr(e->d_name, "operstate", op, sizeof op);
        attr(e->d_name, "speed", sp, sizeof sp);
        attr(e->d_name, "type", ty, sizeof ty);
        attr(e->d_name, "ultar_driver", dr, sizeof dr);
        attr(e->d_name, "ultar_pci", pci, sizeof pci);
        printf("       %s: driver %s pci %s mac %s carrier %s operstate %s speed %s type %s\n", e->d_name, dr, pci, mac, car, op, sp, ty);
        CHECK(strlen(mac) == 17 && strcmp(mac, "00:00:00:00:00:00") && !strcmp(ty, "1"), "%s: MAC and type", e->d_name);
        count++;
    }
    if (d) closedir(d);
    CHECK(count > 0, "%d interface(s)", count);

    // 2. the interface of the driver under test: link, speed, lease
    char ifn[64], v[64], ipv4[1024];
    if (!find_if(drv, ifn, sizeof ifn)) {
        CHECK(0, "an interface with driver %s", drv);
        printf("NETTEST: FEHLER\n");
        return 1;
    }
    attr(ifn, "carrier", v, sizeof v);
    CHECK(!strcmp(v, "1"), "%s (%s): carrier 1", ifn, drv);
    attr(ifn, "operstate", v, sizeof v);
    CHECK(!strcmp(v, "up"), "%s: operstate up", ifn);
    attr(ifn, "speed", v, sizeof v);
    printf("       %s: speed %s Mbit/s\n", ifn, v);
    int bound = wait_state(ifn, "bound", 20, ipv4, sizeof ipv4);
    char addr[64], gw[64], dns[64], lease[32], active[8];
    field(ipv4, "address", addr, sizeof addr);
    field(ipv4, "gateway", gw, sizeof gw);
    field(ipv4, "dns", dns, sizeof dns);
    field(ipv4, "lease", lease, sizeof lease);
    field(ipv4, "active", active, sizeof active);
    CHECK(bound && !strncmp(addr, "10.0.2.", 7) && !strcmp(gw, "10.0.2.2") && dns[0] && atoi(lease) > 0,
          "%s: DHCP lease %s router %s dns %s for %s s", ifn, addr, gw, dns, lease);
    CHECK(!strcmp(active, "1"), "%s carries the default route", ifn);

    // 3. DNS through the leased server
    char server[32], first[32] = "";
    snprintf(server, sizeof server, "%s", dns);
    char *sp = strchr(server, ' ');
    if (sp) *sp = 0;
    int answers = 0, rc = dns_query(server, name, &answers, first, sizeof first);
    CHECK(rc >= 0, "DNS %s answered for %s: rcode %d, %d answer(s)%s%s", server, name, rc, answers, first[0] ? ", first " : "",
          first);

    // 4. TCP to the host through the router
    CHECK(tcp_host(port, "ultar-dhcp"), "TCP to the host 10.0.2.2:%d (address from DHCP)", port);

    // 5. renew: the lease is asked for again and stays
    char path[256];
    snprintf(path, sizeof path, "/sys/class/net/%s/ultar_ipv4", ifn);
    CHECK(writef(path, "renew") == 0, "renew accepted");
    usleep(200000);
    bound = wait_state(ifn, "bound", 15, ipv4, sizeof ipv4);
    char addr2[64];
    field(ipv4, "address", addr2, sizeof addr2);
    CHECK(bound && !strcmp(addr, addr2), "lease renewed, address kept (%s)", addr2);

    // 6. a wrong command is refused, a static address works
    CHECK(writef(path, "static 10.0.2.300/24") == -EINVAL, "a bad address is refused (EINVAL)");
    CHECK(writef(path, "static 10.0.2.77/24 10.0.2.2 10.0.2.3") == 0, "static address accepted");
    readf(path, ipv4, sizeof ipv4);
    char mode[16], st[16];
    field(ipv4, "mode", mode, sizeof mode);
    field(ipv4, "state", st, sizeof st);
    field(ipv4, "address", addr2, sizeof addr2);
    CHECK(!strcmp(mode, "static") && !strcmp(st, "static") && !strcmp(addr2, "10.0.2.77/24"), "static: %s %s %s", mode, st, addr2);
    CHECK(tcp_host(port, "ultar-static"), "TCP to the host with the static address");

    // 7. back to DHCP
    CHECK(writef(path, "dhcp") == 0, "dhcp accepted");
    bound = wait_state(ifn, "bound", 20, ipv4, sizeof ipv4);
    field(ipv4, "address", addr2, sizeof addr2);
    CHECK(bound && !strncmp(addr2, "10.0.2.", 7) && strcmp(addr2, "10.0.2.77/24"), "DHCP again: %s", addr2);
    CHECK(tcp_host(port, "ultar-again"), "TCP to the host after DHCP again");

    printf("NETTEST: %s\n", fails ? "FEHLER" : "ALLES OK");
    return fails != 0;
}
