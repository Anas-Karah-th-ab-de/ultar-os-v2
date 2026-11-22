/* gnsstest – prüft die GNSS-Hardware der A30 (Samsung Kepler, /dev/gnss_ipc) ohne zu blockieren.
 * Statisch aarch64. Jeder Kandidat wird in einem Kindprozess mit 2-s-Timeout geöffnet/gelesen,
 * damit ein blockierender Node nie den Aufrufer (oder die Tür) aufhängt.
 *   gnsstest           – alle Kandidaten prüfen, rohe Daten/NMEA 3 s lang dumpen
 *   gnsstest <dev>      – gezielt einen Device-Pfad dumpen
 * Hinweis: /dev/gnss_ipc liefert erst Daten, wenn der Kepler gebootet ist (LOAD_FIRMWARE),
 * siehe docs/A30-GNSS.md. Ungebootet ist das Device da, aber leer.
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <fcntl.h>
#include <unistd.h>
#include <errno.h>
#include <poll.h>
#include <signal.h>
#include <time.h>
#include <sys/wait.h>
#include <sys/stat.h>

static const char *cands[] = {
    "/dev/gnss_ipc", "/dev/gnss_boot", "/dev/gps", "/dev/ttyBCM",
    "/dev/ttySAC1", "/dev/ttySAC2", "/dev/ttySAC3", NULL
};

static void hexdump(const unsigned char *b, int n) {
    for (int i = 0; i < n; i += 16) {
        printf("  %04x  ", i);
        for (int j = 0; j < 16 && i + j < n; j++) printf("%02x ", b[i + j]);
        printf(" |");
        for (int j = 0; j < 16 && i + j < n; j++) {
            unsigned char c = b[i + j];
            putchar(c >= 32 && c < 127 ? c : '.');
        }
        printf("|\n");
    }
}

/* Im Kindprozess: Device öffnen, bis zu 3 s lesen, Daten/NMEA zeigen. */
static void probe(const char *path) {
    struct stat stt;
    if (stat(path, &stt) != 0) { printf("%s: nicht vorhanden\n", path); return; }
    int fd = open(path, O_RDONLY | O_NONBLOCK);
    if (fd < 0) { printf("%s: open %s\n", path, strerror(errno)); return; }
    printf("%s: geoeffnet\n", path);
    int total = 0, nmea = 0;
    time_t end = time(NULL) + 3;
    unsigned char buf[4096];
    while (time(NULL) < end) {
        struct pollfd pfd = { fd, POLLIN, 0 };
        int pr = poll(&pfd, 1, 500);
        if (pr <= 0) continue;
        int n = read(fd, buf, sizeof buf);
        if (n <= 0) continue;
        total += n;
        if (total <= 512) hexdump(buf, n < 256 ? n : 256);
        for (int i = 0; i + 5 < n; i++)
            if (buf[i] == '$' && (buf[i+1]=='G')) { nmea++; break; }
    }
    printf("%s: %d Bytes gelesen, NMEA-Treffer~%d\n", path, total, nmea);
    close(fd);
}

static void run_with_timeout(const char *path) {
    pid_t pid = fork();
    if (pid == 0) { probe(path); fflush(stdout); _exit(0); }
    if (pid < 0) return;
    int waited = 0, status;
    while (waited < 4000) {
        if (waitpid(pid, &status, WNOHANG) == pid) return;
        struct timespec ts = { 0, 50 * 1000000 }; nanosleep(&ts, NULL);
        waited += 50;
    }
    printf("%s: (blockiert -> abgebrochen)\n", path);
    kill(pid, SIGKILL); waitpid(pid, &status, 0);
}

int main(int argc, char **argv) {
    setvbuf(stdout, NULL, _IONBF, 0);
    if (argc >= 2) { run_with_timeout(argv[1]); return 0; }
    printf("== GNSS-Kandidaten ==\n");
    for (int i = 0; cands[i]; i++) run_with_timeout(cands[i]);
    printf("== GNSS-Firmware suchen ==\n");
    const char *fw[] = { "/vendor/firmware", "/firmware", "/efs", NULL };
    for (int i = 0; fw[i]; i++) {
        char cmd[128];
        snprintf(cmd, sizeof cmd, "ls %s 2>/dev/null", fw[i]);
        /* nur Hinweis ausgeben; eigentliche Suche macht der Koordinator per Shell */
        printf("  pruefe %s (per Shell: ls %s | grep -iE 'gnss|kepler|gps')\n", fw[i], fw[i]);
    }
    return 0;
}
