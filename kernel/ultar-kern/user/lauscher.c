// Testdienst für den Angriffstest: TCP 8080 und UDP 8081 antworten „hallo“ (nur erreichbar, wenn die Brandmauer es erlaubt)
#include <arpa/inet.h>
#include <poll.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>
#include <signal.h>
int main(int argc, char **argv) {
  int t = socket(AF_INET, SOCK_STREAM, 0), u = socket(AF_INET, SOCK_DGRAM, 0), on = 1;
  setsockopt(t, SOL_SOCKET, SO_REUSEADDR, &on, sizeof on);
  struct sockaddr_in a = {AF_INET, htons(8080), {0}};
  if (bind(t, (void *)&a, sizeof a) || listen(t, 16)) { perror("tcp"); return 1; }
  a.sin_port = htons(8081);
  if (bind(u, (void *)&a, sizeof a)) { perror("udp"); return 1; }
  printf("LAUSCHER BEREIT tcp 8080 udp 8081\n"); fflush(stdout);
  signal(SIGCHLD, SIG_IGN);
  int n = 0;
  for (;;) {
    struct pollfd p[2] = {{t, POLLIN, 0}, {u, POLLIN, 0}};
    if (poll(p, 2, -1) <= 0) continue;
    if (p[0].revents & POLLIN) { int c = accept(t, 0, 0); if (c >= 0) { if (fork() == 0) { sleep(2); write(c, "hallo\n", 6); _exit(0); } close(c); n++; } }
    if (p[1].revents & POLLIN) { char b[512]; struct sockaddr_in v; socklen_t l = sizeof v;
      int k = recvfrom(u, b, sizeof b, 0, (void *)&v, &l); if (k >= 0) sendto(u, "hallo\n", 6, 0, (void *)&v, l); }
  }
}
