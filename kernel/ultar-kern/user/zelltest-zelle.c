/* zelltest, the cell side: everything a process inside the cell (new mount + pid namespace, own root)
 * tries – escapes that must fail and ordinary work that must succeed. See zelltest.c. */
#include "zelltest.h"

static void sync_point(char tag) {
    char ack = 0;
    if (write(to_parent[1], &tag, 1) != 1 || read(to_cell[0], &ack, 1) != 1 || ack != tag) fails++;
}


/* (3) a mount in the cell's namespace hides /bin for the cell only (the parent looks meanwhile) */
static void cell_mount_over_host(void) {
    check("Zelle: MS_PRIVATE auf / angenommen", mount("none", "/", NULL, MS_REC | MS_PRIVATE, NULL) == 0);
    check("Zelle: tmpfs über /bin eingehängt", mount("tmpfs", bin_dir, "tmpfs", 0, NULL) == 0);
    denied("Zelle: /bin/kerntest ist für die Zelle verdeckt", access(pf("%s/kerntest", bin_dir), F_OK), ENOENT);
    sync_point('m'); /* parent checks that its /bin is untouched */
    denied("Zelle: aushängen von / verweigert", umount2("/", 0), EINVAL);
    denied("Zelle: aushängen eines Nicht-Einhängepunkts", umount2("/tmp", 0), EINVAL);
    check("Zelle: /bin wieder ausgehängt", umount2(bin_dir, 0) == 0 && access(pf("%s/kerntest", bin_dir), F_OK) == 0);
    denied("Zelle: unbekanntes Dateisystem", mount("x", "/tmp", "ext4", 0, NULL), ENODEV);
}

/* own root: private /tmp, tmpfs root with /bin bound read-only, pivot_root, old tree detached */
static void cell_enter_root(void) {
    check("Zelle: privates /tmp", mount("tmpfs", "/tmp", "tmpfs", 0, NULL) == 0);
    denied("Zelle: Datei des Elternprozesses in /tmp ist weg", access(SECRET, F_OK), ENOENT);
    check("Zelle: Datei im privaten /tmp anlegen", put_file("/tmp/nur-zelle", "zelle") == 0);
    int ok = mkdir("/tmp/wurzel", 0755) == 0 && mount("tmpfs", "/tmp/wurzel", "tmpfs", 0, NULL) == 0;
    ok = ok && mkdir("/tmp/wurzel/alt", 0755) == 0 && mkdir("/tmp/wurzel/bin", 0755) == 0 && mkdir("/tmp/wurzel/proc", 0755) == 0;
    ok = ok && mkdir("/tmp/wurzel/dev", 0755) == 0 && mkdir("/tmp/wurzel/mnt", 0755) == 0;
    check("Zelle: Wurzel vorbereitet (tmpfs + Ordner)", ok);
    check("Zelle: /bin nur lesbar eingebunden", mount(bin_dir, "/tmp/wurzel/bin", NULL, MS_BIND | MS_RDONLY, NULL) == 0);
    denied("Zelle: pivot_root auf Nicht-Einhängepunkt", syscall(SYS_pivot_root, "/tmp/wurzel/alt", "/tmp/wurzel/alt/x"), mkdir("/tmp/wurzel/alt/x", 0755) == 0 ? EINVAL : 0);
    denied("Zelle: pivot_root, put_old ausserhalb", syscall(SYS_pivot_root, "/tmp/wurzel", "/tmp"), EINVAL);
    int pre = open("/", O_RDONLY | O_DIRECTORY); /* opened by the cell itself before pivot_root */
    check("Zelle: pivot_root", syscall(SYS_pivot_root, "/tmp/wurzel", "/tmp/wurzel/alt") == 0 && chdir("/") == 0);
    check("Zelle: alter Baum liegt unter /alt", access(pf("/alt%s", outside_path), F_OK) == 0);
    denied("Zelle: alter Baum hat Untereinhängungen", umount2("/alt", 0), EBUSY);
    check("Zelle: alter Baum abgehängt (MNT_DETACH)", umount2("/alt", MNT_DETACH) == 0);
    denied("Zelle: /alt ist leer", access(pf("/alt%s", outside_path), F_OK), ENOENT);
    denied("Ausbruch fd: vor pivot_root geöffnetes / + openat", openat(pre, outside_path + 1, O_RDONLY), EXDEV);
    close(pre);
    check("Zelle: /proc und /dev eingehängt", mount("proc", "/proc", "proc", 0, NULL) == 0 && mount("dev", "/dev", "devtmpfs", 0, NULL) == 0);
}

/* (1) reading outside the root */
static void cell_escape_files(void) {
    denied("Ausbruch ..: /../../.. + Pfad der Datei ausserhalb", open(pf("/../../..%s", outside_path), O_RDONLY), ENOENT);
    denied("Ausbruch ..: ../../../../tmp/zell-geheim", open("../../../.." SECRET, O_RDONLY), ENOENT);
    check("Zelle: chdir /bin", chdir("/bin") == 0);
    denied("Ausbruch ..: aus /bin nach ../.. + Pfad ausserhalb", open(pf("../..%s", outside_path), O_RDONLY), ENOENT);
    check("Zelle: .. endet an der Wurzel (/../../bin/hallo)", chdir("/") == 0 && access("/../../bin/hallo", X_OK) == 0);
    check("Zelle: symlink anlegen", symlink(outside_path, "/flucht") == 0 && symlink("../../../.." SECRET, "/flucht2") == 0);
    denied("Ausbruch Verknüpfung: absolut auf die Datei ausserhalb", open("/flucht", O_RDONLY), ENOENT);
    denied("Ausbruch Verknüpfung: relativ mit ..", open("/flucht2", O_RDONLY), ENOENT);
    check("Zelle: Schleife anlegen", symlink("/kreis", "/kreis") == 0);
    denied("Ausbruch Verknüpfung: Schleife", open("/kreis", O_RDONLY), ELOOP);
    denied("Ausbruch fd: geerbtes Verzeichnis + openat", openat(outside_dir, "kerntest", O_RDONLY), EXDEV);
    denied("Ausbruch fd: geerbtes Verzeichnis + ../liesmich.txt", openat(outside_dir, "../liesmich.txt", O_RDONLY), EXDEV);
    struct stat st;
    denied("Ausbruch fd: geerbtes Verzeichnis + fstatat", fstatat(outside_dir, "kerntest", &st, 0), EXDEV);
    denied("Ausbruch fd: geerbtes Verzeichnis + mkdirat", mkdirat(outside_dir, "zell-ordner", 0755), EXDEV);
    denied("Ausbruch fd: geerbtes Verzeichnis + unlinkat", unlinkat(outside_dir, "kerntest", 0), EXDEV);
    denied("Ausbruch fd: absoluter Pfad mit geerbtem fd", openat(outside_dir, outside_path, O_RDONLY), ENOENT);
    check("Ausbruch fd: fchdir auf geerbtes Verzeichnis scheitert", fchdir(outside_dir) == -1);
    char b[16] = {0};
    check("Zelle: geerbte offene DATEI bleibt lesbar (Übergabe)", read(outside_file, b, 10) == 10 && !memcmp(b, "Willkommen", 10));
}

/* positive: ordinary file work inside the cell */
static void cell_files_work(void) {
    char b[64] = {0};
    struct stat st;
    check("Zelle: Datei schreiben und lesen", put_file("/daten.txt", "zellinhalt") == 0 && file_is("/daten.txt", "zellinhalt"));
    int ok = symlink("/daten.txt", "/zeiger") == 0 && file_is("/zeiger", "zellinhalt");
    ok = ok && readlink("/zeiger", b, sizeof b - 1) == 10 && !strcmp(b, "/daten.txt");
    check("Zelle: Verknüpfung innerhalb der Zelle (lesen, readlink, lstat)", ok && lstat("/zeiger", &st) == 0 && S_ISLNK(st.st_mode) && stat("/zeiger", &st) == 0 && S_ISREG(st.st_mode));
    denied("Zelle: O_NOFOLLOW auf Verknüpfung", open("/zeiger", O_RDONLY | O_NOFOLLOW), ELOOP);
    check("Zelle: rename ersetzt eine Verknüpfung, statt durch sie zu schreiben", put_file("/neu.txt", "neu") == 0 && rename("/neu.txt", "/zeiger") == 0 && lstat("/zeiger", &st) == 0 && S_ISREG(st.st_mode) && file_is("/zeiger", "neu") && file_is("/daten.txt", "zellinhalt") && unlink("/zeiger") == 0);
    check("Zelle: mkdir, rename, unlink", mkdir("/ordner", 0755) == 0 && rename("/daten.txt", "/ordner/d2") == 0 && file_is("/ordner/d2", "zellinhalt") && unlink("/ordner/d2") == 0 && access("/ordner/d2", F_OK) == -1);
    int d = open("/bin", O_RDONLY | O_DIRECTORY);
    int f = openat(d, "hallo", O_RDONLY);
    check("Zelle: openat mit eigenem Verzeichnis-fd", d >= 0 && f >= 0);
    denied("Ausbruch fd: eigenes Verzeichnis + ../.. + Pfad ausserhalb", openat(d, pf("../..%s", outside_path), O_RDONLY), ENOENT);
    close(f);
    close(d);
    denied("Zelle: nur lesbar eingebundenes /bin (anlegen)", open("/bin/neu", O_WRONLY | O_CREAT, 0644), EROFS);
    denied("Zelle: nur lesbar eingebundenes /bin (mkdir)", mkdir("/bin/ordner", 0755), EROFS);
    denied("Zelle: nur lesbar eingebundenes /bin (löschen)", unlink("/bin/hallo"), EROFS);
    check("Zelle: getcwd ist /", getcwd(b, sizeof b) && !strcmp(b, "/"));
}

static int count_proc_pids(int *only) {
    DIR *d = opendir("/proc");
    int n = 0;
    struct dirent *e;
    if (!d) return -1;
    while ((e = readdir(d))) {
        if (e->d_name[0] >= '0' && e->d_name[0] <= '9') {
            n++;
            *only = atoi(e->d_name);
        }
    }
    closedir(d);
    return n;
}

/* (2) + (5) processes outside: not visible, not signalable, no /proc entry */
static void cell_escape_pids(void) {
    char p[64];
    int only = 0, bad = 0;
    check("Zelle: erster Prozess ist pid 1, ohne sichtbaren Elternprozess", getpid() == 1 && getppid() == 0 && syscall(SYS_gettid) == 1);
    check("Zelle: /proc zeigt genau einen Prozess (1)", count_proc_pids(&only) == 1 && only == 1);
    for (int i = 2; i <= 300; i++) {
        if (kill(i, 0) != -1 || errno != ESRCH) bad++;
    }
    check("Ausbruch pid: kill(2..300, 0) -> ESRCH (kein Prozess ausserhalb sichtbar)", bad == 0);
    denied("Ausbruch pid: kill(-1, SIGKILL) trifft niemanden", kill(-1, SIGKILL), ESRCH);
    denied("Ausbruch pid: tgkill auf Nummer des Zuschauers", syscall(SYS_tgkill, host_bystander, host_bystander, SIGKILL), ESRCH);
    denied("Ausbruch pid: tkill auf Nummer des Zuschauers", syscall(SYS_tkill, host_bystander, SIGKILL), ESRCH);
    denied("Ausbruch pid: waitpid auf fremde Nummer", waitpid(host_bystander, NULL, WNOHANG), ECHILD);
    snprintf(p, sizeof p, "/proc/%d/status", host_bystander);
    denied("Ausbruch /proc: Eintrag des Zuschauers (Wirts-Nummer)", open(p, O_RDONLY), ENOENT);
    snprintf(p, sizeof p, "/proc/%d/ns/mnt", host_bystander);
    denied("Ausbruch /proc: Namensraum-Griff des Zuschauers", open(p, O_RDONLY), ENOENT);
    if (host_parent != 1) {
        snprintf(p, sizeof p, "/proc/%d/cmdline", host_parent);
        denied("Ausbruch /proc: Eintrag des Elternprozesses", open(p, O_RDONLY), ENOENT);
    }
    denied("Ausbruch /proc: Kernprotokoll /proc/ultar/protokoll", open("/proc/ultar/protokoll", O_RDONLY), ENOENT);
    denied("Ausbruch /proc: Brandmauer /proc/ultar/firewall", open("/proc/ultar/firewall", O_WRONLY), ENOENT);
    check("Zelle: /proc/self/status gehört pid 1", file_is("/proc/self/comm", "zelltest\n"));
    check("Zelle: /proc/mounts zeigt die eigene Tabelle", access("/proc/mounts", R_OK) == 0);
}

static int child_identity(void) {
    check("Zelle: Kind ist pid 2 mit Elternprozess 1", getpid() == 2 && getppid() == 1);
    /* pid 1 is shielded from its own namespace: reported as sent, but dropped */
    check("Zelle: kill(1, SIGKILL) aus der Zelle bleibt wirkungslos", kill(1, SIGKILL) == 0 && kill(1, SIGTERM) == 0);
    return 0;
}

/* positive: processes inside the cell */
static void cell_processes_work(void) {
    int st = 0;
    sub(child_identity);
    check("Zelle: pid 1 lebt nach kill aus der Zelle", getpid() == 1);
    pid_t c = fork();
    if (c == 0) _exit(idle());
    check("Zelle: Kind beenden (kill) und abholen (waitpid)", c > 1 && kill(c, 0) == 0 && kill(c, SIGKILL) == 0 && waitpid(c, &st, 0) == c && WIFSIGNALED(st) && WTERMSIG(st) == SIGKILL);
    c = fork();
    if (c == 0) execl("/bin/hallo", "hallo", (char *)0), _exit(99);
    check("Zelle: Programm aus /bin starten (execve)", waitpid(c, &st, 0) == c && WIFEXITED(st) && WEXITSTATUS(st) != 99);
}

/* after execve inside the cell (argument --rechte): nothing came back */
int after_exec(void) {
    return rights() == 0 && mount("tmpfs", "/mnt", "tmpfs", 0, NULL) == -1 && errno == EPERM ? 0 : 1;
}

static int child_keeps_no_rights(void) {
    check("Rechte: Kind erbt die leere Menge", rights() == 0);
    denied("Rechte: Kind darf nicht einhängen", mount("tmpfs", "/mnt", "tmpfs", 0, NULL), EPERM);
    execl("/bin/zelltest", "zelltest", "--rechte", (char *)0);
    return 1;
}

/* (4) dropped rights stay dropped */
static int cell_drop_rights(void) {
    struct timeval tv = {1000, 0};
    check("Rechte: Zelle hält anfangs alle", rights() == R_ALL && prctl(PR_CAPBSET_READ, 21, 0, 0, 0) == 1);
    check("Rechte: Linux-Weg PR_CAPBSET_DROP(CAP_SYS_ADMIN)", prctl(PR_CAPBSET_DROP, 21, 0, 0, 0) == 0 && rights() == (R_ALL & ~R_MOUNT));
    check("Rechte: alle abgeben", prctl(PR_ULTAR_RIGHTS_DROP, R_ALL, 0, 0, 0) == 0 && rights() == 0);
    denied("Rechte: mount ohne Recht", mount("tmpfs", "/mnt", "tmpfs", 0, NULL), EPERM);
    denied("Rechte: umount2 ohne Recht", umount2("/bin", MNT_DETACH), EPERM);
    denied("Rechte: unshare(CLONE_NEWNS) ohne Recht", unshare(CLONE_NEWNS), EPERM);
    denied("Rechte: clone(CLONE_NEWPID) ohne Recht", syscall(SYS_clone, CLONE_NEWPID | SIGCHLD, 0, 0, 0, 0), EPERM);
    denied("Rechte: chroot ohne Recht", chroot("/ordner"), EPERM);
    denied("Rechte: pivot_root ohne Recht", syscall(SYS_pivot_root, "/", "/alt"), EPERM);
    denied("Rechte: setns in den geerbten Wirts-Namensraum", setns(host_mnt_ns, CLONE_NEWNS), EPERM);
    denied("Rechte: reboot ohne Recht", syscall(SYS_reboot, 0xfee1dead, 672274793, 0, 0), EPERM);
    denied("Rechte: Uhr stellen ohne Recht", settimeofday(&tv, NULL), EPERM);
    denied("Rechte: Treiber laden ohne Recht", syscall(SYS_init_module, "", 0, ""), EPERM);
    if (access("/dev/fb0", F_OK) == 0) denied("Rechte: Rohgerät /dev/fb0 ohne Recht", open("/dev/fb0", O_RDONLY), EACCES);
    check("Zelle: /dev/null bleibt nutzbar", access("/dev/null", F_OK) == 0 && put_file("/dev/null", "x") == 0);
    /* attempts to get a right back */
    prctl(PR_ULTAR_RIGHTS_DROP, 0, 0, 0, 0);
    prctl(PR_CAP_AMBIENT, PR_CAP_AMBIENT_RAISE, 21, 0, 0);
    prctl(PR_SET_KEEPCAPS, 1, 0, 0, 0);
    check("Rechte: nichts zurückgewonnen (prctl-Versuche)", rights() == 0 && prctl(PR_CAPBSET_READ, 21, 0, 0, 0) == 0);
    denied("Rechte: mount weiterhin verweigert", mount("tmpfs", "/mnt", "tmpfs", 0, NULL), EPERM);
    sub(child_keeps_no_rights); /* fork + execve */
    return 0;
}

static pid_t sibling;

static int limited_killer(void) {
    check("Rechte: KILL_ANY abgegeben", prctl(PR_ULTAR_RIGHTS_DROP, R_KILL_ANY, 0, 0, 0) == 0);
    denied("Rechte: Geschwisterprozess beenden ohne KILL_ANY", kill(sibling, SIGKILL), EPERM);
    denied("Rechte: pid 1 der Zelle ansprechen ohne KILL_ANY", kill(1, 0), EPERM);
    pid_t c = fork();
    int st = 0;
    if (c == 0) _exit(idle());
    check("Rechte: eigenes Kind und sich selbst darf man weiter", kill(getpid(), 0) == 0 && kill(c, SIGKILL) == 0 && waitpid(c, &st, 0) == c && WIFSIGNALED(st));
    return 0;
}

int cell_main(void) {
    cell_mount_over_host();
    cell_enter_root();
    cell_escape_files();
    cell_files_work();
    cell_escape_pids();
    cell_processes_work();
    sibling = fork();
    if (sibling == 0) _exit(idle());
    sub(limited_killer);
    check("Zelle: Geschwisterprozess lebt noch", kill(sibling, 0) == 0);
    sync_point('p'); /* parent: enters the cell with setns, notes the idle child's real pid */
    sub(cell_drop_rights);
    check("Zelle: pid 1 hält seine Rechte (Abgabe wirkt nur im Kind)", rights() == R_ALL);
    return 0; /* pid 1 ends with the idle child still running: the kernel must end it */
}

/* A second, small cell. Several run at the same time (races between namespaces): each must be pid 1 and
 * see only the file it wrote itself. */
int mini_cell(int nr) {
    const char *mine = pf("/marke-%d", nr);
    int ok = getpid() == 1 && mount("tmpfs", "/tmp", "tmpfs", 0, NULL) == 0;
    ok = ok && mkdir("/tmp/w", 0755) == 0 && mount("tmpfs", "/tmp/w", "tmpfs", 0, NULL) == 0 && mkdir("/tmp/w/alt", 0755) == 0;
    ok = ok && syscall(SYS_pivot_root, "/tmp/w", "/tmp/w/alt") == 0 && chdir("/") == 0 && umount2("/alt", MNT_DETACH) == 0;
    for (int round = 0; ok && round < 20; round++) {
        ok = put_file(mine, "x") == 0 && symlink(mine, "/zeiger") == 0 && file_is("/zeiger", "x") && unlink("/zeiger") == 0 && unlink(mine) == 0;
        for (int other = 0; ok && other < 8; other++) {
            if (other != nr && access(pf("/marke-%d", other), F_OK) == 0) ok = 0;
        }
        pid_t c = fork();
        int st = 0;
        if (c == 0) _exit(getpid() == 2 + round && getppid() == 1 ? 0 : 1);
        ok = ok && c == 2 + round && waitpid(c, &st, 0) == c && WIFEXITED(st) && WEXITSTATUS(st) == 0;
    }
    return ok && access(outside_path, F_OK) == -1 ? 0 : 1;
}
