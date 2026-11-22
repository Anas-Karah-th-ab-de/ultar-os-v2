// fmtprobe – findet heraus, an welcher Stelle der Kern Breite/Höhe/Format im S_FMT-Block liest
#include <stdio.h>
#include <string.h>
#include <fcntl.h>
#include <unistd.h>
#include <errno.h>
#include <stdint.h>
#include <stdlib.h>
#include <sys/ioctl.h>
#include <linux/videodev2.h>
int main(int argc, char **argv) {
    int type = argc > 1 ? atoi(argv[1]) : 10;
    int L = open("/dev/video101", O_RDWR); if (L < 0) { perror("open"); return 1; }
    unsigned in = (0u << 26) | (0u << 16) | (1u << 4) | 1u;
    printf("S_INPUT %d\n", ioctl(L, VIDIOC_S_INPUT, &in));
    for (int p = 0; p < 12; p++) {
        struct v4l2_format f; memset(&f, 0, sizeof f);
        uint32_t *u = (uint32_t *)&f.fmt;
        for (int i = 0; i < 16; i++) u[i] = 1000 + i;
        u[p] = V4L2_PIX_FMT_SBGGR10;
        f.fmt.pix_mp.num_planes = 2;
        f.type = type;
        int r = ioctl(L, VIDIOC_S_FMT, &f);
        printf("pass %d -> %d %s\n", p, r, r < 0 ? strerror(errno) : "");
        int k = open("/dev/kmsg", O_WRONLY); if (k >= 0) { char b[64]; int n = snprintf(b, sizeof b, "fmtprobe pass %d r=%d\n", p, r); write(k, b, n); close(k); }
    }
    close(L); return 0;
}
