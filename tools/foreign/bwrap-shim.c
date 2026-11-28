/* bwrap replacement of the Ultar Linux area (docs/integration/ultar-foreign.md, "bwrap in a cell").
 *
 * GTK's image loader glycin starts every loader through bubblewrap, which needs user namespaces – the
 * Ultar kernel has none, and an app of the Linux area already runs in its own cell (own user, own view,
 * no rights, network as allowed). This program understands bwrap's command line, applies what changes
 * only the started program's own environment (--chdir, --setenv, --unsetenv, --clearenv, --argv0,
 * --die-with-parent, --new-session, --block-fd, --args) and starts the program as it is: in the same
 * cell, with the same user and nothing more. The seccomp filters it is handed (--seccomp,
 * --add-seccomp-fd) are put on the program right before it starts, as bwrap does – where the kernel
 * has seccomp, glycin's own syscall filter still holds (the Ultar kernel has none: the cell is the
 * boundary there). Every namespace, mount and capability request is ignored – none of them could give
 * the program anything the cell does not already give its app. Descriptors handed over for those
 * (--file, --bind-data, --info-fd …) are closed. Built static on the build host
 * (tools/foreign/make-linux-runtime.sh) and installed as the package "bubblewrap".
 */
#define _GNU_SOURCE
#include <errno.h>
#include <linux/filter.h>
#include <linux/seccomp.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/prctl.h>
#include <unistd.h>

#define MAX_ARGS 4096
#define MAX_FILTERS 16
#define MAX_FILTER (64 * 1024)

struct opt {
    const char *name;
    int nargs;
};

/* options that only describe the sandbox (ignored here) and the number of values each takes */
static const struct opt IGNORED[] = {
    {"--unshare-all", 0}, {"--share-net", 0}, {"--unshare-user", 0}, {"--unshare-user-try", 0},
    {"--unshare-ipc", 0}, {"--unshare-pid", 0}, {"--unshare-net", 0}, {"--unshare-uts", 0},
    {"--unshare-cgroup", 0}, {"--unshare-cgroup-try", 0}, {"--as-pid-1", 0},
    {"--disable-userns", 0}, {"--assert-userns-disabled", 0},
    {"--userns", 1}, {"--userns2", 1}, {"--pidns", 1}, {"--uid", 1}, {"--gid", 1}, {"--hostname", 1},
    {"--lock-file", 1}, {"--sync-fd", 1}, {"--remount-ro", 1}, {"--proc", 1}, {"--dev", 1},
    {"--tmpfs", 1}, {"--mqueue", 1}, {"--dir", 1}, {"--cap-add", 1},
    {"--cap-drop", 1}, {"--perms", 1}, {"--size", 1}, {"--exec-label", 1}, {"--file-label", 1},
    {"--overlay-src", 1}, {"--tmp-overlay", 1}, {"--ro-overlay", 1}, {"--level-prefix", 0},
    {"--bind", 2}, {"--bind-try", 2}, {"--dev-bind", 2}, {"--dev-bind-try", 2}, {"--ro-bind", 2},
    {"--ro-bind-try", 2}, {"--symlink", 2}, {"--chmod", 2}, {"--overlay", 3},
};

/* options whose first value is a descriptor bwrap would use up for the sandbox: closed here */
static const struct opt CLOSED[] = {
    {"--info-fd", 1}, {"--json-status-fd", 1}, {"--file", 2}, {"--bind-data", 2},
    {"--ro-bind-data", 2}, {"--bind-fd", 2}, {"--ro-bind-fd", 2},
};

static void die(const char *what) {
    fprintf(stderr, "bwrap (Ultar cell): %s\n", what);
    exit(1);
}

static int values_of(const struct opt *table, size_t len, const char *a) {
    for (size_t i = 0; i < len; i++)
        if (strcmp(a, table[i].name) == 0)
            return table[i].nargs;
    return -1;
}

/* a descriptor number given on the command line */
static int fd_of(const char *v) {
    char *end;
    long fd = strtol(v, &end, 10);
    if (*v == 0 || *end != 0 || fd < 0 || fd > 65535)
        die("not a descriptor");
    return (int)fd;
}

/* a seccomp filter as bwrap gets it: raw struct sock_filter, read from a descriptor */
struct filter {
    struct sock_fprog prog;
};

static void read_filter(int fd, struct filter *f) {
    unsigned char *buf = malloc(MAX_FILTER);
    if (!buf)
        die("no memory for a seccomp filter");
    size_t len = 0;
    ssize_t r;
    while ((r = read(fd, buf + len, MAX_FILTER - len)) > 0 || (r < 0 && errno == EINTR))
        if (r > 0)
            len += (size_t)r;
    close(fd);
    if (r < 0 || len == 0 || len == MAX_FILTER || len % sizeof(struct sock_filter) != 0)
        die("--seccomp: not a filter");
    f->prog.len = (unsigned short)(len / sizeof(struct sock_filter));
    f->prog.filter = (struct sock_filter *)buf;
}

/* bwrap puts its filters on the program last, after everything else it does */
static void apply_filters(struct filter *filters, int count) {
    if (count == 0)
        return;
    if (prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0)
        die("PR_SET_NO_NEW_PRIVS failed");
    for (int i = 0; i < count; i++)
        if (prctl(PR_SET_SECCOMP, SECCOMP_MODE_FILTER, &filters[i].prog) != 0) {
            /* a kernel without seccomp (Ultar): there the cell is the boundary */
            if (prctl(PR_GET_SECCOMP, 0, 0, 0, 0) < 0 && (errno == EINVAL || errno == ENOSYS))
                return;
            die("the seccomp filter does not hold");
        }
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
    static char *args[MAX_ARGS + 1];
    static struct filter filters[MAX_FILTERS];
    int nfilters = 0;
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
        int skip = values_of(IGNORED, sizeof IGNORED / sizeof IGNORED[0], a);
        int closed = values_of(CLOSED, sizeof CLOSED / sizeof CLOSED[0], a);
        if (skip >= 0) {
            if (i + skip >= n)
                die("option without its value");
            i += skip;
        } else if (closed >= 0) {
            if (i + closed >= n)
                die("option without its value");
            close(fd_of(args[i + 1]));
            i += closed;
        } else if ((strcmp(a, "--seccomp") == 0 || strcmp(a, "--add-seccomp-fd") == 0) && i + 1 < n) {
            if (nfilters == MAX_FILTERS)
                die("too many seccomp filters");
            read_filter(fd_of(args[++i]), &filters[nfilters++]);
        } else if (strcmp(a, "--new-session") == 0) {
            setsid();
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
        } else if ((strcmp(a, "--block-fd") == 0 || strcmp(a, "--userns-block-fd") == 0) && i + 1 < n) {
            char c;
            int fd = fd_of(args[++i]);
            while (read(fd, &c, 1) < 0 && errno == EINTR) {
            }
        } else if (strcmp(a, "--args") == 0 && i + 1 < n) {
            /* the read arguments take the place of "--args FD" */
            static char *more[MAX_ARGS];
            int m = read_args(fd_of(args[i + 1]), more, MAX_ARGS);
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
    apply_filters(filters, nfilters);
    execvp(prog, &args[i]);
    perror("bwrap (Ultar cell): exec");
    return 127;
}
