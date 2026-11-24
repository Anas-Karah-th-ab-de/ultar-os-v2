/* rechtetest – identities on the Ultar kernel: user/group ids, file modes and owners, Unix socket
 * nodes with permission check and SO_PEERCRED, fsync. One line per check ("ok" / "FEHLER"); the last
 * line is "RECHTETEST: ALLES OK" when every check passed.
 *   rechtetest                 everything that needs no disk
 *   rechtetest --ext4 <dir>    write a file below <dir> (a writable ext4), chmod/chown/fsync it and then
 *                              wait without closing it – the host inspects the disk (rechte-ext4.sh)
 * Runs as a program started by the kernel: root, every right. */
#define _GNU_SOURCE
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <grp.h>
#include <sched.h>
#include <signal.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mount.h>
#include <sys/prctl.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/syscall.h>
#include <sys/un.h>
#include <sys/wait.h>
#include <unistd.h>

#define PR_ULTAR_RIGHTS_GET 0x554c5200
#define PR_ULTAR_RIGHTS_DROP 0x554c5201
#define R_SET_ID 0x100UL
#define R_FILE_ADMIN 0x200UL
#define R_ALL 0xfffUL
#define D "/tmp/rt"
enum { ALICE = 1000, BOB = 2000, CAROL = 3000, STAFF = 100, OTHERS = 200 };

static int fails;
static const char *bin_dir = "/bin";

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

/* the call must have failed with exactly this error */
static void denied(const char *name, long r, int want) {
    int e = errno, ok = r == -1 && e == want;
    if (!ok) fails++;
    say("%s %s -> %s\n", ok ? "ok    " : "FEHLER", name, r == -1 ? strerror(e) : "angenommen");
}

/* run fn as the given user without any right; its failed checks count here */
static void as(uid_t uid, gid_t gid, gid_t extra, int (*fn)(void)) {
    pid_t c = fork();
    if (c == 0) {
        fails = 0;
        gid_t g[1] = {extra};
        int ok = setgroups(extra ? 1 : 0, g) == 0 && setresgid(gid, gid, gid) == 0 && setresuid(uid, uid, uid) == 0;
        prctl(PR_ULTAR_RIGHTS_DROP, ~0UL, 0, 0, 0);
        if (!ok || prctl(PR_ULTAR_RIGHTS_GET, 0, 0, 0, 0) != 0) _exit(99);
        _exit(fn() ? 1 : 0);
    }
    int st = -1;
    waitpid(c, &st, 0);
    if (!WIFEXITED(st) || WEXITSTATUS(st) == 99) check("Hilfsprozess mit eigener Kennung lief", 0);
    else if (WEXITSTATUS(st)) fails++;
}

static int mode_is(const char *path, mode_t mode, uid_t uid, gid_t gid) {
    struct stat sb;
    if (lstat(path, &sb) != 0) return 0;
    if (sb.st_mode != mode || sb.st_uid != uid || sb.st_gid != gid) say("       (%s: Modus %o, Besitzer %d:%d)\n", path, sb.st_mode, sb.st_uid, sb.st_gid);
    return sb.st_mode == mode && sb.st_uid == uid && sb.st_gid == gid;
}

static int put_file(const char *path, const char *text, mode_t mode) {
    int fd = open(path, O_WRONLY | O_CREAT | O_TRUNC, mode);
    if (fd < 0) return -1;
    int ok = write(fd, text, strlen(text)) == (ssize_t)strlen(text);
    return close(fd) == 0 && ok ? 0 : -1;
}

static int file_is(const char *path, const char *text) {
    char b[64] = {0};
    int fd = open(path, O_RDONLY);
    if (fd < 0) return 0;
    ssize_t n = read(fd, b, sizeof b - 1);
    close(fd);
    return n == (ssize_t)strlen(text) && !strcmp(b, text);
}

static int copy_file(const char *from, const char *to, mode_t mode) {
    static char buf[65536];
    int in = open(from, O_RDONLY), out = open(to, O_WRONLY | O_CREAT | O_TRUNC, mode);
    ssize_t n;
    int ok = in >= 0 && out >= 0;
    while (ok && (n = read(in, buf, sizeof buf)) > 0) ok = write(out, buf, n) == n;
    if (in >= 0) close(in);
    return (out < 0 || close(out) == 0) && ok ? 0 : -1;
}

static int runs(const char *path) {
    pid_t c = fork();
    if (c == 0) {
        int nul = open("/dev/null", O_WRONLY);
        if (nul >= 0) dup2(nul, 1);
        execl(path, path, (char *)NULL);
        _exit(errno == EACCES ? 77 : 78);
    }
    int st = -1;
    waitpid(c, &st, 0);
    return WIFEXITED(st) ? WEXITSTATUS(st) : -1;
}

static const char *hallo(void) {
    static char b[64];
    snprintf(b, sizeof b, "%s/hallo", bin_dir);
    return b;
}

/* ------------------------------------------------------------------ ids */

static int ids_without_right(void) {
    uid_t r, e, s;
    check("Kennungen: setresuid(1000, 2000, 3000) mit Recht SET_ID", setresuid(1000, 2000, 3000) == 0 && getresuid(&r, &e, &s) == 0 && r == 1000 && e == 2000 && s == 3000 && getuid() == 1000 && geteuid() == 2000);
    check("Kennungen: solange das Recht da ist, führt setuid(0) zurück (das Recht zählt, nicht die Nummer)", setresuid(0, 0, 0) == 0 && setresuid(1000, 2000, 3000) == 0);
    prctl(PR_ULTAR_RIGHTS_DROP, R_SET_ID, 0, 0, 0);
    denied("Ohne SET_ID: setuid(0)", setuid(0), EPERM);
    denied("Ohne SET_ID: seteuid auf eine fremde Nummer", seteuid(4000), EPERM);
    denied("Ohne SET_ID: setresuid(0, 0, 0)", setresuid(0, 0, 0), EPERM);
    denied("Ohne SET_ID: setgid auf eine fremde Nummer", syscall(SYS_setgid, 7), EPERM);
    gid_t g = 5;
    denied("Ohne SET_ID: setgroups", setgroups(1, &g), EPERM);
    check("Ohne SET_ID: wechseln zwischen den eigenen Nummern geht (seteuid 1000, 3000)", seteuid(1000) == 0 && geteuid() == 1000 && seteuid(3000) == 0 && geteuid() == 3000);
    check("Ohne SET_ID: setreuid tauscht echte und wirksame Nummer", setreuid(3000, 1000) == 0 && getuid() == 3000 && geteuid() == 1000);
    denied("Kennungen: setuid(-1)", syscall(SYS_setuid, -1), EINVAL);
    return fails;
}

static int ids_after_exec(void) {
    uid_t r, e, s;
    gid_t gr, ge, gs, groups[8];
    int n = getgroups(8, groups);
    int ok = getresuid(&r, &e, &s) == 0 && r == 1000 && e == 1001 && s == 1002 && getresgid(&gr, &ge, &gs) == 0 && gr == 100 && ge == 101 && gs == 102 && n == 2 && groups[0] == 7 && groups[1] == 8;
    return ok ? 0 : 1;
}

static void id_tests(const char *self) {
    uid_t r = 9, e = 9, s = 9;
    gid_t gr = 9, ge = 9, gs = 9;
    say("== Benutzer- und Gruppen-Kennungen\n");
    check("Kennungen: vom Kern gestartet = root (alle sechs 0, keine Zusatzgruppen)", getuid() == 0 && geteuid() == 0 && getgid() == 0 && getegid() == 0 && getresuid(&r, &e, &s) == 0 && r + e + s == 0 && getresgid(&gr, &ge, &gs) == 0 && gr + ge + gs == 0 && getgroups(0, NULL) == 0);
    pid_t c = fork();
    if (c == 0) {
        fails = 0;
        _exit(ids_without_right() ? 1 : 0);
    }
    int st = -1;
    waitpid(c, &st, 0);
    if (st) fails++;
    check("Kennungen: der Elternprozess ist unverändert root", getuid() == 0 && geteuid() == 0);
    c = fork();
    if (c == 0) {
        gid_t g[2] = {7, 8};
        if (setgroups(2, g) || setresgid(100, 101, 102) || setresuid(1000, 1001, 1002)) _exit(50);
        pid_t k = fork();
        if (k == 0) _exit(ids_after_exec());
        int s2 = -1;
        waitpid(k, &s2, 0);
        if (s2) _exit(51);
        execl(self, self, "--ids", (char *)NULL);
        _exit(52);
    }
    st = -1;
    waitpid(c, &st, 0);
    check("Kennungen: fork und execve vererben alle Nummern und Zusatzgruppen", st == 0);
    gid_t many[33] = {0};
    denied("Kennungen: mehr als 32 Zusatzgruppen", setgroups(33, many), EINVAL);
}

/* ------------------------------------------------------------------ modes and owners */

static int alice_files(void) {
    umask(022);
    check("Modus: Datei mit 0666 und umask 022 entsteht als 0644, gehört dem Anleger", put_file(D "/geheim", "geheim", 0666) == 0 && mode_is(D "/geheim", S_IFREG | 0644, ALICE, STAFF));
    check("Modus: chmod 0600 durch den Besitzer", chmod(D "/geheim", 0600) == 0 && mode_is(D "/geheim", S_IFREG | 0600, ALICE, STAFF) && file_is(D "/geheim", "geheim"));
    check("Modus: Verzeichnis 0700 mit Datei darin", mkdir(D "/privat", 0700) == 0 && mode_is(D "/privat", S_IFDIR | 0700, ALICE, STAFF) && put_file(D "/privat/drin", "drin", 0644) == 0);
    check("Modus: Datei für die Gruppe lesbar (0640)", put_file(D "/gruppe", "gruppe", 0640) == 0);
    denied("Besitzer: chown an einen anderen Benutzer ohne Recht", chown(D "/geheim", BOB, -1), EPERM);
    denied("Besitzer: chgrp an eine fremde Gruppe ohne Recht", chown(D "/geheim", -1, OTHERS), EPERM);
    check("Besitzer: chgrp an die eigene Gruppe geht", chown(D "/geheim", -1, STAFF) == 0);
    check("Modus: vorhandene Dateien der Startpartition lesen sich als root 0755", mode_is(hallo(), S_IFREG | 0755, 0, 0));
    check("Ausführen: Kopie mit 0644 angelegt", copy_file(hallo(), D "/prog", 0644) == 0);
    check("Ausführen: ohne x-Bit -> EACCES", runs(D "/prog") == 77);
    check("Ausführen: nach chmod 0750 läuft das Programm", chmod(D "/prog", 0750) == 0 && runs(D "/prog") == 0);
    int fd = open(D "/geheim", O_RDWR);
    check("Modus: fchmod über den Deskriptor", fd >= 0 && fchmod(fd, 0604) == 0 && mode_is(D "/geheim", S_IFREG | 0604, ALICE, STAFF) && fchmod(fd, 0600) == 0);
    close(fd);
    check("access: eigene Datei lesbar und schreibbar, nicht ausführbar", access(D "/geheim", R_OK | W_OK) == 0 && access(D "/geheim", X_OK) == -1 && errno == EACCES);
    return fails;
}

static int bob_is_kept_out(void) {
    struct stat sb;
    denied("Fremder: Datei 0600 lesen", open(D "/geheim", O_RDONLY), EACCES);
    denied("Fremder: Datei 0600 schreiben", open(D "/geheim", O_WRONLY), EACCES);
    denied("Fremder: Datei 0600 kürzen (O_TRUNC)", open(D "/geheim", O_RDONLY | O_TRUNC), EACCES);
    check("Fremder: stat zeigt Modus und Besitzer trotzdem", mode_is(D "/geheim", S_IFREG | 0600, ALICE, STAFF));
    denied("Fremder: access(R_OK)", access(D "/geheim", R_OK), EACCES);
    check("Fremder: access(F_OK) – die Datei gibt es", access(D "/geheim", F_OK) == 0);
    denied("Fremder: chmod auf fremde Datei", chmod(D "/geheim", 0666), EPERM);
    denied("Fremder: chown auf fremde Datei", chown(D "/geheim", BOB, OTHERS), EPERM);
    denied("Fremder: Datei im Verzeichnis 0700 öffnen", open(D "/privat/drin", O_RDONLY), EACCES);
    denied("Fremder: stat im Verzeichnis 0700", stat(D "/privat/drin", &sb), EACCES);
    denied("Fremder: Verzeichnis 0700 auflisten", open(D "/privat", O_RDONLY | O_DIRECTORY), EACCES);
    denied("Fremder: Datei im Verzeichnis 0700 anlegen", open(D "/privat/neu", O_WRONLY | O_CREAT, 0644), EACCES);
    denied("Fremder: Verzeichnis im Verzeichnis 0700 anlegen", mkdir(D "/privat/sub", 0755), EACCES);
    denied("Fremder: in das Verzeichnis 0700 wechseln", chdir(D "/privat"), EACCES);
    denied("Fremder: Datei im Verzeichnis 0700 löschen", unlink(D "/privat/drin"), EACCES);
    denied("Fremder: fremde Datei im Verzeichnis mit Klebe-Bit löschen", unlink(D "/geheim"), EPERM);
    denied("Fremder: fremde Datei im Verzeichnis mit Klebe-Bit umbenennen", rename(D "/geheim", D "/meins"), EPERM);
    denied("Fremder: Datei der Gruppe (0640) lesen", open(D "/gruppe", O_RDONLY), EACCES);
    check("Fremder: Programm 0750 einer fremden Gruppe startet nicht (EACCES)", runs(D "/prog") == 77);
    check("Fremder: eigene Datei im Verzeichnis mit Klebe-Bit anlegen und löschen geht", put_file(D "/bob", "bob", 0600) == 0 && mode_is(D "/bob", S_IFREG | 0600, BOB, OTHERS) && unlink(D "/bob") == 0);
    check("Fremder: nichts verändert", access(D "/meins", F_OK) == -1);
    return fails;
}

static int carol_in_group(void) {
    check("Gruppe: Mitglied über Zusatzgruppe liest die Datei 0640", file_is(D "/gruppe", "gruppe"));
    denied("Gruppe: schreiben erlaubt 0640 der Gruppe nicht", open(D "/gruppe", O_WRONLY), EACCES);
    check("Gruppe: Programm 0750 startet für das Mitglied", runs(D "/prog") == 0);
    denied("Gruppe: Datei 0600 bleibt zu", open(D "/geheim", O_RDONLY), EACCES);
    return fails;
}

/* uid 0 that has dropped the right FILE_ADMIN: no bypass, only what root owns */
static int root_without_right(void) {
    prctl(PR_ULTAR_RIGHTS_DROP, R_FILE_ADMIN, 0, 0, 0);
    denied("root ohne FILE_ADMIN: fremde Datei 0600 lesen", open(D "/geheim", O_RDONLY), EACCES);
    denied("root ohne FILE_ADMIN: chown", chown(D "/geheim", 0, 0), EPERM);
    denied("root ohne FILE_ADMIN: Verzeichnis 0700 eines anderen", open(D "/privat/drin", O_RDONLY), EACCES);
    check("root ohne FILE_ADMIN: eigene Datei geht", put_file(D "/wurzel", "w", 0600) == 0 && file_is(D "/wurzel", "w") && unlink(D "/wurzel") == 0);
    return fails;
}

/* own mount namespace: modes of tmpfs roots and symbolic links */
static int tmpfs_modes(void) {
    struct stat sb;
    check("tmpfs: mode=abc wird abgelehnt", mount("tmpfs", "/tmp", "tmpfs", 0, "mode=abc") == -1 && errno == EINVAL);
    check("tmpfs: ohne Angabe gehört die Wurzel root mit 0755", mount("tmpfs", "/tmp", "tmpfs", 0, NULL) == 0 && mode_is("/tmp", S_IFDIR | 0755, 0, 0));
    check("tmpfs: mode=1777 wie /tmp unter Linux", mkdir("/tmp/s", 0755) == 0 && mount("tmpfs", "/tmp/s", "tmpfs", 0, "size=1m,mode=1777") == 0 && mode_is("/tmp/s", S_IFDIR | 01777, 0, 0));
    check("tmpfs: mode=700,uid=1000,gid=100 für einen Benutzer", mkdir("/tmp/a", 0755) == 0 && mount("tmpfs", "/tmp/a", "tmpfs", 0, "mode=700,uid=1000,gid=100") == 0 && mode_is("/tmp/a", S_IFDIR | 0700, ALICE, STAFF));
    if (setresgid(OTHERS, OTHERS, OTHERS) || setresuid(BOB, BOB, BOB)) return 1;
    prctl(PR_ULTAR_RIGHTS_DROP, ~0UL, 0, 0, 0);
    denied("tmpfs: fremder Benutzer kommt nicht in das tmpfs 0700", stat("/tmp/a/x", &sb), EACCES);
    denied("tmpfs: in der Wurzel 0755 von root legt ein Benutzer nichts an", open("/tmp/neu", O_WRONLY | O_CREAT, 0644), EACCES);
    check("tmpfs: Verknüpfung gehört dem Anleger, Modus 0777", symlink("ziel", "/tmp/s/l") == 0 && mode_is("/tmp/s/l", S_IFLNK | 0777, BOB, OTHERS));
    check("tmpfs: Datei und Verzeichnis im tmpfs tragen Modus und Besitzer", put_file("/tmp/s/f", "f", 0640) == 0 && mode_is("/tmp/s/f", S_IFREG | 0640, BOB, OTHERS) && mkdir("/tmp/s/d", 0750) == 0 && mode_is("/tmp/s/d", S_IFDIR | 0750, BOB, OTHERS));
    check("tmpfs: umbenennen behält Modus und Besitzer", rename("/tmp/s/f", "/tmp/s/g") == 0 && mode_is("/tmp/s/g", S_IFREG | 0640, BOB, OTHERS) && lstat("/tmp/s/f", &sb) == -1);
    return fails;
}

static void mode_tests(void) {
    say("== Modus und Besitzer von Dateien\n");
    mkdir("/tmp", 0755);
    check("Vorbereitung: Verzeichnis mit Klebe-Bit (01777) durch root", mkdir(D, 0755) == 0 && chmod(D, 01777) == 0 && mode_is(D, S_IFDIR | 01777, 0, 0));
    as(ALICE, STAFF, 0, alice_files);
    as(BOB, OTHERS, 0, bob_is_kept_out);
    as(CAROL, 300, STAFF, carol_in_group);
    check("root mit FILE_ADMIN: liest jede Datei, auch im Verzeichnis 0700", file_is(D "/geheim", "geheim") && file_is(D "/privat/drin", "drin"));
    check("root mit FILE_ADMIN: Programm ohne jedes x-Bit startet trotzdem nicht", chmod(D "/prog", 0644) == 0 && runs(D "/prog") == 77 && chmod(D "/prog", 0750) == 0);
    int fd = open(D "/gruppe", O_RDONLY);
    check("root mit FILE_ADMIN: chown/fchown, stat zeigt den neuen Besitzer", fd >= 0 && fchown(fd, BOB, OTHERS) == 0 && mode_is(D "/gruppe", S_IFREG | 0640, BOB, OTHERS) && chown(D "/gruppe", ALICE, STAFF) == 0 && mode_is(D "/gruppe", S_IFREG | 0640, ALICE, STAFF));
    close(fd);
    denied("chmod auf etwas, das es nicht gibt", chmod(D "/fehlt", 0600), ENOENT);
    denied("chmod im Baum des Kerns (/proc)", chmod("/proc/meminfo", 0600), EPERM);
    pid_t c = fork();
    if (c == 0) {
        fails = 0;
        _exit(root_without_right() ? 1 : 0);
    }
    int st = -1;
    waitpid(c, &st, 0);
    if (st) fails++;
    c = syscall(SYS_clone, CLONE_NEWNS | SIGCHLD, 0, 0, 0, 0);
    if (c == 0) {
        fails = 0;
        _exit(tmpfs_modes() ? 1 : 0);
    }
    st = -1;
    waitpid(c, &st, 0);
    check("tmpfs: Prüfungen im eigenen Dateibaum liefen durch", WIFEXITED(st) && WEXITSTATUS(st) == 0);
}

/* ------------------------------------------------------------------ Unix sockets */

#define SOCK D "/sock"

static int unix_sock(const char *path, int do_bind) {
    struct sockaddr_un a = {.sun_family = AF_UNIX};
    int s = socket(AF_UNIX, SOCK_STREAM, 0);
    socklen_t len = sizeof a;
    if (path[0] == '@') {
        strcpy(a.sun_path + 1, path + 1);
        len = sizeof(sa_family_t) + strlen(path);
    } else {
        strcpy(a.sun_path, path);
    }
    int r = do_bind ? bind(s, (struct sockaddr *)&a, len) : connect(s, (struct sockaddr *)&a, len);
    if (r != 0) {
        int e = errno;
        close(s);
        errno = e;
        return -1;
    }
    return s;
}

static int peer_of(int s, struct ucred *u) {
    socklen_t len = sizeof *u;
    memset(u, 0xee, sizeof *u);
    return getsockopt(s, SOL_SOCKET, SO_PEERCRED, u, &len) == 0 && len == sizeof *u;
}

static int ready_fd, go_fd;

/* server (alice): bind, restrict the node to its owner, serve `n` connections by telling each client
 * who the kernel says it is */
static int server(void) {
    struct ucred u;
    int n = 3, l = unix_sock(SOCK, 1);
    check("Sockel: bind legt einen Knoten an (Art Socket, 0777 & ~umask, Besitzer = Binder)", l >= 0 && mode_is(SOCK, S_IFSOCK | 0755, ALICE, STAFF));
    denied("Sockel: zweites bind auf denselben Pfad", unix_sock(SOCK, 1), EADDRINUSE);
    denied("Sockel: bind auf eine vorhandene Datei", unix_sock(D "/prog", 1), EADDRINUSE);
    denied("Sockel: Knoten lässt sich nicht mit open öffnen", open(SOCK, O_RDWR), ENXIO);
    check("Sockel: lauschender Sockel hat keinen Gegenüber (pid 0, uid -1)", peer_of(l, &u) && u.pid == 0 && u.uid == (uid_t)-1 && u.gid == (gid_t)-1);
    check("Sockel: chmod 0600 auf den Knoten", chmod(SOCK, 0600) == 0 && listen(l, 8) == 0 && write(ready_fd, "r", 1) == 1);
    for (int i = 0; i < n; i++) {
        int c = accept(l, NULL, NULL);
        if (c < 0 || !peer_of(c, &u) || write(c, &u, sizeof u) != sizeof u) fails++;
        close(c);
    }
    char b;
    if (read(go_fd, &b, 1) < 0) fails++;
    return fails;
}

/* connect and compare: what the server saw of us (`my_pid_at_server` < 0: any pid), and what we see
 * of the server */
static int talk(pid_t server_pid, pid_t my_pid_at_server) {
    struct ucred mine, theirs;
    int s = unix_sock(SOCK, 0);
    if (s < 0) return 0;
    int ok = peer_of(s, &theirs) && theirs.pid == server_pid && theirs.uid == ALICE && theirs.gid == STAFF;
    ok = ok && read(s, &mine, sizeof mine) == sizeof mine && (my_pid_at_server < 0 || mine.pid == my_pid_at_server) && mine.uid == geteuid() && mine.gid == getegid();
    close(s);
    return ok;
}

static pid_t the_server;
static int bob_connects(void) {
    denied("Sockel: fremder Benutzer verbindet sich mit dem Knoten 0600", unix_sock(SOCK, 0), EACCES);
    denied("Sockel: bind in einem Verzeichnis ohne Schreibrecht", unix_sock(D "/privat/s", 1), EACCES);
    return fails;
}
static int alice_connects(void) {
    check("Sockel: der Besitzer verbindet sich; beide Seiten sehen pid, uid, gid des anderen (SO_PEERCRED)", talk(the_server, getpid()));
    return fails;
}

static void socket_tests(void) {
    int ready[2], go[2], sv[2];
    char b;
    struct ucred u;
    say("== Unix-Sockel: Knoten, Zugriff, Gegenüber\n");
    if (pipe(ready) || pipe(go)) return;
    ready_fd = ready[1], go_fd = go[0];
    the_server = fork();
    if (the_server == 0) {
        fails = 0;
        if (setresgid(STAFF, STAFF, STAFF) || setresuid(ALICE, ALICE, ALICE)) _exit(9);
        prctl(PR_ULTAR_RIGHTS_DROP, ~0UL, 0, 0, 0);
        _exit(server() ? 1 : 0);
    }
    if (read(ready[0], &b, 1) != 1) fails++;
    as(BOB, OTHERS, 0, bob_connects);
    as(ALICE, STAFF, 0, alice_connects);
    check("Sockel: root mit FILE_ADMIN verbindet sich; der Dienst sieht uid 0", talk(the_server, getpid()));
    /* from a pid namespace: the server's pid is not visible there (0), the server sees the real pid */
    pid_t c = syscall(SYS_clone, CLONE_NEWPID | SIGCHLD, 0, 0, 0, 0);
    if (c == 0) _exit(getpid() == 1 && talk(0, -1) ? 0 : 1);
    int st = -1;
    waitpid(c, &st, 0);
    check("Sockel: aus einem pid-Namensraum ist die pid des Dienstes 0 (dort unsichtbar), uid/gid stimmen", st == 0);
    if (write(go[1], "g", 1) != 1) fails++;
    st = -1;
    waitpid(the_server, &st, 0);
    check("Sockel: Dienst lief durch", st == 0);
    check("Sockel: nach dem Ende des Dienstes bleibt der Knoten", mode_is(SOCK, S_IFSOCK | 0600, ALICE, STAFF));
    denied("Sockel: verbinden ohne Lauscher", unix_sock(SOCK, 0), ECONNREFUSED);
    denied("Sockel: bind auf den verwaisten Knoten", unix_sock(SOCK, 1), EADDRINUSE);
    int l = -1;
    check("Sockel: nach unlink geht bind wieder", unlink(SOCK) == 0 && (l = unix_sock(SOCK, 1)) >= 0 && mode_is(SOCK, S_IFSOCK | 0755, 0, 0));
    check("Sockel: unlink bei lebendem Lauscher – Pfad frei, verbinden -> ENOENT", unlink(SOCK) == 0 && unix_sock(SOCK, 0) == -1 && errno == ENOENT);
    close(l);
    denied("Sockel: verbinden mit einer gewöhnlichen Datei", unix_sock(D "/prog", 0), ECONNREFUSED);
    check("Sockel: socketpair – jedes Ende sieht den eigenen Prozess", socketpair(AF_UNIX, SOCK_STREAM, 0, sv) == 0 && peer_of(sv[0], &u) && u.pid == getpid() && u.uid == 0 && peer_of(sv[1], &u) && u.pid == getpid());
    socklen_t small = 4;
    denied("Sockel: SO_PEERCRED mit zu kleinem Puffer", getsockopt(sv[0], SOL_SOCKET, SO_PEERCRED, &u, &small), EINVAL);
    int a1 = unix_sock("@rt-abstrakt", 1), a2 = -1;
    check("Sockel: abstrakter Name – kein Knoten, zweites bind EADDRINUSE, verbinden geht", a1 >= 0 && listen(a1, 2) == 0 && unix_sock("@rt-abstrakt", 1) == -1 && errno == EADDRINUSE && (a2 = unix_sock("@rt-abstrakt", 0)) >= 0 && access("rt-abstrakt", F_OK) == -1);
    close(a1), close(a2), close(sv[0]), close(sv[1]);
}

/* ------------------------------------------------------------------ fsync */

static void sync_tests(void) {
    struct stat sb;
    int p[2];
    say("== fsync\n");
    int fd = open(D "/sync", O_WRONLY | O_CREAT, 0644);
    check("fsync: geschriebene Daten liegen bis dahin nur im Deskriptor (andere sehen Länge 0)", fd >= 0 && write(fd, "abc", 3) == 3 && stat(D "/sync", &sb) == 0 && sb.st_size == 0);
    check("fsync: danach sehen andere die Daten", fsync(fd) == 0 && file_is(D "/sync", "abc"));
    check("fdatasync: ebenso nach weiterem Schreiben", write(fd, "def", 3) == 3 && fdatasync(fd) == 0 && file_is(D "/sync", "abcdef"));
    check("fsync: ohne Änderung und auf ein Verzeichnis -> 0", fsync(fd) == 0 && close(fd) == 0 && (fd = open(D, O_RDONLY | O_DIRECTORY)) >= 0 && fsync(fd) == 0);
    close(fd);
    denied("fsync: auf ein Rohr", pipe(p) == 0 ? fsync(p[1]) : 0, EINVAL);
    denied("fsync: ungültiger Deskriptor", fsync(987), EBADF);
    sync();
    check("sync: kehrt zurück", 1);
}

static const char *ext4_dir;

/* a user's private file as a service stores it (written as .name.new with 0600, fsync, rename): mode and
 * owner go with the rename, and the owner may chmod the renamed file (the device mesh keeps its key so) */
static int ext4_private(void) {
    char tmp[160], fin[160];
    snprintf(tmp, sizeof tmp, "%s/rt-eigen/.schluessel.new", ext4_dir);
    snprintf(fin, sizeof fin, "%s/rt-eigen/schluessel", ext4_dir);
    int fd = open(tmp, O_WRONLY | O_CREAT | O_TRUNC, 0600);
    check("ext4: eigene Datei 0600 anlegen, schreiben, fsync", fd >= 0 && write(fd, "geheim", 6) == 6 && fsync(fd) == 0 && close(fd) == 0);
    check("ext4: die neue Datei gehört dem, der sie anlegte (0600)", mode_is(tmp, S_IFREG | 0600, ALICE, STAFF));
    check("ext4: rename nimmt Modus und Besitzer mit", rename(tmp, fin) == 0 && mode_is(fin, S_IFREG | 0600, ALICE, STAFF));
    check("ext4: der Besitzer darf chmod nach dem rename", chmod(fin, 0400) == 0 && mode_is(fin, S_IFREG | 0400, ALICE, STAFF));
    return fails;
}

/* write below `dir` (a writable ext4), then wait: the host looks at the disk while the file is still open */
static int ext4_part(const char *dir) {
    char path[128], sub[128];
    static char block[8192];
    snprintf(path, sizeof path, "%s/rt-dauer", dir);
    snprintf(sub, sizeof sub, "%s/rt-ordner", dir);
    for (unsigned i = 0; i < sizeof block; i++) block[i] = 'A' + i % 23;
    int fd = open(path, O_WRONLY | O_CREAT | O_TRUNC, 0600);
    check("ext4: Datei anlegen und 8 KiB schreiben", fd >= 0 && write(fd, block, sizeof block) == sizeof block);
    check("ext4: fsync", fsync(fd) == 0);
    check("ext4: chmod 0640 und chown 1000:100 stehen im Inode (stat)", chmod(path, 0640) == 0 && chown(path, ALICE, STAFF) == 0 && mode_is(path, S_IFREG | 0640, ALICE, STAFF));
    check("ext4: Verzeichnis 0750 für 2000:200", mkdir(sub, 0750) == 0 && chown(sub, BOB, OTHERS) == 0 && mode_is(sub, S_IFDIR | 0750, BOB, OTHERS));
    char own[128];
    snprintf(own, sizeof own, "%s/rt-eigen", dir);
    ext4_dir = dir;
    check("ext4: Verzeichnis 0700 für 1000:100", mkdir(own, 0700) == 0 && chown(own, ALICE, STAFF) == 0);
    as(ALICE, STAFF, 0, ext4_private);
    say("RECHTETEST-EXT4: %s\n", fails == 0 ? "GESCHRIEBEN" : "SIEHE OBEN");
    for (;;) pause();
    return 0;
}

static void cleanup(void) {
    static const char *f[] = {"/geheim", "/privat/drin", "/gruppe", "/prog", "/sync", "/meins"};
    for (unsigned i = 0; i < sizeof f / sizeof *f; i++) {
        char b[96];
        snprintf(b, sizeof b, D "%s", f[i]);
        unlink(b);
    }
    rmdir(D "/privat");
    rmdir(D);
}

int main(int argc, char **argv) {
    if (access("/bin/hallo", F_OK) != 0) bin_dir = "/boot/efi/bin";
    if (argc > 1 && !strcmp(argv[1], "--ids")) return ids_after_exec();
    if (argc > 2 && !strcmp(argv[1], "--ext4")) return ext4_part(argv[2]);
    static char self[96];
    snprintf(self, sizeof self, "%s/rechtetest", bin_dir);
    say("RECHTETEST: Kennungen, Dateirechte, Unix-Sockel, fsync\n");
    check("Vorbereitung: root mit allen Rechten", (unsigned long)prctl(PR_ULTAR_RIGHTS_GET, 0, 0, 0, 0) == R_ALL && getuid() == 0);
    id_tests(self);
    mode_tests();
    socket_tests();
    sync_tests();
    cleanup();
    check("Aufräumen: Prüfverzeichnis ist wieder weg", access(D, F_OK) == -1);
    say("RECHTETEST: %d Fehler\n", fails);
    say("RECHTETEST: %s\n", fails == 0 ? "ALLES OK" : "SIEHE OBEN");
    return fails ? 1 : 0;
}
