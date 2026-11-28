// What programs of other systems need from the Ultar kernel (docs/integration/ultar-foreign.md, kernel
// gaps found with dpkg/apt): execve resets caught signals; rmdir/unlink each remove only their own kind;
// a directory named with a trailing slash lists its entries; close_range, sync_file_range, msync, fchdir;
// pseudo-terminals (/dev/ptmx, /dev/pts/N: line discipline, raw mode, TIOCGPTPEER, owner check).
// Verdict line: "FREMDTEST: ALLES OK".
#define _GNU_SOURCE
#include <sys/prctl.h>
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/syscall.h>
#include <sys/wait.h>
#include <termios.h>
#include <unistd.h>

static int failures;

static void check(int ok, const char *what) {
    printf("%s %s\n", ok ? "ok    " : "FEHLER", what);
    if (!ok) failures++;
}

static void on_usr1(int s) { (void)s; }

// child mode: raise a signal after exec and report survival
static int child_raise(int sig) {
    raise(sig);
    return 0;
}

static int run_child(const char *self, const char *mode) {
    pid_t pid = fork();
    if (pid == 0) {
        execl(self, self, mode, (char *)NULL);
        _exit(99);
    }
    int st = 0;
    waitpid(pid, &st, 0);
    return st;
}

static void signals_after_exec(const char *self) {
    signal(SIGUSR1, on_usr1);
    int st = run_child(self, "--raise-usr1");
    check(WIFSIGNALED(st) && WTERMSIG(st) == SIGUSR1, "exec: a caught signal acts by default afterwards");
    signal(SIGUSR2, SIG_IGN);
    st = run_child(self, "--raise-usr2");
    check(WIFEXITED(st) && WEXITSTATUS(st) == 0, "exec: an ignored signal stays ignored");
    signal(SIGUSR1, SIG_DFL);
    signal(SIGUSR2, SIG_DFL);
}

static void remove_kinds(const char *base) {
    char dir[256], file[256];
    snprintf(dir, sizeof dir, "%s/ft-dir", base);
    snprintf(file, sizeof file, "%s/ft-file", base);
    rmdir(dir);
    unlink(file);
    check(mkdir(dir, 0755) == 0, "mkdir");
    int fd = open(file, O_CREAT | O_WRONLY | O_TRUNC, 0644);
    check(fd >= 0 && write(fd, "x", 1) == 1 && close(fd) == 0, "file created");
    errno = 0;
    check(rmdir(file) == -1 && errno == ENOTDIR, "rmdir of a file: ENOTDIR (dpkg's test)");
    check(access(file, F_OK) == 0, "the file is still there");
    errno = 0;
    check(unlinkat(AT_FDCWD, file, AT_REMOVEDIR) == -1 && errno == ENOTDIR, "unlinkat(AT_REMOVEDIR) of a file: ENOTDIR");
    errno = 0;
    check(unlink(dir) == -1 && errno == EISDIR, "unlink of a directory: EISDIR");
    // the trailing slash: the same listing
    char sub[300];
    snprintf(sub, sizeof sub, "%s/inner", dir);
    fd = open(sub, O_CREAT | O_WRONLY, 0644);
    close(fd);
    char slash[300];
    snprintf(slash, sizeof slash, "%s/", dir);
    int seen = 0;
    DIR *d = opendir(slash);
    for (struct dirent *e; d && (e = readdir(d));)
        seen += strcmp(e->d_name, "inner") == 0;
    if (d) closedir(d);
    check(seen == 1, "a directory named with a trailing slash lists its entries");
    unlink(sub);
    check(unlink(file) == 0, "unlink of the file");
    check(rmdir(dir) == 0, "rmdir of the empty directory");
}

static void descriptors(void) {
    int a = open("/dev/null", O_RDONLY), b = dup(a), c = dup(a);
    check(syscall(436, b, ~0U, 4) == 0 && (fcntl(b, F_GETFD) & FD_CLOEXEC) && (fcntl(c, F_GETFD) & FD_CLOEXEC),
          "close_range(CLOEXEC) marks the range");
    check(syscall(436, b, ~0U, 0) == 0 && fcntl(b, F_GETFD) == -1 && fcntl(c, F_GETFD) == -1 && fcntl(a, F_GETFD) >= 0,
          "close_range closes the range, not below it");
    errno = 0;
    check(syscall(436, 5, 3, 0) == -1 && errno == EINVAL, "close_range(first > last): EINVAL");
    close(a);
    int f = open("/tmp/ft-sync", O_CREAT | O_RDWR | O_TRUNC, 0644);
    check(f >= 0 && sync_file_range(f, 0, 0, SYNC_FILE_RANGE_WRITE) == 0, "sync_file_range on a file");
    int p[2];
    check(pipe(p) == 0, "pipe");
    errno = 0;
    check(sync_file_range(p[0], 0, 0, 0) == -1 && errno == ESPIPE, "sync_file_range on a pipe: ESPIPE");
    close(p[0]);
    close(p[1]);
    close(f);
    unlink("/tmp/ft-sync");
    char *m = mmap(NULL, 8192, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    check(m != MAP_FAILED && msync(m, 8192, MS_SYNC) == 0, "msync");
    errno = 0;
    check(msync(m + 1, 10, MS_SYNC) == -1 && errno == EINVAL, "msync of an unaligned start: EINVAL");
    munmap(m, 8192);
    mkdir("/tmp/ft-cwd", 0755);
    int dfd = open("/tmp/ft-cwd", O_RDONLY | O_DIRECTORY);
    char cwd[256] = "";
    check(dfd >= 0 && fchdir(dfd) == 0 && getcwd(cwd, sizeof cwd) && strcmp(cwd, "/tmp/ft-cwd") == 0, "fchdir");
    errno = 0;
    int nd = open("/dev/null", O_RDONLY);
    check(fchdir(nd) == -1 && errno == ENOTDIR, "fchdir of a file: ENOTDIR");
    close(nd);
    close(dfd);
    if (chdir("/") != 0) failures++;
    rmdir("/tmp/ft-cwd");
}

static ssize_t read_some(int fd, char *buf, size_t n) {
    for (int i = 0; i < 50; i++) {
        ssize_t r = read(fd, buf, n);
        if (r > 0 || (r < 0 && errno != EAGAIN)) return r;
        usleep(10000);
    }
    return -1;
}

static void terminals(void) {
    int m = posix_openpt(O_RDWR | O_NOCTTY);
    check(m >= 0, "posix_openpt (/dev/ptmx)");
    if (m < 0) return;
    check(grantpt(m) == 0 && unlockpt(m) == 0, "grantpt, unlockpt");
    char *name = ptsname(m);
    check(name && strncmp(name, "/dev/pts/", 9) == 0, "ptsname gives /dev/pts/N");
    if (!name) return;
    int s = open(name, O_RDWR | O_NOCTTY);
    check(s >= 0 && isatty(s), "the slave opens and is a terminal");
    struct stat st;
    check(fstat(s, &st) == 0 && S_ISCHR(st.st_mode), "the slave is a character device");
    fcntl(m, F_SETFL, O_NONBLOCK);
    fcntl(s, F_SETFL, O_NONBLOCK);
    char buf[64] = "";
    check(write(m, "hal", 3) == 3 && read(s, buf, sizeof buf) == -1 && errno == EAGAIN, "canonical: no line before its end");
    check(write(m, "lo\r", 3) == 3 && read_some(s, buf, sizeof buf) == 6 && memcmp(buf, "hallo\n", 6) == 0, "canonical: the line with ICRNL");
    memset(buf, 0, sizeof buf);
    check(read_some(m, buf, sizeof buf) == 7 && memcmp(buf, "hallo\r\n", 7) == 0, "echo to the master");
    check(write(s, "out\n", 4) == 4 && read_some(m, buf, sizeof buf) == 5 && memcmp(buf, "out\r\n", 5) == 0, "output: ONLCR");
    struct termios t;
    check(tcgetattr(s, &t) == 0, "tcgetattr");
    cfmakeraw(&t);
    check(tcsetattr(s, TCSANOW, &t) == 0, "raw mode");
    check(write(m, "x", 1) == 1 && read_some(s, buf, sizeof buf) == 1 && buf[0] == 'x', "raw: a byte at once, no echo");
    errno = 0;
    check(read(m, buf, sizeof buf) == -1 && errno == EAGAIN, "raw: nothing echoed");
    int peer = ioctl(m, 0x5441, O_RDWR | O_NOCTTY);
    check(peer >= 0 && isatty(peer), "TIOCGPTPEER");
    char num[16];
    snprintf(num, sizeof num, "%s", name + 9);
    int listed = 0;
    DIR *d = opendir("/dev/pts");
    for (struct dirent *e; d && (e = readdir(d));)
        listed += strcmp(e->d_name, num) == 0;
    if (d) closedir(d);
    check(listed == 1, "/dev/pts lists the terminal");
    pid_t pid = fork();
    if (pid == 0) {
        if (setuid(4321) != 0) _exit(2);
        prctl(0x554c5201, ~0UL, 0, 0, 0); // PR_ULTAR_RIGHTS_DROP: all rights (FILE_ADMIN would open it)
        int o = open(name, O_RDWR | O_NOCTTY);
        _exit(o < 0 && errno == EACCES ? 0 : 1);
    }
    int ws = 0;
    waitpid(pid, &ws, 0);
    check(WIFEXITED(ws) && WEXITSTATUS(ws) == 0, "another user cannot open the slave (EACCES)");
    close(peer);
    close(s);
    close(m);
}

int main(int argc, char **argv) {
    if (argc > 1 && strcmp(argv[1], "--raise-usr1") == 0) return child_raise(SIGUSR1);
    if (argc > 1 && strcmp(argv[1], "--raise-usr2") == 0) return child_raise(SIGUSR2);
    setvbuf(stdout, NULL, _IONBF, 0);
    signals_after_exec(argv[0]);
    remove_kinds("/tmp");
    if (access("/data", W_OK) == 0) remove_kinds("/data");
    descriptors();
    terminals();
    if (failures == 0)
        printf("FREMDTEST: ALLES OK\n");
    else
        printf("FREMDTEST: %d FEHLER\n", failures);
    return failures != 0;
}
