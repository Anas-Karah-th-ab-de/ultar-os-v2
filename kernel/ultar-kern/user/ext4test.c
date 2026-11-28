/* Durable whole-file writes on a writable ext4 (/data): ext4test write | verify
 *  write : fragments the free space (256 KiB files, every other one removed), then writes a 64 MiB
 *          file and an 18 MiB file, each with fsync – both must succeed and be on the medium when
 *          fsync returns (extent trees of depth 1); then a file larger than the free space: fsync
 *          and close must fail with ENOSPC (nothing kept in memory as if it were stored); statvfs
 *          shows the ext4's own counts, and the 64 MiB file can still be changed although the free
 *          space holds no second copy of it (its own blocks are reused).
 *          Prints "EXT4TEST: GESCHRIEBEN"; the machine is then stopped hard.
 *  verify: after the restart both files read back byte for byte: "EXT4TEST: ALLES OK". */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/statvfs.h>
#include <unistd.h>

#define MIB (1024L * 1024L)
static int failures;
static void check(int ok, const char *what) {
    printf("%s %s\n", ok ? "ok  " : "FEHLER", what);
    if (!ok) failures++;
}

static unsigned char pattern(long i, int seed) { return (unsigned char)((i * 2654435761u >> 13) + seed); }

static int write_file(const char *path, long size, int seed) {
    unsigned char *buf = malloc(MIB);
    int fd = open(path, O_CREAT | O_TRUNC | O_WRONLY, 0644);
    if (fd < 0 || !buf) return -1;
    for (long off = 0; off < size; off += MIB) {
        long n = size - off < MIB ? size - off : MIB;
        for (long i = 0; i < n; i++) buf[i] = pattern(off + i, seed);
        if (write(fd, buf, n) != n) return -1;
    }
    free(buf);
    int r = fsync(fd);
    int e = errno;
    int c = close(fd);
    errno = r ? e : errno;
    return r ? -1 : c;
}

static int same(const char *path, long size, int seed) {
    int fd = open(path, O_RDONLY);
    if (fd < 0) return 0;
    struct stat st;
    if (fstat(fd, &st) || st.st_size != size) return 0;
    unsigned char *buf = malloc(MIB);
    for (long off = 0; off < size;) {
        long n = read(fd, buf, MIB);
        if (n <= 0) return 0;
        for (long i = 0; i < n; i++)
            if (buf[i] != pattern(off + i, seed)) return 0;
        off += n;
    }
    free(buf);
    close(fd);
    return 1;
}

static void fragment(const char *dir) {
    char p[128];
    unsigned char *buf = calloc(1, 256 * 1024);
    for (int i = 0; i < 160; i++) {
        snprintf(p, sizeof p, "%s/frag%03d", dir, i);
        int fd = open(p, O_CREAT | O_TRUNC | O_WRONLY, 0644);
        if (fd < 0 || write(fd, buf, 256 * 1024) != 256 * 1024 || fsync(fd) || close(fd)) { check(0, "fragmenting"); return; }
    }
    for (int i = 0; i < 160; i += 2) {
        snprintf(p, sizeof p, "%s/frag%03d", dir, i);
        unlink(p);
    }
    free(buf);
    check(1, "free space fragmented (80 holes of 256 KiB)");
}

int main(int argc, char **argv) {
    setvbuf(stdout, NULL, _IONBF, 0);
    const char *dir = argc > 2 ? argv[2] : "/data";
    char big[128], bundle[128], huge[128];
    snprintf(big, sizeof big, "%s/big64", dir);
    snprintf(bundle, sizeof bundle, "%s/bundle18", dir);
    snprintf(huge, sizeof huge, "%s/too-big", dir);
    if (argc > 1 && strcmp(argv[1], "write") == 0) {
        fragment(dir);
        check(write_file(big, 64 * MIB, 1) == 0, "64 MiB file written, fsync ok");
        check(write_file(bundle, 18 * MIB, 2) == 0, "18 MiB file written, fsync ok");
        check(same(big, 64 * MIB, 1) && same(bundle, 18 * MIB, 2), "both read back before the stop");
        int r = write_file(huge, 60 * MIB, 3);
        char msg[80];
        snprintf(msg, sizeof msg, "file larger than the free space: fsync fails (%s)", r ? strerror(errno) : "succeeded");
        check(r != 0 && errno == ENOSPC, msg);
        // statvfs reports the ext4 itself (not the machine's memory); a file larger than the free
        // space can still be changed: its own blocks take the new bytes
        struct statvfs sv;
        int sr = statvfs(dir, &sv);
        unsigned long long size = (unsigned long long)sv.f_blocks * sv.f_frsize, room = (unsigned long long)sv.f_bavail * sv.f_frsize;
        check(sr == 0 && size > 100 * MIB && size <= 170 * MIB && sv.f_files > 0 && sv.f_ffree < sv.f_files,
              "statvfs: the ext4's own size and inode counts");
        check(sr == 0 && room < 64 * MIB, "statvfs: less room than the 64 MiB file");
        unsigned char *b = malloc(MIB);
        for (long i = 0; i < MIB; i++) b[i] = pattern(i, 1);
        int fd = open(big, O_RDWR);
        int done = fd >= 0 && pwrite(fd, b, MIB, 0) == MIB && fsync(fd) == 0 && close(fd) == 0;
        free(b);
        check(done, "64 MiB file changed although the free space holds no second copy of it");
        check(same(big, 64 * MIB, 1), "the changed 64 MiB file reads back");
        // a directory in an indexed (HTree) directory: made on the disk or refused – never kept in memory
        char sub[160];
        snprintf(sub, sizeof sub, "%s/bigdir/neu", dir);
        if (access(sub, F_OK) != 0 && access(dir, F_OK) == 0) {
            snprintf(bundle, sizeof bundle, "%s/bigdir", dir);
            if (access(bundle, F_OK) == 0) {
                int m = mkdir(sub, 0755);
                printf("ok   mkdir in the indexed directory: %s\n", m == 0 ? "made" : strerror(errno));
                FILE *f = fopen("/tmp/ext4test-mkdir", "w");
                if (f) { fprintf(f, "%d\n", m == 0); fclose(f); }
                char flag[192];
                snprintf(flag, sizeof flag, "%s/mkdir-result", dir);
                int fd = open(flag, O_CREAT | O_TRUNC | O_WRONLY, 0644);
                if (fd >= 0) { if (write(fd, m == 0 ? "1" : "0", 1) != 1) failures++; fsync(fd); close(fd); }
            }
        }
        printf(failures ? "EXT4TEST: FEHLER (%d)\n" : "EXT4TEST: GESCHRIEBEN\n", failures);
    } else {
        check(same(big, 64 * MIB, 1), "64 MiB file intact after the hard stop");
        check(same(bundle, 18 * MIB, 2), "18 MiB file intact after the hard stop");
        // what mkdir answered in the indexed directory must hold after the restart
        char flag[192], sub[160], r = 0;
        snprintf(flag, sizeof flag, "%s/mkdir-result", dir);
        snprintf(sub, sizeof sub, "%s/bigdir/neu", dir);
        int fd = open(flag, O_RDONLY);
        if (fd >= 0 && read(fd, &r, 1) == 1) {
            struct stat st;
            int there = stat(sub, &st) == 0 && S_ISDIR(st.st_mode);
            check(r == '1' ? there : !there, r == '1' ? "the directory made in the indexed directory is on the disk" : "the refused directory is nowhere (not kept in memory)");
        }
        if (fd >= 0) close(fd);
        printf(failures ? "EXT4TEST: FEHLER (%d)\n" : "EXT4TEST: ALLES OK\n", failures);
    }
    return failures != 0;
}
