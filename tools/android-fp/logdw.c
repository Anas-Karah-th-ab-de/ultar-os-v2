// Minimaler logd-Ersatz: empfängt Android-Logzeilen (/dev/socket/logdw) und schreibt sie als Text.
// Aufruf: logdw <socket> <ausgabedatei>
#include <stdio.h>
#include <string.h>
#include <unistd.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <sys/stat.h>
int main(int c, char **v) {
  if (c < 3) return 1;
  int s = socket(AF_UNIX, SOCK_DGRAM, 0);
  struct sockaddr_un a = {.sun_family = AF_UNIX};
  strncpy(a.sun_path, v[1], sizeof a.sun_path - 1);
  unlink(v[1]);
  if (bind(s, (struct sockaddr *)&a, sizeof a) < 0) { perror("bind"); return 1; }
  chmod(v[1], 0666);
  FILE *f = fopen(v[2], "a");
  if (!f) return 1;
  static const char P[] = "??VDIWEF";
  unsigned char b[5000];
  for (;;) {
    ssize_t n = recv(s, b, sizeof b - 1, 0);
    if (n < 13) continue;
    b[n] = 0;
    if (b[0] == 2) continue;                  // Binär-Events (LOG_ID_EVENTS) überspringen
    unsigned tid = b[1] | b[2] << 8;
    unsigned char pr = b[11];
    const char *tag = (const char *)b + 12;
    const char *msg = tag + strlen(tag) + 1;
    if ((unsigned char *)msg > b + n) msg = "";
    fprintf(f, "%c %5u %s: %s\n", pr < 8 ? P[pr] : '?', tid, tag, msg);
    fflush(f);
  }
}
