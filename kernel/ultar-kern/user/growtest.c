/* growtest: the kernel's ext4 writer on a file system that ultar-ext4grow grew (new groups with
 * BLOCK_UNINIT / INODE_UNINIT): more files than the original groups have inodes and a file larger
 * than the original file system, each with fsync. tools/kern-test/ext4-grow.sh checks with e2fsck. */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static int fails;

static void check(const char *name, int ok) {
    printf("%s %s\n", ok ? "ok    " : "FEHLER", name);
    if (!ok) fails++;
}

static int write_file(const char *path, const char *data, size_t len) {
    int fd = open(path, O_WRONLY | O_CREAT | O_TRUNC, 0644);
    if (fd < 0) return 0;
    size_t done = 0;
    while (done < len) {
        ssize_t n = write(fd, data + done, len - done);
        if (n <= 0) break;
        done += (size_t)n;
    }
    int synced = fsync(fd) == 0;
    if (!synced || done != len) printf("       %s: %zu of %zu bytes, fsync %s\n", path, done, len, synced ? "ok" : strerror(errno));
    int closed = close(fd) == 0;
    return done == len && synced && closed;
}

int main(int argc, char **argv) {
    const char *dir = argc > 1 ? argv[1] : "/data";
    char path[256];
    int made = 0;
    for (int i = 0; i < 400; i++) {
        snprintf(path, sizeof path, "%s/n%03d", dir, i);
        char text[32];
        int n = snprintf(text, sizeof text, "datei %d\n", i);
        made += write_file(path, text, (size_t)n);
    }
    check("400 files (inodes from groups the growth added)", made == 400);
    size_t big = 60u << 20; /* the kernel takes files up to 64 MiB in one piece */
    char *buf = malloc(big);
    check("memory for the large file", buf != NULL);
    if (buf) {
        for (size_t i = 0; i < big; i++) buf[i] = (char)(i * 7 + 3);
        snprintf(path, sizeof path, "%s/gross", dir);
        check("60 MiB file (blocks from groups the growth added)", write_file(path, buf, big));
        free(buf);
    }
    printf(fails ? "GROWTEST: %d FEHLER\n" : "GROWTEST: ALLES OK\n", fails);
    return fails != 0;
}
