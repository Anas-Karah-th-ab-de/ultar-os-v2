/* Process start and file locks as the system updater uses them (std::process::Command, flock):
 *  1. vfork + execve: the parent goes on as soon as the child has replaced its program – not only
 *     when the child ends (a long-running service started with posix_spawn would hold its parent);
 *  2. flock: exclusive lock, refused for another process (EWOULDBLOCK), given back with LOCK_UN and
 *     when the holder ends.
 * Prints "SPAWNTEST: ALLES OK" when everything holds. `spawntest sleep <ms>` is the child program. */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/file.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

static int failures;
static void check(int ok, const char *what) {
    printf("%s %s\n", ok ? "ok  " : "FEHLER", what);
    if (!ok) failures++;
}

static long now_ms(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return t.tv_sec * 1000 + t.tv_nsec / 1000000;
}

static void vfork_exec(const char *self) {
    long start = now_ms();
    pid_t pid = vfork();
    if (pid == 0) {
        execl(self, self, "sleep", "3000", (char *)0);
        _exit(127);
    }
    long back = now_ms() - start;
    check(pid > 0, "vfork");
    char msg[96];
    snprintf(msg, sizeof msg, "parent continues after the child's exec (%ld ms, child sleeps 3000 ms)", back);
    check(back < 1500, msg);
    int st = 0;
    check(waitpid(pid, &st, 0) == pid && WIFEXITED(st) && WEXITSTATUS(st) == 0, "child ran to its end");
}

static void locks(void) {
    const char *path = "/tmp/spawntest.lock";
    int fd = open(path, O_CREAT | O_RDWR, 0600);
    check(fd >= 0, "lock file opened");
    check(flock(fd, LOCK_EX | LOCK_NB) == 0, "exclusive lock taken");
    pid_t pid = fork();
    if (pid == 0) {
        int f2 = open(path, O_RDWR);
        int r = flock(f2, LOCK_EX | LOCK_NB);
        _exit(r == -1 && errno == EWOULDBLOCK ? 0 : 1);
    }
    int st = 0;
    waitpid(pid, &st, 0);
    check(WIFEXITED(st) && WEXITSTATUS(st) == 0, "second process is refused (EWOULDBLOCK)");
    check(flock(fd, LOCK_UN) == 0, "lock given back");
    pid = fork();
    if (pid == 0) {
        int f2 = open(path, O_RDWR);
        if (flock(f2, LOCK_EX | LOCK_NB) != 0) _exit(1);
        _exit(0); /* ends while holding the lock */
    }
    waitpid(pid, &st, 0);
    check(WIFEXITED(st) && WEXITSTATUS(st) == 0, "other process takes the free lock");
    check(flock(fd, LOCK_EX | LOCK_NB) == 0, "lock free again after its holder ended");
    close(fd);
}

int main(int argc, char **argv) {
    if (argc == 3 && strcmp(argv[1], "sleep") == 0) {
        usleep(atoi(argv[2]) * 1000);
        return 0;
    }
    setvbuf(stdout, NULL, _IONBF, 0);
    /* The kernel self test starts programs with a bare name as argv[0]; exec needs a path. */
    char self[256];
    if (strchr(argv[0], '/')) {
        snprintf(self, sizeof self, "%s", argv[0]);
    } else {
        snprintf(self, sizeof self, "/bin/%s", argv[0]);
    }
    vfork_exec(self);
    locks();
    printf(failures ? "SPAWNTEST: FEHLER (%d)\n" : "SPAWNTEST: ALLES OK\n", failures);
    return failures != 0;
}
