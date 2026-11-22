#include <stdio.h>
#include <string.h>
#include <fcntl.h>
#include <unistd.h>
#include <errno.h>
#include <sys/ioctl.h>
#include <linux/videodev2.h>
static void t(int fd, unsigned long req, void *a, const char *n) { int r = ioctl(fd, req, a); printf("  %-26s -> %d %s\n", n, r, r < 0 ? strerror(errno) : ""); }
int main(int argc, char **argv) {
    alarm(15);
    int fd = open(argv[1], O_RDWR); printf("%s fd=%d %s\n", argv[1], fd, fd < 0 ? strerror(errno) : ""); if (fd < 0) return 1;
    struct v4l2_capability c; memset(&c, 0, sizeof c); t(fd, VIDIOC_QUERYCAP, &c, "QUERYCAP");
    printf("  treiber=%s karte=%s caps=%x dev=%x  sizeof(format)=%zu S_FMT=%lx\n", c.driver, c.card, c.capabilities, c.device_caps, sizeof(struct v4l2_format), (unsigned long)VIDIOC_S_FMT);
    if (argc > 2) { unsigned in = strtoul(argv[2], 0, 0); t(fd, VIDIOC_S_INPUT, &in, "S_INPUT"); }
    int types[] = { 1, 2, 9, 10 };
    for (int i = 0; i < 4; i++) {
        struct v4l2_format f; memset(&f, 0, sizeof f); f.type = types[i];
        char n[40]; snprintf(n, sizeof n, "G_FMT typ %d", types[i]); t(fd, VIDIOC_G_FMT, &f, n);
        f.fmt.pix_mp.width = 2304; f.fmt.pix_mp.height = 1728; f.fmt.pix_mp.pixelformat = V4L2_PIX_FMT_SBGGR10; f.fmt.pix_mp.num_planes = 2;
        snprintf(n, sizeof n, "S_FMT typ %d", types[i]); t(fd, VIDIOC_S_FMT, &f, n);
        snprintf(n, sizeof n, "TRY_FMT typ %d", types[i]); t(fd, VIDIOC_TRY_FMT, &f, n);
        struct v4l2_fmtdesc d; memset(&d, 0, sizeof d); d.type = types[i]; snprintf(n, sizeof n, "ENUM_FMT typ %d", types[i]); t(fd, VIDIOC_ENUM_FMT, &d, n);
        struct v4l2_requestbuffers rb; memset(&rb, 0, sizeof rb); rb.count = 0; rb.type = types[i]; rb.memory = V4L2_MEMORY_DMABUF; snprintf(n, sizeof n, "REQBUFS(0) typ %d", types[i]); t(fd, VIDIOC_REQBUFS, &rb, n);
    }
    struct v4l2_streamparm sp; memset(&sp, 0, sizeof sp); sp.type = 9; t(fd, VIDIOC_G_PARM, &sp, "G_PARM typ 9");
    struct v4l2_control ct = { 0x009a1000 + 17, 0 }; t(fd, VIDIOC_G_CTRL, &ct, "G_CTRL");
    return 0;
}
