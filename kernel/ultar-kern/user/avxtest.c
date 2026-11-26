// AVX state on the Ultar kernel (kernel/ultar-kern/src/trap.rs, arch.rs xsave_an, signal.rs):
//  1. CPUID reports OSXSAVE and AVX, xgetbv(0) has x87|SSE|AVX – so glibc, Graviola and llama.cpp take their
//     AVX paths (without it they fall back to SSE or refuse).
//  2. Four threads (more than the test machine's cores) keep their own pattern in all 16 YMM registers across
//     timer preemption and sched_yield; a lost upper half (the kernel saving only the SSE part) shows here.
//  3. A signal handler that overwrites every YMM register does not change the interrupted code's registers
//     (the signal frame carries the whole XSAVE area, rt_sigreturn restores it).
//  4. fork: the child sees the parent's YMM values.
// Verdict: "AVXTEST: ALLES OK". On a processor without AVX: "AVXTEST: KEIN AVX" (not an error of the kernel).
#define _GNU_SOURCE
#include <cpuid.h>
#include <pthread.h>
#include <sched.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

static int fails;
#define CHECK(c, ...)                                         \
  do {                                                        \
    if (!(c)) {                                               \
      printf("AVXTEST: FEHLER " __VA_ARGS__);                 \
      printf("\n");                                           \
      fails++;                                                \
    }                                                         \
  } while (0)

static uint64_t xgetbv0(void) {
  uint32_t lo, hi;
  __asm__ volatile("xgetbv" : "=a"(lo), "=d"(hi) : "c"(0));
  return ((uint64_t)hi << 32) | lo;
}

// Loads ymm0..ymm15 from `src`, then either spins `spin` integer iterations or makes system call `nr` with
// arguments a1, a2, then stores ymm0..ymm15 to `dst` – all in one block, so no compiler or library code touches
// the vector registers in between (GCC would insert vzeroupper at a function boundary). Returns rax.
#define LOADS                                                                                              \
  "vmovdqu 0(%[s]), %%ymm0\n vmovdqu 32(%[s]), %%ymm1\n vmovdqu 64(%[s]), %%ymm2\n vmovdqu 96(%[s]), %%ymm3\n" \
  "vmovdqu 128(%[s]), %%ymm4\n vmovdqu 160(%[s]), %%ymm5\n vmovdqu 192(%[s]), %%ymm6\n vmovdqu 224(%[s]), %%ymm7\n" \
  "vmovdqu 256(%[s]), %%ymm8\n vmovdqu 288(%[s]), %%ymm9\n vmovdqu 320(%[s]), %%ymm10\n vmovdqu 352(%[s]), %%ymm11\n" \
  "vmovdqu 384(%[s]), %%ymm12\n vmovdqu 416(%[s]), %%ymm13\n vmovdqu 448(%[s]), %%ymm14\n vmovdqu 480(%[s]), %%ymm15\n"
#define STORES                                                                                             \
  "vmovdqu %%ymm0, 0(%[d])\n vmovdqu %%ymm1, 32(%[d])\n vmovdqu %%ymm2, 64(%[d])\n vmovdqu %%ymm3, 96(%[d])\n" \
  "vmovdqu %%ymm4, 128(%[d])\n vmovdqu %%ymm5, 160(%[d])\n vmovdqu %%ymm6, 192(%[d])\n vmovdqu %%ymm7, 224(%[d])\n" \
  "vmovdqu %%ymm8, 256(%[d])\n vmovdqu %%ymm9, 288(%[d])\n vmovdqu %%ymm10, 320(%[d])\n vmovdqu %%ymm11, 352(%[d])\n" \
  "vmovdqu %%ymm12, 384(%[d])\n vmovdqu %%ymm13, 416(%[d])\n vmovdqu %%ymm14, 448(%[d])\n vmovdqu %%ymm15, 480(%[d])\n" \
  "vzeroupper\n"
#define YMM_CLOBBERS                                                                                       \
  "memory", "rcx", "r11", "xmm0", "xmm1", "xmm2", "xmm3", "xmm4", "xmm5", "xmm6", "xmm7", "xmm8", "xmm9",   \
      "xmm10", "xmm11", "xmm12", "xmm13", "xmm14", "xmm15"

static long spin_keep(const uint8_t *src, uint8_t *dst, unsigned long spin) {
  long rax = 0;
  __asm__ volatile(LOADS "1: dec %[n]\n jnz 1b\n" STORES
                   : [n] "+r"(spin), "+a"(rax)
                   : [s] "r"(src), [d] "r"(dst)
                   : YMM_CLOBBERS);
  return rax;
}

static long sys_keep(const uint8_t *src, uint8_t *dst, long nr, long a1, long a2) {
  long rax = nr;
  __asm__ volatile(LOADS "syscall\n" STORES
                   : "+a"(rax)
                   : [s] "r"(src), [d] "r"(dst), "D"(a1), "S"(a2)
                   : YMM_CLOBBERS);
  return rax;
}

// Writes a different value into every YMM register (the signal handler).
static void clobber_all(void) {
  uint8_t junk[512], sink[512];
  memset(junk, 0xa5, sizeof junk);
  spin_keep(junk, sink, 1);
}

static void pattern(uint8_t *p, unsigned seed) {
  for (int i = 0; i < 512; i++) p[i] = (uint8_t)(seed * 131 + i * 7 + (i >> 4));
}

// Keeps the pattern in the registers across timer ticks (spinning) and sched_yield.
static int hold(unsigned seed, int rounds) {
  uint8_t want[512], got[512];
  pattern(want, seed);
  for (int r = 0; r < rounds; r++) {
    memset(got, 0, sizeof got);
    if (r & 1)
      sys_keep(want, got, 24 /* sched_yield */, 0, 0);
    else
      spin_keep(want, got, 30000000UL);
    if (memcmp(want, got, 512) != 0) return r + 1;
  }
  return 0;
}

static void *thread_main(void *arg) {
  unsigned seed = (unsigned)(uintptr_t)arg;
  return (void *)(uintptr_t)hold(seed, 12);
}

static volatile sig_atomic_t handled;
static void clobber(int sig) {
  (void)sig;
  clobber_all();
  handled = 1;
}

int main(void) {
  unsigned a, b, c, d;
  __cpuid(1, a, b, c, d);
  int osxsave = (c >> 27) & 1, avx = (c >> 28) & 1;
  if (!avx) {
    printf("AVXTEST: KEIN AVX (CPUID.1:ECX.AVX = 0)\n");
    return 0;
  }
  CHECK(osxsave, "CPUID meldet OSXSAVE nicht (der Kern hat XSAVE nicht eingeschaltet)");
  if (!osxsave) return 1;
  uint64_t xcr0 = xgetbv0();
  CHECK((xcr0 & 7) == 7, "XCR0 = %#llx statt x87|SSE|AVX", (unsigned long long)xcr0);
  if ((xcr0 & 7) != 7) return 1;

  // 2. threads
  pthread_t t[4];
  for (unsigned i = 0; i < 4; i++) pthread_create(&t[i], NULL, thread_main, (void *)(uintptr_t)(i + 1));
  for (int i = 0; i < 4; i++) {
    void *r;
    pthread_join(t[i], &r);
    CHECK(r == NULL, "Faden %d: YMM-Register nach Runde %lu verändert", i + 1, (unsigned long)(uintptr_t)r);
  }

  // 3. signal handler clobbers every YMM register
  signal(SIGUSR1, clobber);
  uint8_t want[512], got[512];
  pattern(want, 77);
  sys_keep(want, got, 62 /* kill */, getpid(), SIGUSR1);
  CHECK(handled, "Signal nicht zugestellt");
  CHECK(memcmp(want, got, 512) == 0, "YMM-Register nach dem Signal-Handler verändert");

  // 4. fork
  // fork right between loading and storing
  pattern(want, 99);
  memset(got, 0, sizeof got);
  long pid = sys_keep(want, got, 57 /* fork */, 0, 0);
  if (pid == 0) _exit(memcmp(want, got, 512) == 0 ? 0 : 7);
  int st = 0;
  waitpid((pid_t)pid, &st, 0);
  CHECK(WIFEXITED(st) && WEXITSTATUS(st) == 0, "Kind sieht andere YMM-Register (Status %#x)", st);

  if (fails == 0) printf("AVXTEST: ALLES OK (XCR0 %#llx, 4 Faeden, Signal, fork)\n", (unsigned long long)xcr0);
  return fails != 0;
}
