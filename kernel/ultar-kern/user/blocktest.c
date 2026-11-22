// Ein Faden blockiert in recvfrom/accept, der Hauptfaden muss trotzdem pünktlich weiterlaufen.
#include <pthread.h>
#include <stdio.h>
#include <time.h>
#include <unistd.h>
#include <sys/socket.h>
#include <netinet/in.h>
static double jetzt(void) { struct timespec t; clock_gettime(CLOCK_MONOTONIC, &t); return t.tv_sec + t.tv_nsec / 1e9; }
static void *udp(void *a) {
  int s = socket(AF_INET, SOCK_DGRAM, 0); struct sockaddr_in x = {AF_INET, htons(47999), {0}};
  bind(s, (void *)&x, sizeof x); char b[64]; recvfrom(s, b, sizeof b, 0, 0, 0); return a;
}
static void *tcp(void *a) {
  int s = socket(AF_INET, SOCK_STREAM, 0); struct sockaddr_in x = {AF_INET, htons(47998), {0}};
  bind(s, (void *)&x, sizeof x); listen(s, 4); accept(s, 0, 0); return a;
}
int main(void) {
  pthread_t t1, t2; pthread_create(&t1, 0, udp, 0); pthread_create(&t2, 0, tcp, 0);
  double s = jetzt(), max = 0, last = s;
  for (int i = 0; i < 10; i++) { usleep(500000); double n = jetzt(); if (n - last > max) max = n - last; last = n; }
  printf("blocktest: 10 x 500 ms in %.2f s, laengste Luecke %.2f s – %s\n", jetzt() - s, max, max < 1.0 ? "OK" : "FEHLER");
  return 0;
}
