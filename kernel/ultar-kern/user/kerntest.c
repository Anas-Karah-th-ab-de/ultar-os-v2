/* Prüfprogramm für den Ultar-Kern: Fäden (pthread, futex), Signale, TCP/UDP über 127.0.0.1. */
#include <arpa/inet.h>
#include <pthread.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/wait.h>
#include <unistd.h>

static pthread_mutex_t m = PTHREAD_MUTEX_INITIALIZER;
static long counter;
static volatile int got_usr1, got_chld;

static void *work(void *arg) {
    for (int i = 0; i < 100000; i++) { pthread_mutex_lock(&m); counter++; pthread_mutex_unlock(&m); }
    return arg;
}
static void on_usr1(int s) { (void)s; got_usr1++; }
static void on_chld(int s) { (void)s; got_chld++; }

static void *server(void *arg) {
    int ls = *(int *)arg;
    int c = accept(ls, 0, 0);
    char b[64] = {0};
    int n = read(c, b, sizeof b - 1);
    if (n > 0 && !strcmp(b, "ping")) write(c, "pong", 4);
    close(c);
    return 0;
}

int main(void) {
    int ok = 1;
    pthread_t t[4];
    for (long i = 0; i < 4; i++) pthread_create(&t[i], 0, work, (void *)i);
    for (int i = 0; i < 4; i++) pthread_join(t[i], 0);
    printf("Fäden: 4 × 100000 = %ld %s\n", counter, counter == 400000 ? "OK" : "FEHLER");
    ok &= counter == 400000;

    struct sigaction sa = {0};
    sa.sa_handler = on_usr1; sigaction(SIGUSR1, &sa, 0);
    sa.sa_handler = on_chld; sigaction(SIGCHLD, &sa, 0);
    kill(getpid(), SIGUSR1);
    pid_t c = fork();
    if (c == 0) _exit(3);
    int st; while (waitpid(c, &st, 0) < 0) {}
    printf("Signale: SIGUSR1 %d, SIGCHLD %d, Kind-Status %d %s\n", got_usr1, got_chld, WEXITSTATUS(st), got_usr1 == 1 && got_chld >= 1 && WEXITSTATUS(st) == 3 ? "OK" : "FEHLER");
    ok &= got_usr1 == 1 && WEXITSTATUS(st) == 3;

    int ls = socket(AF_INET, SOCK_STREAM, 0);
    struct sockaddr_in a = {.sin_family = AF_INET, .sin_port = htons(7777), .sin_addr.s_addr = htonl(0x7f000001)};
    int one = 1; setsockopt(ls, SOL_SOCKET, SO_REUSEADDR, &one, sizeof one);
    if (bind(ls, (void *)&a, sizeof a) || listen(ls, 4)) { perror("bind/listen"); return 1; }
    pthread_t st_t; pthread_create(&st_t, 0, server, &ls);
    int s = socket(AF_INET, SOCK_STREAM, 0);
    if (connect(s, (void *)&a, sizeof a)) { perror("connect"); return 1; }
    write(s, "ping", 4);
    char b[16] = {0};
    int n = read(s, b, sizeof b - 1);
    pthread_join(st_t, 0);
    printf("TCP 127.0.0.1:7777: gesendet ping, erhalten %s %s\n", n > 0 ? b : "-", !strcmp(b, "pong") ? "OK" : "FEHLER");
    ok &= !strcmp(b, "pong");

    int u1 = socket(AF_INET, SOCK_DGRAM, 0), u2 = socket(AF_INET, SOCK_DGRAM, 0);
    struct sockaddr_in ua = {.sin_family = AF_INET, .sin_port = htons(7778), .sin_addr.s_addr = htonl(0x7f000001)};
    bind(u1, (void *)&ua, sizeof ua);
    sendto(u2, "udp-hallo", 9, 0, (void *)&ua, sizeof ua);
    char ub[32] = {0};
    n = recv(u1, ub, sizeof ub - 1, 0);
    printf("UDP 127.0.0.1:7778: %s %s\n", ub, !strcmp(ub, "udp-hallo") ? "OK" : "FEHLER");
    ok &= !strcmp(ub, "udp-hallo");
    printf("Ergebnis: %s\n", ok ? "ALLES OK" : "FEHLER");
    return !ok;
}
