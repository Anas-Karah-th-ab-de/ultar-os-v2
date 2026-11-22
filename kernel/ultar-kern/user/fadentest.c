// Fäden erzeugen/beenden und Speicher beobachten (Leck-Test für mmap/munmap von Fadenstapeln)
#include <pthread.h>
#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include <sys/mman.h>
#include <spawn.h>
#include <sys/wait.h>
#include <unistd.h>
extern char **environ;
static void *f(void *a) { volatile char b[64 * 1024]; memset((char *)b, 1, sizeof b); return a; }
static long frei(void) {
  FILE *m = fopen("/proc/meminfo", "r"); char z[128]; long v = 0;
  while (m && fgets(z, sizeof z, m)) if (sscanf(z, "MemFree: %ld", &v) == 1) break;
  if (m) fclose(m); return v;
}
int main(int argc, char **argv) {
  int n = argc > 1 ? atoi(argv[1]) : 200;
  long a = frei();
  for (int i = 0; i < n; i++) { pthread_t t; pthread_create(&t, 0, f, 0); pthread_join(t, 0); }
  long b = frei();
  for (int i = 0; i < n; i++) { void *p = mmap(0, 2 << 20, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0); memset(p, 1, 256 * 1024); munmap(p, 2 << 20); }
  long c = frei();
  char *ballast = malloc(32 << 20); memset(ballast, 7, 32 << 20);
  long d = frei();
  for (int i = 0; i < 20; i++) {
    pid_t p; char *av[] = {"true", 0};
    if (posix_spawn(&p, argc > 2 ? argv[2] : "/usr/bin/true", 0, 0, av, environ) == 0) waitpid(p, 0, 0);
  }
  long e = frei();
  for (int i = 0; i < 20; i++) { pid_t p = fork(); if (p == 0) _exit(0); waitpid(p, 0, 0); }
  long g = frei();
  printf("faeden: %ld kB verloren, mmap: %ld kB verloren, posix_spawn x20: %ld kB, fork x20: %ld kB\n", a - b, b - c, d - e, e - g);
  free(ballast);
  return 0;
}
