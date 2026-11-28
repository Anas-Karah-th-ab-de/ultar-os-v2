/* crypttest: the encrypted data partition (kernel crypt.rs). Before the key: /sys/kernel/ultar/crypt
 * reads "locked", /data is not mounted, /dev/ultar/part/ultar-data shows the LUKS2 header and refuses
 * writing, a malformed or wrong key is refused. With the key (argv[1], 128 hex digits, test only):
 * "open", /data/probe.txt readable, a new file written with fsync; a second open is refused.
 * tools/kern-test/crypt-data.sh checks on the host that the new file is on disk encrypted. */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

static int fails;

static void check(const char *name, int ok) {
    printf("%s %s\n", ok ? "ok    " : "FEHLER", name);
    if (!ok) fails++;
}

static int state_is(const char *want) {
    char b[32] = {0};
    int fd = open("/sys/kernel/ultar/crypt", O_RDONLY);
    if (fd < 0) return 0;
    ssize_t n = read(fd, b, sizeof b - 1);
    close(fd);
    return n > 0 && strncmp(b, want, strlen(want)) == 0;
}

/* writes cmd to the control file: 0 or -errno */
static int control(const char *cmd) {
    int fd = open("/sys/kernel/ultar/crypt", O_WRONLY);
    if (fd < 0) return -errno;
    ssize_t n = write(fd, cmd, strlen(cmd));
    int e = n < 0 ? -errno : 0;
    close(fd);
    return e;
}

int main(int argc, char **argv) {
    if (argc < 2 || strlen(argv[1]) != 128) {
        printf("usage: crypttest <128 hex digits>\n");
        return 2;
    }
    check("locked before the key", state_is("locked"));
    check("/data not mounted while locked", access("/data/probe.txt", F_OK) != 0);
    int raw = open("/dev/ultar/part/ultar-data", O_RDONLY);
    char head[8] = {0};
    check("header node readable", raw >= 0 && pread(raw, head, 8, 0) == 8 && memcmp(head, "LUKS\xba\xbe\x00\x02", 8) == 0);
    if (raw >= 0) close(raw);
    check("header node not writable", open("/dev/ultar/part/ultar-data", O_RDWR) < 0);
    check("malformed command refused", control("open 16777216 abc") == -EINVAL);
    char wrong[200];
    snprintf(wrong, sizeof wrong, "open 16777216 %s", argv[1]);
    wrong[14] = wrong[14] == '0' ? '1' : '0';
    check("wrong key refused (no ext4 behind it)", control(wrong) == -129);
    check("still locked after the wrong key", state_is("locked"));
    char cmd[200];
    snprintf(cmd, sizeof cmd, "open 16777216 %s", argv[1]);
    check("right key accepted", control(cmd) == 0);
    check("open after the key", state_is("open"));
    check("second open refused", control(cmd) == -EBUSY);
    char probe[64] = {0};
    int fd = open("/data/probe.txt", O_RDONLY);
    check("probe file readable", fd >= 0 && read(fd, probe, sizeof probe - 1) > 0 && strstr(probe, "ultar luks") != NULL);
    if (fd >= 0) close(fd);
    fd = open("/data/neu.txt", O_WRONLY | O_CREAT | O_TRUNC, 0644);
    const char *text = "geschrieben im Ultar-Kern verschluesselt\n";
    check("new file written", fd >= 0 && write(fd, text, strlen(text)) == (ssize_t)strlen(text) && fsync(fd) == 0);
    if (fd >= 0) close(fd);
    check("header node gone after open", access("/dev/ultar/part/ultar-data", F_OK) != 0);
    printf(fails ? "CRYPTTEST: %d FEHLER\n" : "CRYPTTEST: ALLES OK\n", fails);
    return fails != 0;
}
