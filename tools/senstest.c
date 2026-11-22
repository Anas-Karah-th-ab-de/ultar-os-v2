/* senstest – dumpt alle Sensoren aus /sys/class/sensors und /sys/bus/iio/devices.
 * Statisch aarch64. Zum Gerätetest über die Tür: zeigt Verzeichniseinträge + Werte der
 * üblichen Attribute, damit die echten Dateinamen/Formate sichtbar werden (zum Abgleich
 * mit sensors.rs). Liest nur, O_NONBLOCK, überspringt große/blockierende Dateien.
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <fcntl.h>
#include <unistd.h>
#include <dirent.h>

/* Datei kurz + nicht blockierend lesen; Ausgabe getrimmt. 0 wenn leer/nicht lesbar. */
static int catf(const char *dir, const char *file) {
    char path[512];
    snprintf(path, sizeof path, "%s/%s", dir, file);
    int fd = open(path, O_RDONLY | O_NONBLOCK);
    if (fd < 0) return 0;
    char buf[256];
    ssize_t n = read(fd, buf, sizeof buf - 1);
    close(fd);
    if (n <= 0) return 0;
    buf[n] = 0;
    /* Zeilenumbrüche am Ende weg */
    while (n > 0 && (buf[n-1] == '\n' || buf[n-1] == '\r' || buf[n-1] == ' ')) buf[--n] = 0;
    if (n == 0) return 0;
    printf("    %-22s = %s\n", file, buf);
    return 1;
}

static void dump_dir_listing(const char *dir) {
    DIR *d = opendir(dir);
    if (!d) return;
    struct dirent *e;
    int c = 0;
    printf("    [dateien]");
    while ((e = readdir(d))) {
        if (e->d_name[0] == '.') continue;
        printf(" %s", e->d_name);
        if (++c > 40) { printf(" ..."); break; }
    }
    printf("\n");
    closedir(d);
}

int main(void) {
    const char *cands[] = {
        "name", "vendor", "enable", "raw_data", "data", "value", "lux", "adc",
        "state", "prox_raw", "step_cnt", "step_count",
        "in_accel_x_raw", "in_accel_y_raw", "in_accel_z_raw", "in_accel_scale",
        "in_anglvel_x_raw", "in_anglvel_y_raw", "in_anglvel_z_raw",
        "in_magn_x_raw", "in_magn_y_raw", "in_magn_z_raw",
        "in_illuminance_input", "in_illuminance_raw", "in_proximity_raw", "in_pressure_input",
        NULL
    };

    printf("=== /sys/class/sensors ===\n");
    DIR *d = opendir("/sys/class/sensors");
    if (d) {
        struct dirent *e;
        while ((e = readdir(d))) {
            if (e->d_name[0] == '.') continue;
            char base[512];
            snprintf(base, sizeof base, "/sys/class/sensors/%s", e->d_name);
            printf("[%s]\n", e->d_name);
            dump_dir_listing(base);
            for (int i = 0; cands[i]; i++) catf(base, cands[i]);
        }
        closedir(d);
    } else {
        printf("(nicht vorhanden)\n");
    }

    printf("\n=== /sys/bus/iio/devices ===\n");
    d = opendir("/sys/bus/iio/devices");
    if (d) {
        struct dirent *e;
        while ((e = readdir(d))) {
            if (strncmp(e->d_name, "iio:device", 10) != 0) continue;
            char base[512];
            snprintf(base, sizeof base, "/sys/bus/iio/devices/%s", e->d_name);
            printf("[%s]\n", e->d_name);
            dump_dir_listing(base);
            for (int i = 0; cands[i]; i++) catf(base, cands[i]);
        }
        closedir(d);
    } else {
        printf("(nicht vorhanden)\n");
    }
    return 0;
}
