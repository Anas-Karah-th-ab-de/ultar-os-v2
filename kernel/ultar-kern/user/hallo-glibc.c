/* Test: gewöhnliches C-Programm mit glibc (statisch), wie es Ubuntu baut. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/utsname.h>
#include <time.h>
#include <unistd.h>
#include <sys/wait.h>
int main(int argc, char **argv) {
    struct utsname u; uname(&u);
    printf("glibc-Programm läuft auf %s %s (%s), pid %d\n", u.sysname, u.release, u.machine, getpid());
    char *p = malloc(1 << 20); memset(p, 'x', 1 << 20); p[10] = 0;
    printf("malloc 1 MB ok: %s\n", p);
    char *q = malloc(64 << 20); memset(q, 1, 64 << 20);
    printf("malloc 64 MB (mmap) ok\n");
    time_t t = time(0); printf("Zeit: %s", ctime(&t));
    pid_t c = fork();
    if (c == 0) { printf("Kindprozess %d\n", getpid()); _exit(7); }
    int st; waitpid(c, &st, 0); printf("Kind %d beendet mit %d\n", c, WEXITSTATUS(st));
    return 0;
}
