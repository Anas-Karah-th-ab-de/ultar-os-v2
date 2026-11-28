/* bwrap replacement of the Ultar Linux area (docs/integration/ultar-foreign.md, "bwrap in a cell").
 *
 * GTK's image loader glycin starts every loader through bubblewrap, which needs user namespaces – the
 * Ultar kernel has none, and an app of the Linux area already runs in its own cell (own user, own view,
 * no rights, network as allowed). This program understands bwrap's command line, applies what changes
 * only the started program's own environment (--chdir, --setenv, --unsetenv, --clearenv, --argv0,
 * --die-with-parent, --block-fd, --args) and starts the program as it is: in the same cell, with the
 * same user and nothing more. Every namespace, mount and capability request is ignored – none of them
 * could give the program anything the cell does not already give its app. Built static on the build
 * host (tools/foreign/make-linux-runtime.sh) and installed as the package "bubblewrap".
 */
#define _GNU_SOURCE
#include <errno.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/prctl.h>
#include <unistd.h>

#define MAX_ARGS 4096

struct opt {
    const char *name;
    int nargs;
};

/* options that only describe the sandbox (ignored here) and the number of values each takes */
static const struct opt IGNORED[] = {
    {"--unshare-all", 0}, {"--share-net", 0}, {"--unshare-user", 0}, {"--unshare-user-try", 0},
    {"--unshare-ipc", 0}, {"--unshare-pid", 0}, {"--unshare-net", 0}, {"--unshare-uts", 0},
    {"--unshare-cgroup", 0}, {"--unshare-cgroup-try", 0}, {"--as-pid-1", 0}, {"--new-session", 0},
    {"--disable-userns", 0}, {"--assert-userns-disabled", 0},
    {"--userns", 1}, {"--userns2", 1}, {"--pidns", 1}, {"--uid", 1}, {"--gid", 1}, {"--hostname", 1},
    {"--lock-file", 1}, {"--sync-fd", 1}, {"--remount-ro", 1}, {"--proc", 1}, {"--dev", 1},
    {"--tmpfs", 1}, {"--mqueue", 1}, {"--dir", 1}, {"--seccomp", 1}, {"--add-seccomp-fd", 1},
    {"--userns-block-fd", 1}, {"--info-fd", 1}, {"--json-status-fd", 1}, {"--cap-add", 1},
    {"--cap-drop", 1}, {"--perms", 1}, {"--size", 1}, {"--exec-label", 1}, {"--file-label", 1},
    {"--overlay-src", 1}, {"--tmp-overlay", 1}, {"--ro-overlay", 1}, {"--level-prefix", 0},
    {"--bind", 2}, {"--bind-try", 2}, {"--dev-bind", 2}, {"--dev-bind-try", 2}, {"--ro-bind", 2},
    {"--ro-bind-try", 2}, {"--bind-fd", 2}, {"--ro-bind-fd", 2}, {"--file", 2}, {"--bind-data", 2},
    {"--ro-bind-data", 2}, {"--symlink", 2}, {"--chmod", 2}, {"--overlay", 3},
};

static void die(const char *what) {
    fprintf(stderr, "bwrap (Ultar cell): %s\n", what);
    exit(1);
}

static int ignored_values(const char *a) {
    for (size_t i = 0; i < sizeof IGNORED / sizeof IGNORED[0]; i++)
        if (strcmp(a, IGNORED[i].name) == 0)
            return IGNORED[i].nargs;
    return -1;
}

/* read NUL-separated arguments from a descriptor (bwrap's --args FD) into out[] */
static int read_args(int fd, char **out, int room) {
    static char buf[1 << 16];
    size_t len = 0;
    ssize_t r;
    while ((r = read(fd, buf + len, sizeof buf - 1 - len)) > 0)
        len += (size_t)r;
    if (r < 0)
        die("--args: read failed");
    buf[len] = 0;
    int n = 0;
    for (size_t i = 0; i < len && n < room; i += strlen(buf + i) + 1)
        out[n++] = buf + i;
    return n;
}

int main(int argc, char **argv) {
    static char *args[MAX_ARGS];
    int n = 0;
    for (int i = 1; i < argc && n < MAX_ARGS; i++)
        args[n++] = argv[i];
    const char *chdir_to = NULL, *argv0 = NULL;
    int i = 0;
    for (; i < n; i++) {
        const char *a = args[i];
        if (strcmp(a, "--") == 0) {
            i++;
            break;
        }
        if (strncmp(a, "--", 2) != 0)
            break;
        if (strcmp(a, "--version") == 0) {
            puts("bubblewrap 0.11.0 (Ultar cell: no namespaces, runs the program in the app's cell)");
            return 0;
        }
        int skip = ignored_values(a);
        if (skip >= 0) {
            if (i + skip >= n)
                die("option without its value");
            i += skip;
        } else if (strcmp(a, "--die-with-parent") == 0) {
            prctl(PR_SET_PDEATHSIG, SIGKILL);
        } else if (strcmp(a, "--clearenv") == 0) {
            clearenv();
        } else if (strcmp(a, "--chdir") == 0 && i + 1 < n) {
            chdir_to = args[++i];
        } else if (strcmp(a, "--argv0") == 0 && i + 1 < n) {
            argv0 = args[++i];
        } else if (strcmp(a, "--unsetenv") == 0 && i + 1 < n) {
            unsetenv(args[++i]);
        } else if (strcmp(a, "--setenv") == 0 && i + 2 < n) {
            setenv(args[i + 1], args[i + 2], 1);
            i += 2;
        } else if (strcmp(a, "--block-fd") == 0 && i + 1 < n) {
            char c;
            while (read(atoi(args[++i]), &c, 1) < 0 && errno == EINTR) {
            }
        } else if (strcmp(a, "--args") == 0 && i + 1 < n) {
            /* the read arguments take the place of "--args FD" */
            static char *more[MAX_ARGS];
            int m = read_args(atoi(args[i + 1]), more, MAX_ARGS);
            if (n - (i + 2) + m > MAX_ARGS)
                die("too many arguments");
            memmove(&args[i + m], &args[i + 2], (size_t)(n - (i + 2)) * sizeof args[0]);
            memcpy(&args[i], more, (size_t)m * sizeof args[0]);
            n = n - 2 + m;
            i--;
        } else {
            fprintf(stderr, "bwrap (Ultar cell): unknown option %s\n", a);
            return 1;
        }
    }
    if (i >= n)
        die("no program to start");
    if (chdir_to && chdir(chdir_to) != 0)
        die("--chdir failed");
    char *prog = args[i];
    if (argv0)
        args[i] = (char *)argv0;
    args[n] = NULL;
    execvp(prog, &args[i]);
    perror("bwrap (Ultar cell): exec");
    return 127;
}
