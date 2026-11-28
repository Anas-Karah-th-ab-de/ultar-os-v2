/*
 * bwrap-cell: stands in for bubblewrap ("bwrap") inside an Ultar app cell
 * (docs/integration/ultar-foreign.md, "Helpers that ask for bwrap").
 *
 * The app's cell already is the sandbox: its own user, its own mount and pid view, the Linux
 * area read-only, no rights, the network of its choice. Programs of the area that start their
 * helpers through bwrap (GTK's image loaders "glycin", portals, thumbnailers) cannot get the user
 * namespaces bwrap needs – there the helper would not start and every picture stays empty.
 * This program runs the helper directly, in the same cell, with what bwrap would give it that
 * needs no rights: the environment (--clearenv, --setenv, --unsetenv), the working directory,
 * --die-with-parent, --new-session, --argv0 and the seccomp filters it is handed (where the
 * kernel has seccomp; the Ultar kernel has none – the cell is the boundary there). Namespace,
 * mount, capability and id options are accepted and ignored: the helper never gets more than
 * the app has. It is never set-id; tools/foreign/install-bwrap-cell.sh installs it.
 *
 *   bwrap [OPTION...] [--] COMMAND [ARG...]      (the options of bubblewrap 0.11)
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
#define MAX_FILTER (64 * 1024)

static void die(const char *what, const char *arg) {
  fprintf(stderr, "bwrap: %s%s%s\n", what, arg ? ": " : "", arg ? arg : "");
  exit(1);
}

/* Options and how many values follow each; NULL ends the table. */
struct option_spec { const char *name; int values; };
static const struct option_spec OPTIONS[] = {
  {"--unshare-all", 0}, {"--share-net", 0}, {"--unshare-user", 0}, {"--unshare-user-try", 0},
  {"--unshare-ipc", 0}, {"--unshare-pid", 0}, {"--unshare-net", 0}, {"--unshare-uts", 0},
  {"--unshare-cgroup", 0}, {"--unshare-cgroup-try", 0}, {"--die-with-parent", 0},
  {"--new-session", 0}, {"--as-pid-1", 0}, {"--clearenv", 0}, {"--disable-userns", 0},
  {"--assert-userns-disabled", 0}, {"--level-prefix", 0},
  {"--chdir", 1}, {"--unsetenv", 1}, {"--uid", 1}, {"--gid", 1}, {"--hostname", 1},
  {"--remount-ro", 1}, {"--dev", 1}, {"--proc", 1}, {"--tmpfs", 1}, {"--mqueue", 1},
  {"--dir", 1}, {"--exec-label", 1}, {"--file-label", 1}, {"--seccomp", 1},
  {"--add-seccomp-fd", 1}, {"--info-fd", 1}, {"--json-status-fd", 1}, {"--block-fd", 1},
  {"--userns-block-fd", 1}, {"--sync-fd", 1}, {"--cap-add", 1}, {"--cap-drop", 1},
  {"--perms", 1}, {"--size", 1}, {"--args", 1}, {"--userns", 1}, {"--userns2", 1},
  {"--pidns", 1}, {"--argv0", 1}, {"--overlay-src", 1}, {"--tmp-overlay", 1},
  {"--ro-overlay", 1},
  {"--setenv", 2}, {"--bind", 2}, {"--bind-try", 2}, {"--dev-bind", 2}, {"--dev-bind-try", 2},
  {"--ro-bind", 2}, {"--ro-bind-try", 2}, {"--bind-fd", 2}, {"--ro-bind-fd", 2},
  {"--symlink", 2}, {"--file", 2}, {"--bind-data", 2}, {"--ro-bind-data", 2}, {"--chmod", 2},
  {"--overlay", 3},
  {NULL, 0},
};

static int values_of(const char *name) {
  for (const struct option_spec *o = OPTIONS; o->name; o++)
    if (strcmp(o->name, name) == 0) return o->values;
  return -1;
}

static int fd_of(const char *text) {
  char *end = NULL;
  errno = 0;
  long fd = strtol(text, &end, 10);
  if (errno || !end || *end || fd < 0 || fd > 65535) die("not a file descriptor", text);
  return (int)fd;
}

/* --args FD: NUL-separated arguments read from FD, put in place of the option. */
static int read_args(int fd, char **out, int room) {
  static char buf[256 * 1024];
  size_t len = 0;
  ssize_t n;
  while ((n = read(fd, buf + len, sizeof buf - 1 - len)) > 0) {
    len += (size_t)n;
    if (len >= sizeof buf - 1) die("--args: too long", NULL);
  }
  if (n < 0) die("--args: read failed", NULL);
  close(fd);
  buf[len] = 0;
  int count = 0;
  for (size_t at = 0; at < len; at += strlen(buf + at) + 1) {
    if (count >= room) die("--args: too many arguments", NULL);
    out[count++] = buf + at;
  }
  return count;
}

/* bwrap applies a seccomp filter (raw struct sock_filter) right before the command. */
static void apply_filter(int fd) {
  static unsigned char prog[MAX_FILTER];
  size_t len = 0;
  ssize_t n;
  while ((n = read(fd, prog + len, sizeof prog - len)) > 0) len += (size_t)n;
  close(fd);
  if (n < 0 || len == 0 || len % sizeof(struct sock_filter) != 0) die("--seccomp: bad filter", NULL);
  struct sock_fprog fprog = { .len = (unsigned short)(len / sizeof(struct sock_filter)),
                              .filter = (struct sock_filter *)prog };
  if (prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0) return;
  /* No seccomp in the kernel (Ultar): the cell is the boundary; the helper still runs. */
  (void)prctl(PR_SET_SECCOMP, SECCOMP_MODE_FILTER, &fprog);
}

static void wait_byte(int fd) {
  char c;
  while (read(fd, &c, 1) < 0 && errno == EINTR) {}
}

int main(int argc, char **argv) {
  static char *args[MAX_ARGS];
  int n = 0;
  for (int i = 1; i < argc; i++) {
    if (n >= MAX_ARGS - 1) die("too many arguments", NULL);
    args[n++] = argv[i];
  }
  const char *chdir_to = NULL, *argv0 = NULL;
  int i = 0;
  while (i < n) {
    const char *opt = args[i];
    if (strcmp(opt, "--") == 0) { i++; break; }
    if (strncmp(opt, "--", 2) != 0) break;
    int values = values_of(opt);
    if (values < 0) die("unknown option", opt);
    if (i + values >= n) die("missing value", opt);
    const char *v1 = values > 0 ? args[i + 1] : NULL, *v2 = values > 1 ? args[i + 2] : NULL;
    if (strcmp(opt, "--args") == 0) {
      static char *more[MAX_ARGS];
      int m = read_args(fd_of(v1), more, MAX_ARGS);
      if (n - 2 + m >= MAX_ARGS) die("too many arguments", NULL);
      memmove(&args[i + m], &args[i + 2], (size_t)(n - i - 2) * sizeof args[0]);
      memcpy(&args[i], more, (size_t)m * sizeof args[0]);
      n = n - 2 + m;
      continue;
    }
    if (strcmp(opt, "--clearenv") == 0) clearenv();
    else if (strcmp(opt, "--setenv") == 0) { if (setenv(v1, v2, 1) != 0) die("--setenv", v1); }
    else if (strcmp(opt, "--unsetenv") == 0) unsetenv(v1);
    else if (strcmp(opt, "--chdir") == 0) chdir_to = v1;
    else if (strcmp(opt, "--argv0") == 0) argv0 = v1;
    else if (strcmp(opt, "--die-with-parent") == 0) prctl(PR_SET_PDEATHSIG, SIGKILL, 0, 0, 0);
    else if (strcmp(opt, "--new-session") == 0) setsid();
    else if (strcmp(opt, "--seccomp") == 0 || strcmp(opt, "--add-seccomp-fd") == 0) apply_filter(fd_of(v1));
    else if (strcmp(opt, "--block-fd") == 0 || strcmp(opt, "--userns-block-fd") == 0) wait_byte(fd_of(v1));
    else if (strcmp(opt, "--info-fd") == 0 || strcmp(opt, "--json-status-fd") == 0 ||
             strcmp(opt, "--file") == 0 || strcmp(opt, "--bind-data") == 0 ||
             strcmp(opt, "--ro-bind-data") == 0 || strcmp(opt, "--bind-fd") == 0 ||
             strcmp(opt, "--ro-bind-fd") == 0) close(fd_of(v1));
    i += 1 + values;
  }
  if (i >= n) die("no command given", NULL);
  if (chdir_to && chdir(chdir_to) != 0) die("cannot change directory", chdir_to);
  char **cmd = &args[i];
  const char *path = cmd[0];
  if (argv0) cmd[0] = (char *)argv0;
  args[n] = NULL;
  execvp(path, cmd);
  die("cannot run", path);
  return 1;
}
