/* grenztest, part 3: a cell without rights tries to exhaust the machine (the four attacks of the
 * security review plus floods of small kernel objects), and the dynamic rules: protected minimum,
 * fair share, and the order in which the kernel ends processes when memory is critical. */
#include "grenztest.h"

static long cells_killed(void) {
    return keyed_num(CG "/cells/memory.events", "oom_kill");
}

/* after an attack: the machine works, memory is back, nothing is left charged */
static void aftermath(const char *what, long free_before, long cells_before) {
    check(pf("%s: Wirt lebt, ein normales Programm startet und endet mit Erfolg", what), normal_program_runs());
    int back = settled(cells_before);
    long now = free_kb();
    say("       (frei vorher %ld KiB, jetzt %ld KiB)\n", free_before, now);
    /* free memory is counted in whole pages: the kernel heap keeps up to a few of its 2-MiB blocks for
     * reuse. The exact test for a leak is the accounting (`settled`: zero bytes left charged). */
    check(pf("%s: Speicher ist wieder frei, `cells` beim alten Stand, kein Rest", what), back && now >= free_before - 8192);
}

/* end every cell that exists right now: the group tree names them (`cells/cell-<pid>`) */
static int end_all_cells(void) {
    DIR *d = opendir(CG "/cells");
    struct dirent *e;
    int n = 0;
    while (d && (e = readdir(d))) {
        if (!strncmp(e->d_name, "cell-", 5) && kill(atoi(e->d_name + 5), SIGKILL) == 0) n++;
    }
    if (d) closedir(d);
    return n;
}

/* run `ausbruch --dos <mode>` (the reviewer's attack program); if it is still there after `ms`, the
 * cell it started is ended from outside */
static int run_attack(const char *mode, long ms, int *timed_out) {
    pid_t c = fork();
    if (c == 0) {
        execl(pf("%s/ausbruch", bin_dir), "ausbruch", "--dos", mode, (char *)NULL);
        _exit(127);
    }
    int st = -1;
    *timed_out = !wait_ms(c, &st, ms);
    if (*timed_out && (end_all_cells() != 1 || !wait_ms(c, &st, 20000))) {
        kill(c, SIGKILL);
        waitpid(c, &st, 0);
        return -1;
    }
    return st;
}

static void reviewer_attacks(void) {
    if (access(pf("%s/ausbruch", bin_dir), X_OK) != 0) {
        check("Angriffsprogramm ausbruch ist auf dem Abbild", 0);
        return;
    }
    static const struct {
        const char *mode, *name;
        long ms;
        int ends, killed;
    } a[] = {
        {"mem", "Angriff Speicher (3 GiB berühren)", 600000, 1, 1},
        {"tmpfs", "Angriff tmpfs (Datei ohne Ende)", 300000, 1, 0},
        {"pipe", "Angriff Rohr (ohne Leser füllen)", 4000, 0, 0},
        {"fork", "Angriff Prozess-Lawine", 600000, 1, 0},
    };
    for (unsigned i = 0; i < sizeof a / sizeof *a; i++) {
        long free_before = free_kb(), cells_before = group_mem("/cells"), kills = cells_killed();
        int timed_out = 0;
        long t0 = now_ms();
        int st = run_attack(a[i].mode, a[i].ms, &timed_out);
        say("       (%s: %ld ms, Status %#x, vom Kern beendet: %ld)\n", a[i].mode, now_ms() - t0, st, cells_killed() - kills);
        if (a[i].ends) {
            check(pf("%s: endet von selbst – abgewiesen oder beendet", a[i].name), !timed_out && WIFEXITED(st) && WEXITSTATUS(st) == 0);
        } else {
            long held = group_mem("/cells") - cells_before;
            say("       (während der Schreiber wartet, belegt die Zelle %ld KiB)\n", held >> 10);
            check(pf("%s: Schreiber wartet am vollen Rohr (64 KiB), bis der Wirt die Zelle beendet", a[i].name), timed_out && WIFEXITED(st) && WEXITSTATUS(st) == 0);
        }
        /* the attack touches 3 GiB: a machine with much more free memory simply grants them */
        if (a[i].killed && free_before < 4096L * 1024) check(pf("%s: der Kern hat die Zelle beendet", a[i].name), cells_killed() > kills);
        aftermath(a[i].name, free_before, cells_before);
    }
}

/* cell: fill tmpfs files until refused; reports the MiB written as exit status (max 250) */
static int many_files(long unused) {
    (void)unused;
    if (!enter_root(NULL)) return 255;
    long mb = 0;
    int e = 0;
    for (int f = 0; f < 4096 && !e; f++) {
        int fd = open(pf("/tmp/gross%d", f), O_WRONLY | O_CREAT, 0644);
        if (fd < 0) {
            e = errno;
            break;
        }
        for (int i = 0; i < 16; i++) {
            if (write(fd, block, sizeof block) != sizeof block) {
                e = errno;
                break;
            }
            mb++;
        }
        close(fd);
    }
    say("       (Zelle: %ld MiB in Dateien zu 16 MiB, dann %s)\n", mb, strerror(e));
    return e == ENOSPC || e == ENOMEM ? (mb > 250 ? 250 : (int)mb) : 254;
}

/* cell: floods of small kernel objects; each must end with an error number */
static int small_objects(long unused) {
    (void)unused;
    if (!enter_root("size=1m")) return 255;
    long n = 0, t0 = now_ms();
    while (n < 100000 && symlink("x", pf("/klein/l%ld", n)) == 0) n++;
    int e = errno;
    say("       (Zelle: %ld Verknüpfungen in einem tmpfs mit size=1m, dann %s; %ld ms)\n", n, strerror(e), now_ms() - t0);
    check("Flut Verknüpfungen: endet mit ENOSPC (Kontingent zählt jeden Eintrag)", n > 1000 && n < 100000 && e == ENOSPC);
    int p[2], pipes = 0;
    t0 = now_ms();
    while (pipes < 2000 && pipe2(p, O_NONBLOCK) == 0) {
        while (write(p[1], block, 65536) > 0) {}
        pipes++;
    }
    e = errno;
    say("       (Zelle: %d volle Rohre, dann %s; %ld ms)\n", pipes, strerror(e), now_ms() - t0);
    check("Flut Rohre: jedes nimmt 64 KiB, das Ende ist eine Fehlernummer oder die Schleifengrenze", pipes > 100 && (pipes == 2000 || e == EMFILE || e == ENOMEM || e == EAGAIN));
    /* descriptors: the table has room for 65536; start near its end (the kernel looks for the lowest
     * free number from the given one) */
    n = 0, t0 = now_ms();
    while (n < 100000 && fcntl(0, F_DUPFD, 60000) >= 0) n++;
    e = errno;
    say("       (Zelle: %ld Deskriptoren ab Nummer 60000, dann %s; %ld ms)\n", n, strerror(e), now_ms() - t0);
    check("Flut Deskriptoren: die Tabelle endet bei 65536 -> EMFILE", n == 5536 && e == EMFILE);
    denied("Flut Deskriptoren: dup2 auf Nummer 65536", dup2(0, 65536), EBADF);
    n = 0, t0 = now_ms();
    while (n < 300000 && mmap(NULL, 4096, PROT_NONE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0) != MAP_FAILED) n++;
    e = errno;
    say("       (Zelle: %ld Abbildungen, dann %s; %ld ms)\n", n, strerror(e), now_ms() - t0);
    check("Flut Abbildungen: endet mit ENOMEM bei 65530 Bereichen", n > 60000 && n <= 65530 && e == ENOMEM);
    return fails ? 1 : 0;
}

static void own_attacks(void) {
    long free_before = free_kb(), cells_before = group_mem("/cells");
    long quota = keyed_num("/proc/ultar/gruppen", "gesamt") / 4 / 1024;
    int st = -1;
    pid_t c = cell(many_files, 0);
    check("Angriff viele Dateien: Zelle endet von selbst", wait_ms(c, &st, 600000) && WIFEXITED(st));
    int mb = WIFEXITED(st) ? WEXITSTATUS(st) : -1;
    check("Angriff viele Dateien: abgewiesen (ENOSPC/ENOMEM) spätestens beim Kontingent des tmpfs (¼ des Speichers)", mb >= 16 && mb < 254 && (mb == 250 || mb <= quota));
    aftermath("Angriff viele Dateien", free_before, cells_before);
    free_before = free_kb();
    c = cell(small_objects, 0);
    st = -1;
    check("Angriff kleine Kern-Objekte: Zelle lief durch", wait_ms(c, &st, 600000) && WIFEXITED(st) && WEXITSTATUS(st) != 255);
    if (WIFEXITED(st) && WEXITSTATUS(st)) fails++;
    aftermath("Angriff kleine Kern-Objekte", free_before, cells_before);
}

void attack_tests(void) {
    say("== Angriffe einer Zelle ohne Rechte\n");
    reviewer_attacks();
    own_attacks();
}

/* ------------------------------------------------------------------ dynamic rules */

/* a process that holds `mb` MiB, says so on `fd` and waits to be ended */
static void hold(long mb, int fd) {
    if (!eat(mb) || write(fd, "r", 1) != 1) _exit(1);
    for (;;) pause();
}

static int ready_fd;
static int holding_cell(long mb) {
    hold(mb, ready_fd);
    return 0;
}

/* cell: take memory with calls that are refused with ENOMEM (the process lives) until refused; then
 * on request from the host try `again` more MiB. Reports the MiB it got and the result of the second try. */
static int from_host, to_host;
static int brk_cell(long again) {
    int fd = open(pf("%s/hallo", bin_dir), O_RDONLY);
    long mb = 0;
    while (mb < (1L << 20) && grab_mb(fd)) mb++;
    char cmd;
    if (fd < 0 || write(to_host, &mb, sizeof mb) != sizeof mb || read(from_host, &cmd, 1) != 1) return 1;
    long more = 0;
    while (more < again && grab_mb(fd)) more++;
    if (write(to_host, &more, sizeof more) != sizeof more || read(from_host, &cmd, 1) != 1) return 1;
    return 0;
}

struct brk_peer {
    pid_t pid;
    int to[2], from[2];
};

static struct brk_peer start_brk_cell(long again) {
    struct brk_peer p = {0};
    if (pipe(p.to) || pipe(p.from)) return p;
    from_host = p.to[0];
    to_host = p.from[1];
    p.pid = cell(brk_cell, again);
    return p;
}

static long ask(struct brk_peer *p, int go) {
    long v = -1;
    if (go && write(p->to[1], "g", 1) != 1) return -1;
    if (read(p->from[0], &v, sizeof v) != sizeof v) return -1;
    return v;
}

static void end_peer(struct brk_peer *p) {
    int st;
    if (write(p->to[1], "e", 1) == 1) wait_ms(p->pid, &st, 20000);
    kill(p->pid, SIGKILL);
    waitpid(p->pid, &st, 0);
    close(p->to[0]), close(p->to[1]), close(p->from[0]), close(p->from[1]);
}

/* (a) the protected minimum of `system` and `session` cannot be consumed by a cell */
static void protected_minimum(void) {
    long prot_sys = num_at(CG "/system/memory.min"), prot_sess = num_at(CG "/session/memory.min");
    long reserve = keyed_num(CG "/ultar.memory.pressure", "reserve");
    struct brk_peer cellp = start_brk_cell(0);
    long got = ask(&cellp, 0);
    long free_now = free_kb() * 1024, sess = group_mem("/session"), sys = group_mem("/system");
    long kept = reserve + (prot_sys > sys ? prot_sys - sys : 0) + (prot_sess > sess ? prot_sess - sess : 0);
    say("       (Zelle bekam %ld MiB; frei %ld KiB; Vorrat + ungenutzter Schutz = %ld KiB)\n", got, free_now >> 10, kept >> 10);
    check("Schutz: Zelle wird abgewiesen (ENOMEM), bevor sie den Schutz von system/session antastet", got > 0 && free_now >= kept - MB && free_now < kept + 4 * MB);
    long want = (prot_sess - sess) / MB - 8;
    pid_t s = fork();
    if (s == 0) _exit(want > 0 && eat(want) ? 0 : 1);
    int st = -1;
    check("Schutz: ein Sitzungsprogramm bekommt den geschützten Rest der Sitzung trotzdem", waitpid(s, &st, 0) == s && st == 0);
    end_peer(&cellp);
}

/* (b) memory short and the protected minimums in use: the larger of two cells is refused first */
static void fair_share(void) {
    long prot_sys = num_at(CG "/system/memory.min") / MB, prot_sess = num_at(CG "/session/memory.min") / MB;
    int rd[2];
    if (pipe(rd)) return;
    pid_t sys = fork(), sess = -1;
    if (sys == 0) {
        if (put_text(CG "/system/cgroup.procs", "0") != 0) _exit(1);
        hold(prot_sys + 1, rd[1]);
    }
    char r;
    int ok = read(rd[0], &r, 1) == 1;
    sess = fork();
    if (sess == 0) hold(prot_sess + 1, rd[1]);
    ok = ok && read(rd[0], &r, 1) == 1;
    check("Anteil: system und session nutzen ihren geschützten Anteil voll", ok && group_mem("/system") >= prot_sys * MB && group_mem("/session") >= prot_sess * MB);
    struct brk_peer big;
    ready_fd = rd[1];
    /* the small cell holds 8 MiB (plain pages), then the big one grows until refused */
    pid_t little = cell(holding_cell, 8);
    ok = read(rd[0], &r, 1) == 1;
    big = start_brk_cell(2);
    long got = ask(&big, 0);
    long low = keyed_num(CG "/ultar.memory.pressure", "low"), free_now = free_kb() * 1024;
    say("       (grosse Zelle bekam %ld MiB und wurde abgewiesen; frei %ld KiB, Schwelle `knapp` %ld KiB)\n", got, free_now >> 10, low >> 10);
    check("Anteil: die grösste Zelle wird an der Schwelle `knapp` abgewiesen", ok && got > 8 && free_now < low + 2 * MB && free_now > low - 4 * MB);
    /* a third, small cell still gets memory below the watermark; the big one still does not */
    pid_t third = cell(holding_cell, 3);
    check("Anteil: eine kleine Zelle bekommt unterhalb der Schwelle weiter Speicher", read(rd[0], &r, 1) == 1 && free_kb() * 1024 < low);
    check("Anteil: die grosse Zelle bleibt abgewiesen, lebt aber weiter", ask(&big, 1) == 0 && kill(big.pid, 0) == 0);
    end_peer(&big);
    pid_t all[] = {little, third, sess, sys};
    for (unsigned i = 0; i < sizeof all / sizeof *all; i++) {
        kill(all[i], SIGKILL);
        waitpid(all[i], NULL, 0);
    }
    close(rd[0]), close(rd[1]);
}

/* (c) critical: a `system` process takes memory until the kernel must end someone – first the larger
 * cell, then the smaller, then the largest session process; never a `system` process */
static void killing_order(void) {
    int rd[2];
    char r;
    if (pipe(rd)) return;
    ready_fd = rd[1];
    long session_kills = keyed_num(CG "/session/memory.events", "oom_kill");
    pid_t small = cell(holding_cell, 24), large = cell(holding_cell, 48);
    pid_t hog = fork();
    if (hog == 0) hold(16, rd[1]);
    pid_t bystander = fork();
    if (bystander == 0) {
        if (put_text(CG "/system/cgroup.procs", "0") != 0) _exit(1);
        hold(40, rd[1]);
    }
    int ok = 1;
    for (int i = 0; i < 4; i++) ok = ok && read(rd[0], &r, 1) == 1;
    check("Reihenfolge: zwei Zellen (24 und 48 MiB), ein Sitzungsprogramm (16 MiB), ein Systemdienst (40 MiB) laufen", ok);
    pid_t eater = fork();
    if (eater == 0) {
        if (put_text(CG "/system/cgroup.procs", "0") != 0) _exit(1);
        /* stops as soon as the kernel has ended a session process */
        for (long mb = 0; mb < 1 << 20; mb++) {
            if (!eat(1)) _exit(2);
            if (keyed_num(CG "/session/memory.events", "oom_kill") > session_kills) _exit(0);
        }
        _exit(3);
    }
    pid_t victims[3] = {large, small, hog}, order[3] = {0, 0, 0};
    int status[3] = {0, 0, 0}, n = 0;
    long end = now_ms() + 900000;
    while (n < 3 && now_ms() < end) {
        for (int i = 0; i < 3; i++) {
            int st;
            if (victims[i] > 0 && waitpid(victims[i], &st, WNOHANG) == victims[i]) {
                say("       (%ld ms: %s vom Kern beendet, Status %#x)\n", now_ms(), i == 0 ? "grosse Zelle" : i == 1 ? "kleine Zelle" : "Sitzungsprogramm", st);
                order[n] = victims[i];
                status[n++] = st;
                victims[i] = 0;
            }
        }
        nap_ms(5);
    }
    int st = -1;
    int eater_ok = wait_ms(eater, &st, 60000) && WIFEXITED(st) && WEXITSTATUS(st) == 0;
    check("Reihenfolge: zuerst die grössere Zelle, dann die kleinere, zuletzt das Sitzungsprogramm", n == 3 && order[0] == large && order[1] == small && order[2] == hog);
    check("Reihenfolge: alle drei durch SIGKILL beendet", n == 3 && status[0] == SIGKILL && status[1] == SIGKILL && status[2] == SIGKILL);
    check("Reihenfolge: der Systemdienst, der den Speicher nahm, wurde nicht beendet", eater_ok);
    check("Reihenfolge: der unbeteiligte Systemdienst lebt", kill(bystander, 0) == 0 && waitpid(bystander, &st, WNOHANG) == 0);
    kill(bystander, SIGKILL);
    waitpid(bystander, NULL, 0);
    if (!eater_ok) kill(eater, SIGKILL), waitpid(eater, NULL, 0);
    close(rd[0]), close(rd[1]);
}

void dynamic_tests(void) {
    say("== Dynamischer Schutz ohne gesetzte Grenzen\n");
    long free_before = free_kb(), cells_before = group_mem("/cells");
    say("       (%s", "Regeln: ");
    say("Vorrat %ld, kritisch %ld, knapp %ld KiB; Schutz system %ld, session %ld KiB)\n", keyed_num(CG "/ultar.memory.pressure", "reserve") >> 10, keyed_num(CG "/ultar.memory.pressure", "critical") >> 10,
        keyed_num(CG "/ultar.memory.pressure", "low") >> 10, num_at(CG "/system/memory.min") >> 10, num_at(CG "/session/memory.min") >> 10);
    protected_minimum();
    aftermath("Schutz", free_before, cells_before);
    fair_share();
    aftermath("Anteil", free_before, cells_before);
    killing_order();
    aftermath("Reihenfolge", free_before, cells_before);
}
