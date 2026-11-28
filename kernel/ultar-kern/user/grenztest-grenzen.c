/* grenztest, part 2: explicit limits (`memory.max`, `pids.max`), the group tree as files, the right
 * MANAGE_GROUPS, the automatic group of a cell, and that the accounting returns to zero. */
#include "grenztest.h"

#define G_MEM CG "/apps/grenz-speicher"
#define G_PIDS CG "/apps/grenz-prozesse"

/* child: join the group, then take memory in 1 MiB steps and report each step on `fd` */
static void eat_in_group(const char *group, int fd, long steps) {
    if (put_text(pf("%s/cgroup.procs", group), "0") != 0) _exit(40);
    for (long i = 0; i < steps; i++) {
        if (!eat(1)) _exit(41);
        if (write(fd, "x", 1) != 1) _exit(42);
    }
    _exit(0);
}

static void memory_limit(void) {
    const long limit = 32 * MB;
    check("Gruppe: anlegen (mkdir apps/grenz-speicher)", mkdir(G_MEM, 0755) == 0 && num_at(G_MEM "/memory.current") == 0 && num_at(G_MEM "/memory.max") == -2);
    check("Gruppe: memory.max = 32 MiB setzen und lesen", put_text(G_MEM "/memory.max", "32M") == 0 && num_at(G_MEM "/memory.max") == limit);
    int p[2];
    if (pipe(p)) return;
    pid_t c = fork();
    if (c == 0) {
        close(p[0]);
        eat_in_group(G_MEM, p[1], 64);
    }
    close(p[1]);
    long steps = 0, most = 0;
    char b;
    while (read(p[0], &b, 1) == 1) {
        steps++;
        long now = num_at(G_MEM "/memory.current");
        if (now > most) most = now;
    }
    close(p[0]);
    int st = -1;
    waitpid(c, &st, 0);
    long peak = num_at(G_MEM "/memory.peak");
    say("       (%ld MiB bekommen; memory.peak %ld, höchster gelesener Stand %ld, Grenze %ld)\n", steps, peak, most, limit);
    check("Grenze: Prozess wird an der Grenze beendet (SIGKILL), nicht der Rechner", WIFSIGNALED(st) && WTERMSIG(st) == SIGKILL);
    check("Grenze: genau bis zur Grenze – 31 MiB bekommen, das 32. nicht mehr ganz", steps == 31);
    check("Grenze: memory.current und memory.peak nie über memory.max", most <= limit && peak <= limit && peak > limit - MB);
    check("Grenze: memory.events zählt Ablehnung und Beendigung", keyed_num(G_MEM "/memory.events", "max") >= 1 && keyed_num(G_MEM "/memory.events", "oom_kill") == 1);
    say("       (nach dem Ende: memory.current %ld, pids.current %ld)\n", num_at(G_MEM "/memory.current"), num_at(G_MEM "/pids.current"));
    check("Abrechnung: nach dem Ende des Prozesses memory.current = 0 und pids.current = 0", num_at(G_MEM "/memory.current") == 0 && num_at(G_MEM "/pids.current") == 0);
}

/* child: in a group with 8 MiB, system calls that need more are refused with ENOMEM and the process lives */
static int refused_calls(void) {
    if (put_text(G_MEM "/cgroup.procs", "0") != 0) return 40;
    void *old = sbrk(0);
    int ok = sbrk(16 * MB) == (void *)-1 && errno == ENOMEM && sbrk(0) == old;
    check("Grenze 8 MiB: brk um 16 MiB -> ENOMEM, Grenze des Programms unverändert", ok);
    check("Grenze 8 MiB: brk um 2 MiB geht", sbrk(2 * MB) != (void *)-1 && sbrk(-2 * MB) != (void *)-1);
    int fd = open("/tmp/grenz-datei", O_WRONLY | O_CREAT | O_TRUNC, 0644);
    long mb = 0;
    ssize_t r;
    while ((r = write(fd, block, sizeof block)) == sizeof block && mb < 64) mb++;
    int e = errno;
    say("       (Datei in der RAM-Schicht: %ld MiB geschrieben, dann %s)\n", mb, strerror(e));
    check("Grenze 8 MiB: Schreiben in eine RAM-Datei endet mit ENOMEM vor 8 MiB", r == -1 && e == ENOMEM && mb >= 2 && mb < 8);
    close(fd);
    unlink("/tmp/grenz-datei");
    int p[2];
    check("Grenze 8 MiB: kleine Aufrufe gehen weiter (pipe, fork)", pipe(p) == 0 && write(p[1], "a", 1) == 1);
    pid_t k = fork();
    if (k == 0) _exit(3);
    int st = -1;
    check("Grenze 8 MiB: Kindprozess läuft und endet", k > 0 && waitpid(k, &st, 0) == k && WIFEXITED(st) && WEXITSTATUS(st) == 3);
    return fails ? 1 : 0;
}

static void memory_limit_calls(void) {
    check("Gruppe: memory.max = 8 MiB", put_text(G_MEM "/memory.max", "8388608") == 0);
    mkdir("/tmp", 0755);
    pid_t c = fork();
    if (c == 0) _exit(refused_calls());
    int st = -1;
    check("Grenze 8 MiB: Prozess lief durch (nicht beendet)", waitpid(c, &st, 0) == c && WIFEXITED(st) && WEXITSTATUS(st) < 40);
    if (WIFEXITED(st) && WEXITSTATUS(st)) fails++;
    say("       (nach dem Ende: memory.current %ld, pids.current %ld)\n", num_at(G_MEM "/memory.current"), num_at(G_MEM "/pids.current"));
    check("Abrechnung: memory.current = 0, pids.current = 0", num_at(G_MEM "/memory.current") == 0 && num_at(G_MEM "/pids.current") == 0);
    check("Gruppe: entfernen (rmdir), Verzeichnis ist weg", rmdir(G_MEM) == 0 && access(G_MEM, F_OK) == -1);
}

/* child: join the group with pids.max = 6 and fork until refused */
static int fork_until_refused(void) {
    if (put_text(G_PIDS "/cgroup.procs", "0") != 0) return 40;
    pid_t kids[32];
    int n = 0, e = 0;
    while (n < 32) {
        pid_t k = fork();
        if (k < 0) {
            e = errno;
            break;
        }
        if (k == 0) {
            for (;;) pause();
        }
        kids[n++] = k;
    }
    long counted = num_at(G_PIDS "/pids.current");
    say("       (%d Kinder, dann %s; pids.current %ld)\n", n, strerror(e), counted);
    check("Grenze pids.max = 6: genau 5 Kinder neben dem Prozess selbst, das 6. -> EAGAIN", n == 5 && e == EAGAIN && counted == 6);
    for (int i = 0; i < n; i++) kill(kids[i], SIGKILL);
    for (int i = 0; i < n; i++) waitpid(kids[i], NULL, 0);
    pid_t k = fork();
    if (k == 0) _exit(0);
    check("Grenze pids.max = 6: nach dem Ende der Kinder geht fork wieder", k > 0 && waitpid(k, NULL, 0) == k);
    return fails ? 1 : 0;
}

static void pids_limit(void) {
    check("Gruppe: anlegen, pids.max = 6", mkdir(G_PIDS, 0755) == 0 && put_text(G_PIDS "/pids.max", "6") == 0 && num_at(G_PIDS "/pids.max") == 6);
    pid_t c = fork();
    if (c == 0) _exit(fork_until_refused());
    int st = -1;
    check("Grenze pids.max: Prozess lief durch", waitpid(c, &st, 0) == c && WIFEXITED(st) && WEXITSTATUS(st) < 40);
    if (WIFEXITED(st) && WEXITSTATUS(st)) fails++;
    check("Grenze pids.max: pids.events zählt die Ablehnung, pids.peak = 6", keyed_num(G_PIDS "/pids.events", "max") >= 1 && num_at(G_PIDS "/pids.peak") == 6);
    check("Abrechnung: pids.current = 0, memory.current = 0", num_at(G_PIDS "/pids.current") == 0 && num_at(G_PIDS "/memory.current") == 0);
}

/* child without the right MANAGE_GROUPS (all others kept) */
static int without_right(void) {
    prctl(PR_ULTAR_RIGHTS_DROP, R_MANAGE_GROUPS, 0, 0, 0);
    check("Recht: MANAGE_GROUPS abgegeben, die übrigen behalten", (unsigned long)prctl(PR_ULTAR_RIGHTS_GET, 0, 0, 0, 0) == (R_ALL & ~R_MANAGE_GROUPS));
    check("Ohne Recht: Gruppenbaum bleibt lesbar", num_at(G_PIDS "/pids.max") == 6);
    denied("Ohne Recht: memory.max zum Schreiben öffnen", open(G_PIDS "/memory.max", O_WRONLY), EACCES);
    denied("Ohne Recht: pids.max zum Schreiben öffnen", open(G_PIDS "/pids.max", O_RDWR), EACCES);
    denied("Ohne Recht: sich selbst in eine andere Gruppe schreiben", open(CG "/system/cgroup.procs", O_WRONLY), EACCES);
    denied("Ohne Recht: Gruppe anlegen", mkdir(CG "/apps/grenz-fremd", 0755), EPERM);
    denied("Ohne Recht: Gruppe entfernen", rmdir(G_PIDS), EPERM);
    denied("Ohne Recht: grösste Rohrgrösse ändern", open("/proc/sys/fs/pipe-max-size", O_WRONLY), EACCES);
    int p[2];
    check("Ohne Recht: Rohr bis zur Höchstgrösse", pipe(p) == 0 && fcntl(p[1], F_SETPIPE_SZ, 1 << 20) == 1 << 20);
    denied("Ohne Recht: Rohr über die Höchstgrösse", fcntl(p[1], F_SETPIPE_SZ, 2 << 20), EPERM);
    check("Ohne Recht: Linux-Weg – CAP_SYS_RESOURCE ist nicht mehr in der Menge", prctl(PR_CAPBSET_READ, 24, 0, 0, 0) == 0);
    pid_t k = fork();
    if (k == 0) _exit(open(G_PIDS "/pids.max", O_WRONLY) == -1 && errno == EACCES ? 0 : 1);
    int st = -1;
    check("Ohne Recht: Kind erbt das Fehlen", waitpid(k, &st, 0) == k && st == 0);
    return fails ? 1 : 0;
}

static void tree_rules(void) {
    pid_t held = fork();
    if (held == 0) {
        if (put_text(G_PIDS "/cgroup.procs", "0") != 0) _exit(40);
        eat(4);
        for (;;) pause();
    }
    long end = now_ms() + 20000;
    while (num_at(G_PIDS "/memory.current") < 4 * MB && now_ms() < end) nap_ms(10);
    check("Baum: cgroup.procs nennt den Prozess, cgroup.events populated 1", num_at(G_PIDS "/cgroup.procs") == held && keyed_num(G_PIDS "/cgroup.events", "populated") == 1);
    denied("Baum: Gruppe mit Prozess entfernen", rmdir(G_PIDS), EBUSY);
    check("Baum: memory.max unter den Verbrauch -> EBUSY", put_text(G_PIDS "/memory.max", "1048576") == -EBUSY && num_at(G_PIDS "/memory.max") == -2);
    check("Baum: Unsinn in memory.max / pids.max -> EINVAL", put_text(G_PIDS "/memory.max", "viel") == -EINVAL && put_text(G_PIDS "/pids.max", "-3") == -EINVAL);
    check("Baum: memory.current ist nicht schreibbar -> EACCES", put_text(G_PIDS "/memory.current", "0") == -EACCES);
    check("Baum: kein Prozess direkt in `cells`, `apps` oder der Wurzel -> EBUSY", put_text(CG "/cells/cgroup.procs", "0") == -EBUSY && put_text(CG "/apps/cgroup.procs", "0") == -EBUSY && put_text(CG "/cgroup.procs", "0") == -EBUSY);
    check("Baum: unbekannter Prozess -> ESRCH", put_text(G_PIDS "/cgroup.procs", "999999") == -ESRCH);
    denied("Baum: Gruppe doppelt anlegen", mkdir(G_PIDS, 0755), EEXIST);
    denied("Baum: Name mit Leerzeichen", mkdir(CG "/apps/zwei worte", 0755), EINVAL);
    denied("Baum: feste Äste lassen sich nicht entfernen", rmdir(CG "/system"), EBUSY);
    denied("Baum: Datei anlegen", open(G_PIDS "/neu", O_WRONLY | O_CREAT, 0644), EPERM);
    denied("Baum: Datei löschen", unlink(G_PIDS "/pids.max"), EPERM);
    check("Baum: Untergruppe anlegen; Elterngruppe mit Untergruppe -> EBUSY beim Entfernen", mkdir(G_PIDS "/unter", 0755) == 0 && kill(held, SIGKILL) == 0 && waitpid(held, NULL, 0) == held && rmdir(G_PIDS) == -1 && errno == EBUSY);
    pid_t c = fork();
    if (c == 0) _exit(without_right());
    int st = -1;
    check("Recht: Prozess ohne MANAGE_GROUPS lief durch", waitpid(c, &st, 0) == c && WIFEXITED(st));
    if (WIFEXITED(st) && WEXITSTATUS(st)) fails++;
    check("Baum: Untergruppe, dann Gruppe entfernen", rmdir(G_PIDS "/unter") == 0 && rmdir(G_PIDS) == 0 && access(G_PIDS, F_OK) == -1);
}

#define G_KILL CG "/apps/grenz-ende"

/* child: join `group`, then wait to be ended */
static void sit_in(const char *group) {
    if (put_text(pf("%s/cgroup.procs", group), "0") != 0) _exit(40);
    for (;;) pause();
}

/* review R2-3 / S-9: `cgroup.kill` ends the group and its subgroups at once, by group, not by number */
static void group_kill(void) {
    check("Ende: Gruppe und Untergruppe angelegt", mkdir(G_KILL, 0755) == 0 && mkdir(G_KILL "/unter", 0755) == 0);
    pid_t a = fork();
    if (a == 0) sit_in(G_KILL);
    pid_t b = fork();
    if (b == 0) sit_in(G_KILL);
    pid_t c = fork();
    if (c == 0) sit_in(G_KILL "/unter");
    pid_t bystander = fork();
    if (bystander == 0) for (;;) pause();
    long end = now_ms() + 10000;
    while ((num_at(G_KILL "/pids.current") < 2 || num_at(G_KILL "/unter/pids.current") < 1) && now_ms() < end) nap_ms(10);
    check("Ende: cgroup.kill steht im Verzeichnis", access(G_KILL "/cgroup.kill", W_OK) == 0);
    check("Ende: Unsinn -> EINVAL, Wurzel und feste Äste -> EINVAL", put_text(G_KILL "/cgroup.kill", "2") == -EINVAL && put_text(CG "/cgroup.kill", "1") == -EINVAL && put_text(CG "/apps/cgroup.kill", "1") == -EINVAL && put_text(CG "/session/cgroup.kill", "1") == -EINVAL);
    check("Ende: cgroup.kill schreiben", put_text(G_KILL "/cgroup.kill", "1") == 0);
    int sa = 0, sb = 0, sc = 0;
    check("Ende: alle drei durch SIGKILL beendet (auch in der Untergruppe)", wait_ms(a, &sa, 5000) && wait_ms(b, &sb, 5000) && wait_ms(c, &sc, 5000) && WIFSIGNALED(sa) && WTERMSIG(sa) == SIGKILL && WIFSIGNALED(sb) && WIFSIGNALED(sc) && WTERMSIG(sc) == SIGKILL);
    check("Ende: Gruppe leer, Unbeteiligter und Schreiber leben", num_at(G_KILL "/pids.current") == 0 && keyed_num(G_KILL "/cgroup.events", "populated") == 0 && kill(bystander, 0) == 0);
    kill(bystander, SIGKILL);
    waitpid(bystander, NULL, 0);
    check("Ende: Gruppen entfernt", rmdir(G_KILL "/unter") == 0 && rmdir(G_KILL) == 0);
}

/* cell: ordinary activity (files, links, pipes, processes, memory), then tell the host and wait for it */
static int cell_ready;
static int busy_cell(long go_fd) {
    char b;
    if (!enter_root(NULL)) return 9;
    int ok = 1;
    for (int i = 0; i < 24 && ok; i++) {
        int fd = open(pf("/tmp/f%d", i), O_WRONLY | O_CREAT, 0644);
        ok = fd >= 0 && write(fd, block, 65536) == 65536 && close(fd) == 0 && symlink(pf("f%d", i), pf("/tmp/l%d", i)) == 0;
    }
    mkdir("/tmp/ordner", 0755);
    int p[2];
    ok = ok && pipe(p) == 0 && write(p[1], block, 30000) == 30000;
    for (int i = 0; i < 12 && ok; i++) {
        pid_t k = fork();
        if (k == 0) _exit(eat(1) ? 0 : 1);
        int st = -1;
        ok = k > 0 && waitpid(k, &st, 0) == k && st == 0;
    }
    ok = ok && eat(6) != NULL;
    if (write(cell_ready, "r", 1) != 1 || read((int)go_fd, &b, 1) < 0) return 8;
    return ok ? 0 : 1;
}

static void cell_group(void) {
    long base = group_mem("/cells"), groups_before = num_at(CG "/cells/pids.current");
    int go[2], ready[2];
    char r;
    if (pipe(go) || pipe(ready)) return;
    cell_ready = ready[1];
    pid_t c = cell(busy_cell, go[0]);
    char dir[96];
    snprintf(dir, sizeof dir, CG "/cells/cell-%d", (int)c);
    close(ready[1]);
    if (read(ready[0], &r, 1) != 1) fails++;
    close(ready[0]);
    long mem = num_at(pf("%s/memory.current", dir));
    say("       (Gruppe der Zelle: %s, %ld KiB, %ld Prozess(e))\n", dir + strlen(CG), mem >> 10, num_at(pf("%s/pids.current", dir)));
    check("Zelle: eigene Gruppe unter `cells` entsteht von selbst und nennt den Prozess", num_at(pf("%s/cgroup.procs", dir)) == c && num_at(pf("%s/pids.current", dir)) == 1);
    check("Zelle: ihr Speicher (6 MiB + Dateien + Rohr) ist in ihrer Gruppe und in `cells` gezählt", mem >= 7 * MB + 24 * 65536 && group_mem("/cells") >= base + mem);
    check("Zelle: wirksame Prozessgrenze = ein Achtel der Maschine", num_at(pf("%s/ultar.pids.effective", dir)) == num_at(CG "/ultar.pids.effective") / 8 && num_at(CG "/cells/ultar.pids.effective") == num_at(CG "/ultar.pids.effective") / 2);
    check("Zelle: Grenze von aussen setzen (memory.max der Zellengruppe)", put_text(pf("%s/memory.max", dir), "64M") == 0);
    int st = -1;
    check("Zelle: endet normal", write(go[1], "g", 1) == 1 && waitpid(c, &st, 0) == c && WIFEXITED(st) && WEXITSTATUS(st) == 0);
    close(go[0]);
    close(go[1]);
    check("Abrechnung: Gruppe der Zelle ist verschwunden", access(dir, F_OK) == -1 && num_at(CG "/cells/pids.current") == groups_before);
    check("Abrechnung: `cells` wieder genau beim alten Stand, keine entfernte Gruppe mit Rest", settled(base));
}

void limit_tests(void) {
    say("== Ausdrückliche Grenzen, Gruppenbaum, Rechte\n");
    check("Baum: vier feste Äste, Steuerdateien wie cgroup2", access(CG "/system/memory.max", F_OK) == 0 && access(CG "/session/pids.max", F_OK) == 0 && access(CG "/apps/cgroup.procs", F_OK) == 0 && access(CG "/cells/memory.current", F_OK) == 0);
    check("Baum: dieser Prozess gehört zu `session`", keyed_num(CG "/session/cgroup.events", "populated") == 1 && num_at(CG "/session/memory.current") > 0);
    memory_limit();
    memory_limit_calls();
    pids_limit();
    tree_rules();
    group_kill();
    cell_group();
}
