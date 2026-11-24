/* Installer mode of the Ultar kernel: whole-disk nodes for the graphical installer (rawpart.rs).
 *   installtest normal               no /dev/ultar/disk, no /dev/ultar/medium, /proc/cmdline without a profile
 *   installtest installer <targets>  /dev/ultar/medium is the boot disk (GPT with ULTAR-INSTALL) and never a
 *                                    target; exactly <targets> target disks; only the raw-device right together
 *                                    with the installer's user or group (840) opens them; the medium is read only
 * Verdict line: "INSTALLTEST: ALLES OK". */
#define _GNU_SOURCE
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <grp.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/prctl.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <unistd.h>

#define PR_ULTAR_RIGHTS_DROP 0x554c5201
#define R_RAW_DEVICE 0x20UL
#define INSTALLER 840

static int fails;

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

static void denied(const char *name, long r, int want) {
    int e = errno, ok = r == -1 && e == want;
    if (!ok) fails++;
    say("%s %s -> %s\n", ok ? "ok    " : "FEHLER", name, r == -1 ? strerror(e) : "angenommen");
}

static char targets[8][300];
static int ntargets;

static void list_targets(void) {
    DIR *d = opendir("/dev/ultar/disk");
    struct dirent *e;
    while (d && (e = readdir(d))) {
        if (e->d_name[0] == '.' || ntargets == 8) continue;
        snprintf(targets[ntargets++], sizeof targets[0], "/dev/ultar/disk/%s", e->d_name);
    }
    if (d) closedir(d);
}

/* GPT disk GUID (16 bytes) and whether a partition is named ULTAR-INSTALL */
static int gpt(int fd, unsigned char guid[16], int *medium) {
    unsigned char h[512], ent[128];
    *medium = 0;
    if (pread(fd, h, 512, 512) != 512 || memcmp(h, "EFI PART", 8) != 0) return 0;
    memcpy(guid, h + 56, 16);
    unsigned long long lba = 0;
    memcpy(&lba, h + 72, 8);
    unsigned n = 0, size = 0;
    memcpy(&n, h + 80, 4);
    memcpy(&size, h + 84, 4);
    for (unsigned i = 0; i < n && i < 128 && size == 128; i++) {
        if (pread(fd, ent, 128, lba * 512 + i * 128) != 128) return 0;
        char name[37] = {0};
        for (int k = 0; k < 36; k++) name[k] = (char)ent[56 + 2 * k];
        if (strcmp(name, "ULTAR-INSTALL") == 0) *medium = 1;
    }
    return 1;
}

/* the helper runs as uid with the supplementary group extra (0: none) and only the rights keep */
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
    if (!WIFEXITED(st) || WEXITSTATUS(st) == 99) check("helper with its own identity ran", 0);
    else if (WEXITSTATUS(st)) fails++;
}

static int stranger(void) {
    denied("user 1000 with the raw-device right, without the installer group: target", open(targets[0], O_RDONLY), EACCES);
    denied("user 1000: medium", open("/dev/ultar/medium", O_RDONLY), EACCES);
    return fails;
}

static int group_without_right(void) {
    denied("installer group without the raw-device right: target", open(targets[0], O_RDWR), EACCES);
    return fails;
}

static int installer_user(void) {
    int fd = open(targets[0], O_RDONLY);
    check("installer user (840) with the raw-device right: target opens", fd >= 0);
    if (fd >= 0) close(fd);
    return fails;
}

static int installer_group(void) {
    int t = open(targets[0], O_RDWR);
    check("user 1000 in group 840 with the raw-device right: target opens for writing", t >= 0);
    off_t size = t >= 0 ? lseek(t, 0, SEEK_END) : -1;
    check("lseek(SEEK_END) gives the size of the whole disk", size >= (off_t)(64 << 20));
    char pat[4096], back[4096];
    for (int i = 0; i < (int)sizeof pat; i++) pat[i] = (char)(i * 7 + 3);
    /* not sector aligned on purpose: the kernel reads, modifies and writes the sectors around it */
    off_t at = size - 3 * 4096 - 100;
    check("pwrite at the end of the target (unaligned)", t >= 0 && pwrite(t, pat, sizeof pat, at) == (ssize_t)sizeof pat);
    check("fsync on the target (device flush)", t >= 0 && fsync(t) == 0);
    check("pread gives the written bytes back", t >= 0 && pread(t, back, sizeof back, at) == (ssize_t)sizeof back && memcmp(pat, back, sizeof pat) == 0);
    if (t >= 0) close(t);
    denied("medium for writing", open("/dev/ultar/medium", O_RDWR), EROFS);
    int m = open("/dev/ultar/medium", O_RDONLY);
    unsigned char g[16];
    int is_medium = 0;
    check("medium opens for reading: GPT with the partition ULTAR-INSTALL (the boot disk)", m >= 0 && gpt(m, g, &is_medium) && is_medium);
    if (m >= 0) close(m);
    return fails;
}

static void installer(int want) {
    struct stat st;
    list_targets();
    check("installer mode: /dev/ultar/medium exists", stat("/dev/ultar/medium", &st) == 0);
    say("--     targets: %d (%s)\n", ntargets, ntargets ? targets[0] : "-");
    check("installer mode: the expected number of target disks", ntargets == want);
    if (ntargets == 0) return;
    check("stat: block device root:840 0660", stat(targets[0], &st) == 0 && S_ISBLK(st.st_mode) && (st.st_mode & 07777) == 0660 && st.st_uid == 0 && st.st_gid == INSTALLER);
    /* the boot disk is never a target: no target has the medium's GPT */
    unsigned char mg[16] = {0}, tg[16];
    int medium_ok = 0, is_medium = 0;
    gid_t g[1] = {INSTALLER};
    if (setgroups(1, g) == 0) {
        int m = open("/dev/ultar/medium", O_RDONLY);
        medium_ok = m >= 0 && gpt(m, mg, &is_medium) && is_medium;
        if (m >= 0) close(m);
        int none = 1;
        for (int i = 0; i < ntargets; i++) {
            int t = open(targets[i], O_RDONLY);
            int other = 0;
            if (t < 0 || (gpt(t, tg, &other) && (other || memcmp(tg, mg, 16) == 0))) none = 0;
            if (t >= 0) close(t);
        }
        check("the boot disk (medium) is not among the targets", medium_ok && none);
        setgroups(0, g);
    } else {
        check("setgroups for the check", 0);
    }
    denied("root with every right but without the installer group: target", open(targets[0], O_RDONLY), EACCES);
    as(1000, 0, R_RAW_DEVICE, stranger);
    as(1000, INSTALLER, 0, group_without_right);
    as(INSTALLER, 0, R_RAW_DEVICE, installer_user);
    as(1000, INSTALLER, R_RAW_DEVICE, installer_group);
}

static void normal(void) {
    struct stat st;
    denied("normal start: no /dev/ultar/disk", stat("/dev/ultar/disk", &st), ENOENT);
    denied("normal start: no /dev/ultar/medium", stat("/dev/ultar/medium", &st), ENOENT);
    denied("normal start: no target node by name", open("/dev/ultar/disk/sata1", O_RDONLY), ENOENT);
    char b[256] = {0};
    int fd = open("/proc/cmdline", O_RDONLY);
    long n = fd >= 0 ? read(fd, b, sizeof b - 1) : -1;
    if (fd >= 0) close(fd);
    check("normal start: /proc/cmdline names no installer profile", n >= 0 && !strstr(b, "ultar.profile=installer"));
}

int main(int argc, char **argv) {
    say("INSTALLTEST: whole-disk nodes of the installer mode\n");
    if (argc >= 2 && strcmp(argv[1], "normal") == 0) normal();
    else if (argc >= 3 && strcmp(argv[1], "installer") == 0) installer(atoi(argv[2]));
    else check("usage: installtest normal | installer <targets>", 0);
    say("INSTALLTEST: %d Fehler\n", fails);
    say("INSTALLTEST: %s\n", fails == 0 ? "ALLES OK" : "SIEHE OBEN");
    return fails != 0;
}
