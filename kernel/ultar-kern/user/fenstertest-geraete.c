/* fenstertest: descriptor passing (K4/K5), the framebuffer (K6), device node modes and the input
 * nodes (K8) – see fenstertest.c */
#define _GNU_SOURCE
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <grp.h>
#include <linux/fb.h>
#include <linux/input.h>
#include <linux/netlink.h>
#include <poll.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <sys/prctl.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/sysmacros.h>
#include <sys/wait.h>
#include <unistd.h>
#include "fenstertest.h"

#define R_RAW_DEVICE 0x20UL

/* ---------------------------------------------------------------- K4/K5: descriptors on Unix sockets */

static long send_fds(int s, const char *data, const int *fds, int n, int flags) {
    char ctl[CMSG_SPACE(sizeof(int) * 8)];
    struct iovec io = {(void *)data, strlen(data)};
    struct msghdr m = {.msg_iov = &io, .msg_iovlen = 1};
    if (n > 0) {
        memset(ctl, 0, sizeof ctl);
        m.msg_control = ctl;
        m.msg_controllen = CMSG_SPACE(sizeof(int) * n);
        struct cmsghdr *c = CMSG_FIRSTHDR(&m);
        c->cmsg_level = SOL_SOCKET;
        c->cmsg_type = SCM_RIGHTS;
        c->cmsg_len = CMSG_LEN(sizeof(int) * n);
        memcpy(CMSG_DATA(c), fds, sizeof(int) * n);
    }
    return sendmsg(s, &m, flags);
}

/* receive into buf; the descriptors go to fds (at most `room` fit the control buffer) */
static long recv_fds(int s, char *buf, size_t len, int *fds, int *nfds, int room, int flags, int *mflags) {
    char ctl[CMSG_SPACE(sizeof(int) * 8)];
    struct iovec io = {buf, len};
    struct msghdr m = {.msg_iov = &io, .msg_iovlen = 1, .msg_control = ctl, .msg_controllen = CMSG_SPACE(sizeof(int) * room)};
    long r = recvmsg(s, &m, flags);
    *nfds = 0;
    if (mflags) *mflags = m.msg_flags;
    if (r < 0) return r;
    for (struct cmsghdr *c = CMSG_FIRSTHDR(&m); c; c = CMSG_NXTHDR(&m, c)) {
        if (c->cmsg_level == SOL_SOCKET && c->cmsg_type == SCM_RIGHTS) {
            int k = (c->cmsg_len - CMSG_LEN(0)) / sizeof(int);
            memcpy(fds + *nfds, CMSG_DATA(c), sizeof(int) * k);
            *nfds += k;
        }
    }
    return r;
}

static int pipes_seen;
static void on_pipe(int s) {
    (void)s;
    pipes_seen++;
}

static int memobj(const char *text) {
    int fd = memfd_create("teilen", 0);
    if (fd >= 0 && (ftruncate(fd, 4096) || pwrite(fd, text, strlen(text), 0) < 0)) return -1;
    return fd;
}

static void passing_checks(int sv[2]) {
    int a = memobj("Inhalt A"), b = memobj("Inhalt B"), got[8], n, fl;
    char buf[64] = {0};
    struct stat st;
    check("K4 sendmsg mit SCM_RIGHTS", send_fds(sv[0], "A", &a, 1, 0) == 1);
    long r = recv_fds(sv[1], buf, sizeof buf, got, &n, 8, 0, &fl);
    char t[9] = {0};
    check("K4 recvmsg liefert einen neuen Deskriptor auf dasselbe Objekt", r == 1 && n == 1 && got[0] != a && fstat(got[0], &st) == 0 && st.st_size == 4096 && pread(got[0], t, 8, 0) == 8 && strcmp(t, "Inhalt A") == 0);
    check("K4 ohne MSG_CMSG_CLOEXEC: close-on-exec aus", n == 1 && fcntl(got[0], F_GETFD) == 0);
    if (n == 1) close(got[0]);
    send_fds(sv[0], "B", &b, 1, 0);
    r = recv_fds(sv[1], buf, sizeof buf, got, &n, 8, MSG_CMSG_CLOEXEC, NULL);
    check("K4 MSG_CMSG_CLOEXEC setzt close-on-exec", r == 1 && n == 1 && (fcntl(got[0], F_GETFD) & FD_CLOEXEC));
    if (n == 1) close(got[0]);
    int two[2] = {a, b};
    send_fds(sv[0], "11", &a, 1, 0);
    send_fds(sv[0], "22", two, 2, 0);
    r = recv_fds(sv[1], buf, sizeof buf, got, &n, 8, 0, NULL);
    check("K4 zwei Nachrichten: die erste endet vor der zweiten (1 Deskriptor)", r == 2 && memcmp(buf, "11", 2) == 0 && n == 1);
    for (int i = 0; i < n; i++) close(got[i]);
    r = recv_fds(sv[1], buf, sizeof buf, got, &n, 8, 0, NULL);
    check("K4 die zweite Nachricht bringt ihre 2 Deskriptoren", r == 2 && memcmp(buf, "22", 2) == 0 && n == 2);
    for (int i = 0; i < n; i++) close(got[i]);
    send_fds(sv[0], "3", &a, 1, 0);
    send_fds(sv[0], "4", NULL, 0, 0);
    check("K4 read verwirft mitgeschickte Deskriptoren", read(sv[1], buf, 1) == 1 && buf[0] == '3' && recv_fds(sv[1], buf, sizeof buf, got, &n, 8, 0, NULL) == 1 && buf[0] == '4' && n == 0);
    int three[3] = {a, b, a};
    send_fds(sv[0], "5", three, 3, 0);
    r = recv_fds(sv[1], buf, sizeof buf, got, &n, 1, 0, &fl);
    check("K4 zu kleiner Steuerpuffer (Platz für 2 von 3): MSG_CTRUNC, was passt, kommt an", r == 1 && n == 2 && (fl & MSG_CTRUNC));
    for (int i = 0; i < n; i++) close(got[i]);
    int bad = 999;
    denied("K4 ungültiger Deskriptor", send_fds(sv[0], "x", &bad, 1, 0), EBADF);
    denied("K4 Unix-Sockets werden nicht übergeben (kein Kreis ohne Sammler)", send_fds(sv[0], "x", &sv[1], 1, 0), EOPNOTSUPP);
    pid_t c = fork();
    if (c == 0) {
        int fd = -1, k;
        recv_fds(sv[1], buf, sizeof buf, &fd, &k, 1, 0, NULL);
        char *m = k == 1 ? mmap(NULL, 4096, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0) : MAP_FAILED;
        if (m == MAP_FAILED) _exit(1);
        memcpy(m + 100, "vom Kind", 8);
        _exit(0);
    }
    send_fds(sv[0], "M", &a, 1, 0);
    int ws = -1;
    check("K4 ein anderer Prozess bildet das übergebene Objekt geteilt ab", waitpid(c, &ws, 0) == c && ws == 0 && pread(a, t, 8, 100) == 8 && memcmp(t, "vom Kind", 8) == 0);
    close(a);
    close(b);
}

void rights_checks(void) {
    int sv[2];
    check("K4 socketpair AF_UNIX", socketpair(AF_UNIX, SOCK_STREAM, 0, sv) == 0);
    passing_checks(sv);
    char buf[16];
    long t0 = now_us();
    denied("K5 recv MSG_DONTWAIT auf leerem, blockierendem Socket", recv(sv[1], buf, sizeof buf, MSG_DONTWAIT), EAGAIN);
    struct iovec io = {buf, sizeof buf};
    struct msghdr m = {.msg_iov = &io, .msg_iovlen = 1};
    denied("K5 recvmsg MSG_DONTWAIT", recvmsg(sv[1], &m, MSG_DONTWAIT), EAGAIN);
    char big[4096] = {0};
    long r;
    int k = 0;
    while ((r = send(sv[0], big, sizeof big, MSG_DONTWAIT)) > 0 && k < 100000) k++;
    check("K5 send MSG_DONTWAIT: voller Socket meldet EAGAIN statt zu warten", r == -1 && errno == EAGAIN && now_us() - t0 < 5000000);
    close(sv[1]);
    signal(SIGPIPE, on_pipe);
    denied("K5 MSG_NOSIGNAL: EPIPE", send(sv[0], "x", 1, MSG_NOSIGNAL), EPIPE);
    check("K5 MSG_NOSIGNAL: kein SIGPIPE", pipes_seen == 0);
    denied("K5 ohne MSG_NOSIGNAL: EPIPE", send(sv[0], "x", 1, 0), EPIPE);
    check("K5 ohne MSG_NOSIGNAL: SIGPIPE", pipes_seen == 1);
    signal(SIGPIPE, SIG_DFL);
    close(sv[0]);
}

/* ---------------------------------------------------------------- K6: framebuffer */

void fb_checks(void) {
    int fd = open("/dev/fb0", O_RDWR | O_CLOEXEC);
    if (fd < 0) {
        say("--     Bildspeicher übersprungen: /dev/fb0 %s\n", strerror(errno));
        return;
    }
    struct fb_var_screeninfo v;
    struct fb_fix_screeninfo f;
    check("K6 FBIOGET_VSCREENINFO/FSCREENINFO", ioctl(fd, FBIOGET_VSCREENINFO, &v) == 0 && ioctl(fd, FBIOGET_FSCREENINFO, &f) == 0 && f.smem_len >= v.yres * f.line_length);
    unsigned *px = mmap(NULL, f.smem_len, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    check("K6 mmap des Bildspeichers", px != MAP_FAILED);
    if (px != MAP_FAILED) {
        px[0] = 0x00336699;
        unsigned back = 0, w = 0x00ffeedd;
        check("K6 ein Bildpunkt über die Abbildung steht im Bildspeicher", pread(fd, &back, 4, 0) == 4 && back == 0x00336699);
        check("K6 pwrite erscheint in der Abbildung", pwrite(fd, &w, 4, 8) == 4 && px[2] == 0x00ffeedd);
        munmap(px, f.smem_len);
    }
    errno = 0;
    check("K6 MAP_PRIVATE des Bildspeichers: ENODEV", mmap(NULL, 4096, PROT_READ, MAP_PRIVATE, fd, 0) == MAP_FAILED && errno == ENODEV);
    struct fb_vblank vb0 = {0}, vb1 = {0};
    int arg = 0;
    long t0 = now_us();
    int ok = ioctl(fd, FBIOGET_VBLANK, &vb0) == 0;
    for (int i = 0; i < 12 && ok; i++) ok = ioctl(fd, FBIO_WAITFORVSYNC, &arg) == 0;
    long took = (now_us() - t0) / 1000;
    check("K6 FBIO_WAITFORVSYNC: 12 Austastlücken dauern etwa 200 ms (60 Hz)", ok && took >= 150 && took < 1500);
    check("K6 FBIOGET_VBLANK zählt die Austastlücken", ioctl(fd, FBIOGET_VBLANK, &vb1) == 0 && (vb1.flags & FB_VBLANK_HAVE_COUNT) && vb1.count >= vb0.count + 10);
    close(fd);
    fd = open("/dev/fb0", O_RDONLY | O_CLOEXEC);
    errno = 0;
    check("K6 nur lesend geöffnet: schreibbare Abbildung abgewiesen", fd >= 0 && mmap(NULL, 4096, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0) == MAP_FAILED && errno == EACCES);
    close(fd);
}

/* ---------------------------------------------------------------- device node modes */

static int node_is(const char *path, mode_t mode, uid_t uid, gid_t gid, unsigned maj, unsigned min) {
    struct stat st;
    return lstat(path, &st) == 0 && S_ISCHR(st.st_mode) && (st.st_mode & 07777) == mode && st.st_uid == uid && st.st_gid == gid && major(st.st_rdev) == maj && minor(st.st_rdev) == min;
}

static int seat(const char *path, mode_t mode, gid_t gid) {
    return chmod(path, 0600) == 0 && lchown(path, 0, gid) == 0 && chmod(path, mode) == 0;
}

/* run fn as uid/gid with the supplementary group `extra` (0: none) and only the rights `keep` */
static void as(uid_t uid, gid_t extra, unsigned long keep, int (*fn)(void)) {
    pid_t c = fork();
    if (c == 0) {
        fails = 0;
        gid_t g[1] = {extra};
        int ok = setgroups(extra ? 1 : 0, g) == 0 && setresgid(uid, uid, uid) == 0 && setresuid(uid, uid, uid) == 0;
        prctl(PR_ULTAR_RIGHTS_DROP, ~keep, 0, 0, 0);
        if (!ok) _exit(99);
        _exit(fn() ? 1 : 0);
    }
    int st = -1;
    waitpid(c, &st, 0);
    if (!WIFEXITED(st) || WEXITSTATUS(st) == 99) check("Hilfsprozess mit eigener Kennung lief", 0);
    else if (WEXITSTATUS(st)) fails++;
}

static int opens(const char *path, int flags) {
    int fd = open(path, flags | O_CLOEXEC | O_NONBLOCK);
    if (fd >= 0) close(fd);
    return fd >= 0;
}

static int seat_member(void) {
    check("Sitz-Mitglied (1000, Gruppe 812, keine Rechte): event0 lesen", opens("/dev/input/event0", O_RDONLY));
    check("Sitz-Mitglied: fb0 lesen und schreiben", opens("/dev/fb0", O_RDWR));
    check("Sitz-Mitglied: /dev/null", opens("/dev/null", O_RDWR));
    denied("Sitz-Mitglied: chmod fb0", chmod("/dev/fb0", 0666), EPERM);
    denied("Sitz-Mitglied: chown fb0", lchown("/dev/fb0", 1000, 1000), EPERM);
    denied("Sitz-Mitglied: uevent schreiben (braucht FILE_ADMIN)", open("/sys/class/input/event0/uevent", O_WRONLY), EACCES);
    return fails;
}

static int stranger(void) {
    denied("Fremder (1000, ohne Gruppe): event0", open("/dev/input/event0", O_RDONLY | O_NONBLOCK), EACCES);
    denied("Fremder: fb0", open("/dev/fb0", O_RDWR), EACCES);
    return fails;
}

static int bare_root(void) {
    denied("uid 0 ohne Rechte und ohne Gruppe: fb0 (Besitz allein reicht nicht)", open("/dev/fb0", O_RDWR), EACCES);
    return fails;
}

static int raw_root(void) {
    check("uid 0 nur mit RAW_DEVICE: event1", opens("/dev/input/event1", O_RDONLY));
    return fails;
}

void devnode_checks(void) {
    struct stat st;
    check("Knoten: /dev/fb0 Zeichengerät 29:0, root 0600", node_is("/dev/fb0", 0600, 0, 0, 29, 0));
    check("Knoten: /dev/input/event0 Zeichengerät 13:64, root 0600", node_is("/dev/input/event0", 0600, 0, 0, 13, 64));
    check("Knoten: /dev/null 1:3, 0666", node_is("/dev/null", 0666, 0, 0, 1, 3));
    check("Knoten: /dev und /dev/input gehören root, ohne Schreibrecht für andere", lstat("/dev", &st) == 0 && S_ISDIR(st.st_mode) && st.st_uid == 0 && !(st.st_mode & 022) && lstat("/dev/input", &st) == 0 && st.st_uid == 0 && !(st.st_mode & 022));
    check("Sitz: fb0, event0, event1 -> root:812 0660", seat("/dev/fb0", 0660, SEAT_GROUP) && seat("/dev/input/event0", 0660, SEAT_GROUP) && seat("/dev/input/event1", 0660, SEAT_GROUP));
    check("Sitz: stat liest root:812 0660 zurück", node_is("/dev/fb0", 0660, 0, SEAT_GROUP, 29, 0) && node_is("/dev/input/event0", 0660, 0, SEAT_GROUP, 13, 64));
    denied("chmod /dev selbst bleibt abgewiesen", chmod("/dev", 0777), EPERM);
    denied("chmod in /proc bleibt abgewiesen", chmod("/proc/self/status", 0777), EPERM);
    as(1000, SEAT_GROUP, 0, seat_member);
    as(1000, 0, 0, stranger);
    as(0, 0, 0, bare_root);
    as(0, 0, R_RAW_DEVICE, raw_root);
    check("zurück: root 0600", seat("/dev/fb0", 0600, 0) && seat("/dev/input/event0", 0600, 0) && seat("/dev/input/event1", 0600, 0) && node_is("/dev/fb0", 0600, 0, 0, 29, 0));
}

/* ---------------------------------------------------------------- K8: input nodes */

static int has_bit(const unsigned char *b, int bit) {
    return b[bit / 8] >> (bit % 8) & 1;
}

static int slurp(const char *path, char *b, size_t n) {
    int fd = open(path, O_RDONLY | O_CLOEXEC);
    long r = fd >= 0 ? read(fd, b, n - 1) : -1;
    if (fd >= 0) close(fd);
    b[r > 0 ? r : 0] = 0;
    return r > 0;
}

static const char *key(const char *msg, long len, const char *k) {
    size_t kl = strlen(k);
    for (long i = 0; i < len; i += strlen(msg + i) + 1)
        if (strncmp(msg + i, k, kl) == 0 && msg[i + kl] == '=') return msg + i + kl + 1;
    return "";
}

static void sysfs_checks(void) {
    char b[256], real[4096];
    check("sysfs: /sys/class/input/event0/device/name", slurp("/sys/class/input/event0/device/name", b, sizeof b) && strcmp(b, "Ultar Maus\n") == 0);
    check("sysfs: /sys/class/input/event1/dev 13:65", slurp("/sys/class/input/event1/dev", b, sizeof b) && strcmp(b, "13:65\n") == 0);
    check("sysfs: realpath geht, kein /devices/virtual/", realpath("/sys/class/input/event0", real) && !strstr(real, "/devices/virtual/"));
    int ev = socket(AF_NETLINK, SOCK_DGRAM | SOCK_CLOEXEC, NETLINK_KOBJECT_UEVENT);
    struct sockaddr_nl a = {.nl_family = AF_NETLINK, .nl_groups = 1};
    bind(ev, (struct sockaddr *)&a, sizeof a);
    int fd = open("/sys/class/input/event1/uevent", O_WRONLY | O_CLOEXEC);
    check("sysfs: \"change\" in uevent schreiben", fd >= 0 && write(fd, "change", 6) == 6);
    if (fd >= 0) close(fd);
    char msg[1024];
    long len = -1;
    struct pollfd pf = {ev, POLLIN, 0};
    for (int i = 0; i < 5 && len < 0; i++) {
        len = poll(&pf, 1, 1000) == 1 ? recv(ev, msg, sizeof msg, 0) : -1;
        if (len > 0 && strcmp(key(msg, len, "SUBSYSTEM"), "input")) len = -1;
    }
    check("sysfs: Ereignis change SUBSYSTEM=input DEVNAME=input/event1", len > 0 && strcmp(key(msg, len, "ACTION"), "change") == 0 && strcmp(key(msg, len, "DEVNAME"), "input/event1") == 0);
    close(ev);
}

void input_checks(void) {
    int m = open("/dev/input/event0", O_RDONLY | O_NONBLOCK | O_CLOEXEC), k = open("/dev/input/event1", O_RDONLY | O_NONBLOCK | O_CLOEXEC);
    if (m < 0 || k < 0) {
        say("--     Eingabe übersprungen: keine Eingabeknoten (%s)\n", strerror(errno));
        return;
    }
    int ver = 0;
    struct input_id id = {0};
    check("K8 EVIOCGVERSION, EVIOCGID (Tastatur am i8042-Bus)", ioctl(m, EVIOCGVERSION, &ver) == 0 && ver == EV_VERSION && ioctl(k, EVIOCGID, &id) == 0 && id.bustype == BUS_I8042);
    unsigned char ev[4] = {0}, keys[KEY_MAX / 8 + 1] = {0}, abs[ABS_MAX / 8 + 1] = {0};
    int r = ioctl(m, EVIOCGBIT(0, sizeof ev), ev);
    check("K8 EVIOCGBIT(0) Maus: EV_KEY, EV_REL, EV_ABS; Länge zurück", r == (int)sizeof ev && has_bit(ev, EV_KEY) && has_bit(ev, EV_ABS) && has_bit(ev, EV_REL));
    check("K8 EVIOCGBIT(EV_ABS) Maus: ABS_X, ABS_Y", ioctl(m, EVIOCGBIT(EV_ABS, sizeof abs), abs) > 0 && has_bit(abs, ABS_X) && has_bit(abs, ABS_Y));
    check("K8 EVIOCGBIT(EV_KEY) Maus: BTN_LEFT, keine Buchstaben", ioctl(m, EVIOCGBIT(EV_KEY, sizeof keys), keys) > 0 && has_bit(keys, BTN_LEFT) && !has_bit(keys, KEY_A));
    memset(ev, 0, sizeof ev);
    memset(keys, 0, sizeof keys);
    check("K8 EVIOCGBIT Tastatur: EV_KEY ohne EV_ABS, KEY_A und KEY_ENTER", ioctl(k, EVIOCGBIT(0, sizeof ev), ev) > 0 && has_bit(ev, EV_KEY) && !has_bit(ev, EV_ABS) && ioctl(k, EVIOCGBIT(EV_KEY, sizeof keys), keys) > 0 && has_bit(keys, KEY_A) && has_bit(keys, KEY_ENTER));
    char name[64] = {0};
    r = ioctl(k, EVIOCGNAME(sizeof name), name);
    check("K8 EVIOCGNAME liefert die Länge", r == (int)strlen("Ultar Tastatur") + 1 && strcmp(name, "Ultar Tastatur") == 0);
    close(m);
    close(k);
    denied("K8 ohne USB-Tastatur gibt es kein event2", open("/dev/input/event2", O_RDONLY), ENOENT);
    sysfs_checks();
}
