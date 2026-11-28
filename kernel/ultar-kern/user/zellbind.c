// zellbind – single files in a cell's view and chown inside a cell on the Ultar kernel
// (docs/integration/zell-chown.md, docs/integration/zell-netz.md).
//
//   zellbind      every check; verdict line "ZELLBIND: ALLES OK"
//
// Part 1, binds of single files: device nodes (null, zero, urandom, tty), a host file read-only and
// writable, the node of a host Unix socket; the kind must match (ENOTDIR); remount and umount of a file
// mount point; a mount point cannot be removed or renamed (EBUSY), so the bound host entry stays.
// Part 2, the right CELL_CHOWN: cell root without FILE_ADMIN changes owner and group on the cell's own
// writable mounts – never through the copied host tree, never on a read-only mount, never through a
// descriptor of an entry outside them, never outside a cell; set-id bits never appear in a cell.
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <sched.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mount.h>
#include <sys/prctl.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/sysmacros.h>
#include <sys/un.h>
#include <sys/wait.h>
#include <unistd.h>

#define PR_ULTAR_RIGHTS_GET 0x554c5200UL
#define PR_ULTAR_RIGHTS_DROP 0x554c5201UL
#define R_MOUNT 0x1UL
#define R_FILE_ADMIN 0x200UL
#define R_CELL_CHOWN 0x4000UL
#define R_ALL 0x7fffUL
#define APT 42 // the id dpkg gives `_apt`

#define T "/tmp/zb"
#define HOSTFILE T "/hostfile"   // bound read-only into the cell
#define HOSTRW T "/hostrw"       // bound writable into the cell
#define HOSTSOCK T "/host.sock"  // a host service the cell reaches through a bound node
#define AREA T "/linux"          // the "Linux area" the install cell may write
#define OUTSIDE T "/outside"     // seen read-only by the cell
#define CELL T "/cell"           // the cell's own tmpfs
#define VIEW T "/view"           // mount points of the area and of the read-only tree

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

// -1 with errno `want`
#define DENIED(call, want, ...)                                                                                         \
    do {                                                                                                                \
        errno = 0;                                                                                                      \
        int r_ = (call);                                                                                                \
        int e_ = errno;                                                                                                 \
        CHECK(r_ == -1 && e_ == (want), __VA_ARGS__);                                                                   \
        if (!(r_ == -1 && e_ == (want))) printf("      (Antwort %d, errno %d %s)\n", r_, e_, strerror(e_));            \
    } while (0)

static unsigned long rights(void) { return (unsigned long)prctl(PR_ULTAR_RIGHTS_GET, 0, 0, 0, 0); }

static int put(const char *path, const char *text, int mode) {
    int fd = open(path, O_WRONLY | O_CREAT | O_TRUNC, mode);
    if (fd < 0) return -1;
    int ok = write(fd, text, strlen(text)) == (ssize_t)strlen(text);
    close(fd);
    return ok ? 0 : -1;
}

// the file's text (at most 63 bytes), "" if unreadable
static const char *text_of(const char *path) {
    static char buf[64];
    buf[0] = 0;
    int fd = open(path, O_RDONLY);
    if (fd < 0) return buf;
    ssize_t n = read(fd, buf, sizeof buf - 1);
    buf[n > 0 ? n : 0] = 0;
    close(fd);
    return buf;
}

static int owner_is(const char *path, uid_t uid, gid_t gid) {
    struct stat st;
    return stat(path, &st) == 0 && st.st_uid == uid && st.st_gid == gid;
}

static unsigned perm_of(const char *path) {
    struct stat st;
    return stat(path, &st) == 0 ? (unsigned)(st.st_mode & 07777) : 0xffff;
}

static int is_chr(const char *path, unsigned major_n, unsigned minor_n) {
    struct stat st;
    return stat(path, &st) == 0 && S_ISCHR(st.st_mode) && major(st.st_rdev) == major_n && minor(st.st_rdev) == minor_n;
}

// an empty file to bind onto
static int spot(const char *path) { return put(path, "", 0644); }

static int bind_at(const char *from, const char *to, unsigned long extra) { return mount(from, to, NULL, MS_BIND | extra, NULL); }

// ------------------------------------------------------------------ part 1: single files

static void file_binds(void) {
    CHECK(unshare(CLONE_NEWNS) == 0, "Zelle: eigener Einhängungs-Namensraum");
    CHECK(mount("tmpfs", CELL, "tmpfs", MS_NOSUID | MS_NODEV, "size=65536,mode=0755") == 0, "Zelle: eigenes tmpfs");

    // device nodes – what a cell's /dev is made of
    CHECK(spot(CELL "/null") == 0 && bind_at("/dev/null", CELL "/null", 0) == 0, "Datei-Bind: /dev/null auf eine Datei");
    CHECK(mount(NULL, CELL "/null", NULL, MS_REMOUNT | MS_BIND | MS_NOSUID | MS_NOEXEC, NULL) == 0,
          "Datei-Bind: Neueinhängen mit nosuid,noexec");
    {
        int fd = open(CELL "/null", O_RDWR);
        char c;
        CHECK(fd >= 0 && write(fd, "x", 1) == 1 && read(fd, &c, 1) == 0 && is_chr(CELL "/null", 1, 3),
              "Datei-Bind: null schluckt, liest leer, stat zeigt das Gerät 1:3");
        if (fd >= 0) close(fd);
    }
    CHECK(spot(CELL "/zero") == 0 && bind_at("/dev/zero", CELL "/zero", 0) == 0, "Datei-Bind: /dev/zero");
    {
        unsigned char b[8] = {1, 1, 1, 1, 1, 1, 1, 1};
        int fd = open(CELL "/zero", O_RDONLY);
        CHECK(fd >= 0 && read(fd, b, sizeof b) == (ssize_t)sizeof b && !b[0] && !b[7], "Datei-Bind: zero liefert Nullen");
        if (fd >= 0) close(fd);
    }
    CHECK(spot(CELL "/urandom") == 0 && bind_at("/dev/urandom", CELL "/urandom", 0) == 0, "Datei-Bind: /dev/urandom");
    {
        unsigned char b[32] = {0};
        int fd = open(CELL "/urandom", O_RDONLY), any = 0;
        ssize_t n = fd >= 0 ? read(fd, b, sizeof b) : -1;
        for (int i = 0; i < 32; i++) any |= b[i];
        CHECK(n == 32 && any, "Datei-Bind: urandom liefert Zufall");
        if (fd >= 0) close(fd);
    }
    CHECK(spot(CELL "/tty") == 0 && bind_at("/dev/tty", CELL "/tty", 0) == 0 && is_chr(CELL "/tty", 5, 0),
          "Datei-Bind: /dev/tty (stat zeigt 5:0)");

    // host files
    CHECK(spot(CELL "/ro") == 0 && bind_at(HOSTFILE, CELL "/ro", MS_RDONLY) == 0 && !strcmp(text_of(CELL "/ro"), "HOST-DATA"),
          "Datei-Bind: Wirtsdatei schreibgeschützt, Inhalt sichtbar");
    DENIED(open(CELL "/ro", O_WRONLY), EROFS, "Datei-Bind: schreibgeschützt heißt EROFS");
    CHECK(mount(NULL, CELL "/ro", NULL, MS_REMOUNT | MS_BIND | MS_RDONLY | MS_NOSUID | MS_NODEV, NULL) == 0,
          "Datei-Bind: Neueinhängen schreibgeschützt,nosuid,nodev");
    DENIED(open(CELL "/ro", O_WRONLY), EROFS, "Datei-Bind: bleibt nach dem Neueinhängen schreibgeschützt");
    CHECK(spot(CELL "/rw") == 0 && bind_at(HOSTRW, CELL "/rw", 0) == 0 && put(CELL "/rw", "CELL-WROTE", 0644) == 0,
          "Datei-Bind: Wirtsdatei beschreibbar, die Zelle schreibt hinein");

    // the kind must match, like Linux
    CHECK(mkdir(CELL "/d", 0755) == 0, "Vorbereitung: Verzeichnis in der Zelle");
    DENIED(bind_at("/dev/null", CELL "/d", 0), ENOTDIR, "Art: Datei auf Verzeichnis -> ENOTDIR");
    DENIED(bind_at(AREA, CELL "/null", 0), ENOTDIR, "Art: Verzeichnis auf Datei -> ENOTDIR");
    DENIED(mount("tmpfs", CELL "/ro", "tmpfs", 0, "size=4096"), ENOTDIR, "Art: tmpfs auf Datei -> ENOTDIR");
    DENIED(bind_at("/dev/gibtsnicht", CELL "/null", 0), ENOENT, "Art: fehlende Quelle -> ENOENT");

    // a mount point is not an entry of its own
    DENIED(unlink(CELL "/rw"), EBUSY, "Einhängepunkt: unlink -> EBUSY");
    DENIED(rename(CELL "/rw", CELL "/weg"), EBUSY, "Einhängepunkt: umbenennen -> EBUSY");
    CHECK(put(CELL "/neu", "NEU", 0644) == 0, "Vorbereitung: eine gewöhnliche Datei");
    DENIED(rename(CELL "/neu", CELL "/rw"), EBUSY, "Einhängepunkt: darüber umbenennen -> EBUSY");
    DENIED(unlink(CELL "/null"), EBUSY, "Einhängepunkt: /dev/null bleibt");
    DENIED(rmdir(CELL), EBUSY, "Einhängepunkt: Verzeichnis (tmpfs) -> EBUSY");

    // a host service through its bound socket node
    {
        int s = socket(AF_UNIX, SOCK_STREAM, 0);
        struct sockaddr_un a = {.sun_family = AF_UNIX};
        strcpy(a.sun_path, CELL "/bus.sock");
        int ok = spot(CELL "/bus.sock") == 0 && bind_at(HOSTSOCK, CELL "/bus.sock", 0) == 0;
        ok = ok && connect(s, (void *)&a, sizeof a) == 0 && write(s, "CELL-SOCK", 9) == 9;
        CHECK(ok, "Datei-Bind: Socket-Knoten des Wirts, die Zelle verbindet sich");
        close(s);
    }

    // umount of a file mount point: the empty file underneath shows again
    CHECK(umount2(CELL "/null", 0) == 0, "Aushängen: Datei-Einhängepunkt");
    {
        struct stat st;
        CHECK(stat(CELL "/null", &st) == 0 && S_ISREG(st.st_mode) && st.st_size == 0, "Aushängen: darunter liegt wieder die leere Datei");
    }
    DENIED(umount2(CELL "/neu", 0), EINVAL, "Aushängen: keine Einhängung -> EINVAL");
}

// ------------------------------------------------------------------ part 2: chown in a cell

// chown/fchown by a process outside a cell that keeps CELL_CHOWN but not FILE_ADMIN
static void host_side(void) {
    CHECK(prctl(PR_ULTAR_RIGHTS_DROP, R_FILE_ADMIN, 0, 0, 0) == 0 && (rights() & R_CELL_CHOWN), "Wirt: ohne FILE_ADMIN, mit CELL_CHOWN");
    DENIED(chown(AREA "/host", APT, APT), EPERM, "Wirt: CELL_CHOWN gilt nur in einer Zelle");
}

static void cell_chown(void) {
    // the view an install cell gets: the Linux area writable, another tree read-only, a tmpfs; the
    // copied host tree stays writable for the first boundary check and is made read-only after it
    int out = open(OUTSIDE "/g", O_RDONLY); // opened before the cell: a descriptor of an outside entry
    CHECK(unshare(CLONE_NEWNS) == 0 && bind_at(AREA, VIEW "/linux", 0) == 0 && bind_at(OUTSIDE, VIEW "/ro", MS_RDONLY) == 0 &&
              mount("tmpfs", CELL, "tmpfs", 0, "size=65536") == 0,
          "Zelle: Linux-Bereich beschreibbar, fremder Baum schreibgeschützt, tmpfs");
    CHECK(prctl(PR_ULTAR_RIGHTS_DROP, R_FILE_ADMIN, 0, 0, 0) == 0 && rights() == (R_ALL & ~R_FILE_ADMIN) && getuid() == 0,
          "Zelle: root ohne FILE_ADMIN, mit CELL_CHOWN");

    // set-id bits never appear in a cell (cleared silently: dpkg expects its chmod to succeed)
    CHECK(chmod(VIEW "/linux/f", 04755) == 0 && perm_of(VIEW "/linux/f") == 0755, "Set-ID: chmod 4755 ergibt 0755");
    CHECK(chmod(VIEW "/linux/f", 01777) == 0 && perm_of(VIEW "/linux/f") == 01777 && chmod(VIEW "/linux/f", 0644) == 0,
          "Set-ID: das Sticky-Bit bleibt erlaubt");
    CHECK(put(VIEW "/linux/neu", "x", 06755) == 0 && (perm_of(VIEW "/linux/neu") & 06000) == 0, "Set-ID: open(O_CREAT, 6755) ohne Set-ID-Bits");

    // dpkg and apt: hand files to _apt
    CHECK(chown(VIEW "/linux/f", APT, APT) == 0 && owner_is(VIEW "/linux/f", APT, APT), "chown: im Linux-Bereich an _apt (42:42)");
    CHECK(chown(VIEW "/linux/f", 0, 0) == 0 && owner_is(VIEW "/linux/f", 0, 0), "chown: und zurück an root");
    CHECK(chown(VIEW "/linux/sid", APT, -1) == 0 && perm_of(VIEW "/linux/sid") == 0755, "chown: löscht Set-ID-Bits (4755 -> 0755)");
    CHECK(mkdir(VIEW "/linux/partial", 0700) == 0 && chown(VIEW "/linux/partial", APT, 0) == 0 && owner_is(VIEW "/linux/partial", APT, 0),
          "chown: Verzeichnis an _apt:root");
    CHECK(put(CELL "/t", "t", 0644) == 0 && chown(CELL "/t", 1000, 1000) == 0 && owner_is(CELL "/t", 1000, 1000), "chown: im eigenen tmpfs");
    DENIED(chown(OUTSIDE "/g", APT, APT), EPERM, "Grenze: Wirtsdatei außerhalb über den kopierten Wirtsbaum -> EPERM");

    // the copied host tree read-only, then MOUNT goes
    CHECK(mount(NULL, "/", NULL, MS_REMOUNT | MS_BIND | MS_RDONLY, NULL) == 0 && prctl(PR_ULTAR_RIGHTS_DROP, R_MOUNT, 0, 0, 0) == 0 &&
              rights() == (R_ALL & ~(R_FILE_ADMIN | R_MOUNT)),
          "Zelle: Wirtsbaum schreibgeschützt, MOUNT abgegeben");
    {
        int fd = open(VIEW "/linux/f", O_RDONLY);
        CHECK(fd >= 0 && fchown(fd, APT, 65534) == 0 && owner_is(VIEW "/linux/f", APT, 65534), "fchown: im Linux-Bereich");
        if (fd >= 0) close(fd);
    }
    DENIED(chown(OUTSIDE "/g", APT, APT), EROFS, "Grenze: Wirtsbaum schreibgeschützt -> EROFS");
    DENIED(chown(VIEW "/ro/g", APT, APT), EROFS, "Grenze: schreibgeschützter Bind -> EROFS");
    {
        int fd = open(VIEW "/ro/g", O_RDONLY);
        DENIED(fchown(fd, APT, APT), EROFS, "Grenze: fchown über einen schreibgeschützten Bind -> EROFS");
        DENIED(fchmod(fd, 0777), EROFS, "Grenze: fchmod über einen schreibgeschützten Bind -> EROFS");
        if (fd >= 0) close(fd);
    }
    DENIED(fchown(out, APT, APT), EROFS, "Grenze: fchown eines Deskriptors von vor der Zelle -> EROFS");
    DENIED(fchmod(out, 0777), EROFS, "Grenze: fchmod eines Deskriptors von vor der Zelle -> EROFS");
    DENIED(chown("/dev/null", APT, APT), EPERM, "Grenze: Gerätebaum -> EPERM");
    CHECK(owner_is(VIEW "/ro/g", 0, 0) && perm_of(VIEW "/ro/g") == 0644, "Grenze: der fremde Baum ist unverändert");

    // without the right, and CAP_CHOWN takes it along
    CHECK(prctl(PR_CAPBSET_DROP, 0 /* CAP_CHOWN */, 0, 0, 0) == 0 && !(rights() & R_CELL_CHOWN), "Recht: CAP_CHOWN abgeben nimmt CELL_CHOWN mit");
    DENIED(chown(VIEW "/linux/f2", APT, APT), EPERM, "Recht: ohne CELL_CHOWN -> EPERM");
    if (out >= 0) close(out);
}

// ------------------------------------------------------------------ main

// run `fn` in a child; its failures count here
static void sub(void (*fn)(void)) {
    pid_t pid = fork();
    if (pid == 0) {
        fn();
        _exit(fails > 100 ? 100 : fails);
    }
    int st = 0;
    if (pid < 0 || waitpid(pid, &st, 0) != pid || !WIFEXITED(st)) {
        CHECK(0, "Kindprozess endet normal");
        return;
    }
    fails += WEXITSTATUS(st);
}

int main(void) {
    setvbuf(stdout, NULL, _IOLBF, 0);
    CHECK(rights() == R_ALL && getuid() == 0, "Vorbereitung: root mit allen Rechten (0x7fff, mit CELL_CHOWN)");
    mkdir(T, 0755);
    int prep = mkdir(CELL, 0755) == 0 && mkdir(VIEW, 0755) == 0 && mkdir(VIEW "/linux", 0755) == 0 && mkdir(VIEW "/ro", 0755) == 0 &&
               mkdir(AREA, 0755) == 0 && mkdir(OUTSIDE, 0755) == 0 && put(HOSTFILE, "HOST-DATA", 0644) == 0 && put(HOSTRW, "HOST", 0644) == 0 &&
               put(AREA "/f", "f", 0644) == 0 && put(AREA "/f2", "f2", 0644) == 0 && put(AREA "/host", "h", 0644) == 0 &&
               put(AREA "/sid", "s", 0644) == 0 && chmod(AREA "/sid", 04755) == 0 && put(OUTSIDE "/g", "g", 0644) == 0;
    CHECK(prep, "Vorbereitung: Wirtsdateien, Linux-Bereich, fremder Baum");

    // the host service the cell reaches through a bound socket node
    int ls = socket(AF_UNIX, SOCK_STREAM, 0);
    struct sockaddr_un a = {.sun_family = AF_UNIX};
    strcpy(a.sun_path, HOSTSOCK);
    CHECK(ls >= 0 && bind(ls, (void *)&a, sizeof a) == 0 && listen(ls, 4) == 0, "Vorbereitung: Wirtsdienst lauscht auf " HOSTSOCK);

    pid_t pid = fork();
    if (pid == 0) {
        file_binds();
        _exit(fails > 100 ? 100 : fails);
    }
    struct pollfd p = {.fd = ls, .events = POLLIN};
    char got[16] = {0};
    if (poll(&p, 1, 15000) == 1) {
        int c = accept(ls, NULL, NULL);
        if (c >= 0) {
            struct pollfd q = {.fd = c, .events = POLLIN};
            if (poll(&q, 1, 5000) == 1 && read(c, got, sizeof got - 1) < 0) got[0] = 0;
            close(c);
        }
    }
    int st = 0;
    CHECK(waitpid(pid, &st, 0) == pid && WIFEXITED(st), "Zelle (Datei-Binds) endet normal");
    fails += WIFEXITED(st) ? WEXITSTATUS(st) : 1;
    CHECK(!strcmp(got, "CELL-SOCK"), "Wirt: der Dienst erhielt die Nachricht der Zelle (%s)", got);
    CHECK(!strcmp(text_of(HOSTRW), "CELL-WROTE"), "Wirt: die beschreibbar gebundene Datei trägt, was die Zelle schrieb");
    CHECK(!strcmp(text_of(HOSTFILE), "HOST-DATA") && access("/dev/null", F_OK) == 0 && access(HOSTSOCK, F_OK) == 0,
          "Wirt: gebundene Einträge stehen noch (kein unlink/rename durch die Zelle)");

    sub(host_side);
    sub(cell_chown);
    CHECK(owner_is(AREA "/f", APT, 65534) && owner_is(AREA "/partial", APT, 0), "Wirt: die Besitzer im Linux-Bereich sind die der Zelle");
    CHECK(owner_is(AREA "/f2", 0, 0) && owner_is(AREA "/host", 0, 0) && owner_is(OUTSIDE "/g", 0, 0), "Wirt: alles andere gehört weiter root");

    printf(fails ? "ZELLBIND: %d FEHLER\n" : "ZELLBIND: ALLES OK\n", fails);
    fflush(stdout);
    return fails != 0;
}
