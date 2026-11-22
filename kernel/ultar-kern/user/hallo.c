/* Kleinstes Testprogramm für den Ultar-Kern: nur Systemaufrufe, keine C-Bibliothek. */
static long sys(long n, long a, long b, long c) {
    long r;
    __asm__ volatile("syscall" : "=a"(r) : "a"(n), "D"(a), "S"(b), "d"(c) : "rcx", "r11", "memory");
    return r;
}
static unsigned long len(const char *s) { unsigned long n = 0; while (s[n]) n++; return n; }
void _start_c(long *sp) {
    long argc = sp[0];
    char **argv = (char **)(sp + 1);
    const char *m = "Hallo vom Ultar-Kern! Ring 3, eigener Adressraum, Linux-Systemaufrufe.\n";
    sys(1, 1, (long)m, len(m));
    for (long i = 0; i < argc; i++) {
        sys(1, 1, (long)"  arg: ", 7);
        sys(1, 1, (long)argv[i], len(argv[i]));
        sys(1, 1, (long)"\n", 1);
    }
    sys(60, 0, 0, 0);
}
__asm__(".global _start\n_start:\n mov %rsp, %rdi\n and $-16, %rsp\n call _start_c\n");
