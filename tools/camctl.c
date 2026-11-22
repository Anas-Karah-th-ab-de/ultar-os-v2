/* camctl – v4l2 Kamera-Testwerkzeug für Ultar (Exynos FIMC-IS). Statisch aarch64.
 * Modi:
 *   camctl list                 – alle /dev/video* mit QUERYCAP-Name, caps und Formaten
 *   camctl grab <dev> <out.ppm> – einen Frame von /dev/video<dev> holen, als PPM (P6) speichern
 *   camctl grab <pfad> <out.ppm>– wenn <dev> mit '/' beginnt, wird es als voller Pfad genutzt
 * Unterstützt Single-Plane und MPLANE (Exynos nutzt meist MPLANE). Formate: YUYV, NV21, NV12, RGB24.
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <fcntl.h>
#include <unistd.h>
#include <errno.h>
#include <poll.h>
#include <sys/wait.h>
#include <signal.h>
#include <time.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <linux/videodev2.h>

static int xioctl(int fd, unsigned long req, void *arg) {
    int r;
    do { r = ioctl(fd, req, arg); } while (r < 0 && errno == EINTR);
    return r;
}

static void fourcc_str(unsigned int f, char out[5]) {
    out[0] = f & 0xff; out[1] = (f >> 8) & 0xff; out[2] = (f >> 16) & 0xff; out[3] = (f >> 24) & 0xff; out[4] = 0;
}

/* ---- Formate auflisten ---- */
static void enum_fmts(int fd, unsigned int type) {
    struct v4l2_fmtdesc fd_;
    for (unsigned int i = 0;; i++) {
        memset(&fd_, 0, sizeof fd_);
        fd_.index = i; fd_.type = type;
        if (xioctl(fd, VIDIOC_ENUM_FMT, &fd_) < 0) break;
        char fc[5]; fourcc_str(fd_.pixelformat, fc);
        printf("    fmt[%u] %s  %s\n", i, fc, fd_.description);
    }
}

/* Einen Node prüfen (im Kindprozess aufgerufen). */
static void probe_one(int i) {
    char path[64];
    snprintf(path, sizeof path, "/dev/video%d", i);
    int fd = open(path, O_RDWR | O_NONBLOCK);
    if (fd < 0) return;
    struct v4l2_capability cap; memset(&cap, 0, sizeof cap);
    if (xioctl(fd, VIDIOC_QUERYCAP, &cap) == 0) {
        unsigned int c = cap.device_caps ? cap.device_caps : cap.capabilities;
        int cap_sp = !!(c & V4L2_CAP_VIDEO_CAPTURE);
        int cap_mp = !!(c & V4L2_CAP_VIDEO_CAPTURE_MPLANE);
        int strm   = !!(c & V4L2_CAP_STREAMING);
        printf("video%d: %s | %s | caps=0x%08x%s%s%s\n", i, cap.driver, cap.card, c,
               cap_sp ? " CAPTURE" : "", cap_mp ? " CAPTURE_MPLANE" : "", strm ? " STREAMING" : "");
        if (cap_sp) enum_fmts(fd, V4L2_BUF_TYPE_VIDEO_CAPTURE);
        if (cap_mp) enum_fmts(fd, V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE);
    }
    close(fd);
}

/* Jeden Node in einem Kindprozess mit 2-s-Timeout prüfen, damit ein blockierender
 * Node (Exynos ISP ignoriert O_NONBLOCK beim open) nie den ganzen Lauf/Tür aufhängt. */
static int do_list(void) {
    for (int i = 0; i < 260; i++) {
        fflush(stdout);
        pid_t pid = fork();
        if (pid == 0) { probe_one(i); fflush(stdout); _exit(0); }
        if (pid < 0) continue;
        int waited = 0, status;
        while (waited < 2000) {
            if (waitpid(pid, &status, WNOHANG) == pid) { pid = 0; break; }
            struct timespec ts = { 0, 50 * 1000000 }; nanosleep(&ts, NULL);
            waited += 50;
        }
        if (pid > 0) { printf("video%d: (blockiert beim Oeffnen -> uebersprungen)\n", i); kill(pid, SIGKILL); waitpid(pid, &status, 0); }
    }
    return 0;
}

/* ---- YUV/RGB-Wandlung ---- */
static inline unsigned char clamp8(int v) { return v < 0 ? 0 : v > 255 ? 255 : v; }
static void yuv2rgb(int y, int u, int v, unsigned char *p) {
    int c = y - 16, d = u - 128, e = v - 128;
    p[0] = clamp8((298 * c + 409 * e + 128) >> 8);
    p[1] = clamp8((298 * c - 100 * d - 208 * e + 128) >> 8);
    p[2] = clamp8((298 * c + 516 * d + 128) >> 8);
}

/* YUYV (packed) -> RGB */
static void yuyv_to_rgb(const unsigned char *src, int w, int h, int stride, unsigned char *rgb) {
    for (int y = 0; y < h; y++) {
        const unsigned char *l = src + y * stride;
        unsigned char *o = rgb + y * w * 3;
        for (int x = 0; x < w; x += 2) {
            int y0 = l[0], u = l[1], y1 = l[2], v = l[3]; l += 4;
            yuv2rgb(y0, u, v, o); o += 3;
            yuv2rgb(y1, u, v, o); o += 3;
        }
    }
}

/* NV21 (Y plane + interleaved VU) / NV12 (UV) -> RGB. nv21: V zuerst */
static void nv_to_rgb(const unsigned char *src, int w, int h, int ystride, int nv21, unsigned char *rgb) {
    const unsigned char *yp = src;
    const unsigned char *cp = src + ystride * h;
    for (int y = 0; y < h; y++) {
        unsigned char *o = rgb + y * w * 3;
        const unsigned char *yl = yp + y * ystride;
        const unsigned char *cl = cp + (y / 2) * ystride;
        for (int x = 0; x < w; x++) {
            int yy = yl[x];
            int ci = (x / 2) * 2;
            int u, v;
            if (nv21) { v = cl[ci]; u = cl[ci + 1]; }
            else      { u = cl[ci]; v = cl[ci + 1]; }
            yuv2rgb(yy, u, v, o); o += 3;
        }
    }
}

/* ---- Frame holen ---- */
static int do_grab(const char *devarg, const char *outpath) {
    char path[128];
    if (devarg[0] == '/') snprintf(path, sizeof path, "%s", devarg);
    else snprintf(path, sizeof path, "/dev/video%s", devarg);

    int fd = open(path, O_RDWR | O_NONBLOCK);
    if (fd < 0) { fprintf(stderr, "%s: %s\n", path, strerror(errno)); return 1; }

    struct v4l2_capability cap; memset(&cap, 0, sizeof cap);
    if (xioctl(fd, VIDIOC_QUERYCAP, &cap) < 0) { perror("QUERYCAP"); return 1; }
    unsigned int c = cap.device_caps ? cap.device_caps : cap.capabilities;
    int mp = (c & V4L2_CAP_VIDEO_CAPTURE_MPLANE) && !(c & V4L2_CAP_VIDEO_CAPTURE);
    unsigned int btype = mp ? V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE : V4L2_BUF_TYPE_VIDEO_CAPTURE;
    printf("grab %s: %s/%s %s\n", path, cap.driver, cap.card, mp ? "MPLANE" : "single");

    /* aktuelles Format lesen, dann bevorzugtes setzen */
    struct v4l2_format fmt; memset(&fmt, 0, sizeof fmt); fmt.type = btype;
    if (xioctl(fd, VIDIOC_G_FMT, &fmt) < 0) { perror("G_FMT"); /* weiter mit Defaults */ }

    int w, h; unsigned int pf;
    if (mp) {
        if (fmt.fmt.pix_mp.width == 0) { fmt.fmt.pix_mp.width = 640; fmt.fmt.pix_mp.height = 480; }
        fmt.fmt.pix_mp.pixelformat = V4L2_PIX_FMT_NV21;
        fmt.fmt.pix_mp.field = V4L2_FIELD_ANY;
    } else {
        if (fmt.fmt.pix.width == 0) { fmt.fmt.pix.width = 640; fmt.fmt.pix.height = 480; }
        fmt.fmt.pix.pixelformat = V4L2_PIX_FMT_YUYV;
        fmt.fmt.pix.field = V4L2_FIELD_ANY;
    }
    if (xioctl(fd, VIDIOC_S_FMT, &fmt) < 0) { perror("S_FMT"); /* Treiber kann Default liefern */ }

    int stride;
    if (mp) {
        w = fmt.fmt.pix_mp.width; h = fmt.fmt.pix_mp.height; pf = fmt.fmt.pix_mp.pixelformat;
        stride = fmt.fmt.pix_mp.plane_fmt[0].bytesperline; if (!stride) stride = w;
    } else {
        w = fmt.fmt.pix.width; h = fmt.fmt.pix.height; pf = fmt.fmt.pix.pixelformat;
        stride = fmt.fmt.pix.bytesperline; if (!stride) stride = (pf == V4L2_PIX_FMT_YUYV) ? w * 2 : w;
    }
    char fc[5]; fourcc_str(pf, fc);
    printf("format: %dx%d %s stride=%d\n", w, h, fc, stride);

    /* Puffer anfordern (MMAP) */
    struct v4l2_requestbuffers req; memset(&req, 0, sizeof req);
    req.count = 4; req.type = btype; req.memory = V4L2_MEMORY_MMAP;
    if (xioctl(fd, VIDIOC_REQBUFS, &req) < 0) { perror("REQBUFS"); return 1; }
    if (req.count < 1) { fprintf(stderr, "keine Puffer\n"); return 1; }

    void *bufs[8]; unsigned int blen[8];
    for (unsigned int i = 0; i < req.count; i++) {
        struct v4l2_buffer b; memset(&b, 0, sizeof b);
        struct v4l2_plane planes[VIDEO_MAX_PLANES]; memset(planes, 0, sizeof planes);
        b.type = btype; b.memory = V4L2_MEMORY_MMAP; b.index = i;
        if (mp) { b.m.planes = planes; b.length = VIDEO_MAX_PLANES; }
        if (xioctl(fd, VIDIOC_QUERYBUF, &b) < 0) { perror("QUERYBUF"); return 1; }
        unsigned int len; unsigned int off;
        if (mp) { len = b.m.planes[0].length; off = b.m.planes[0].m.mem_offset; }
        else    { len = b.length; off = b.m.offset; }
        bufs[i] = mmap(NULL, len, PROT_READ | PROT_WRITE, MAP_SHARED, fd, off);
        if (bufs[i] == MAP_FAILED) { perror("mmap"); return 1; }
        blen[i] = len;
        if (xioctl(fd, VIDIOC_QBUF, &b) < 0) { perror("QBUF"); return 1; }
    }

    if (xioctl(fd, VIDIOC_STREAMON, &btype) < 0) { perror("STREAMON"); return 1; }

    /* ein paar Frames verwerfen (Auto-Belichtung), dann einen nehmen */
    unsigned char *frame = NULL; unsigned int flen = 0; int got = 0;
    for (int tries = 0; tries < 30 && !got; tries++) {
        struct pollfd pfd = { fd, POLLIN, 0 };
        int pr = poll(&pfd, 1, 2000);
        if (pr <= 0) { if (tries > 3) break; else continue; }
        struct v4l2_buffer b; memset(&b, 0, sizeof b);
        struct v4l2_plane planes[VIDEO_MAX_PLANES]; memset(planes, 0, sizeof planes);
        b.type = btype; b.memory = V4L2_MEMORY_MMAP;
        if (mp) { b.m.planes = planes; b.length = VIDEO_MAX_PLANES; }
        if (xioctl(fd, VIDIOC_DQBUF, &b) < 0) { if (errno == EAGAIN) continue; perror("DQBUF"); break; }
        if (tries >= 5) {
            flen = blen[b.index];
            frame = malloc(flen);
            memcpy(frame, bufs[b.index], flen);
            got = 1;
        }
        xioctl(fd, VIDIOC_QBUF, &b);
    }
    xioctl(fd, VIDIOC_STREAMOFF, &btype);
    for (unsigned int i = 0; i < req.count; i++) munmap(bufs[i], blen[i]);
    close(fd);

    if (!got) { fprintf(stderr, "kein Frame bekommen\n"); return 2; }

    unsigned char *rgb = malloc((size_t)w * h * 3);
    if (pf == V4L2_PIX_FMT_YUYV) yuyv_to_rgb(frame, w, h, stride, rgb);
    else if (pf == V4L2_PIX_FMT_NV21) nv_to_rgb(frame, w, h, stride, 1, rgb);
    else if (pf == V4L2_PIX_FMT_NV12) nv_to_rgb(frame, w, h, stride, 0, rgb);
    else if (pf == V4L2_PIX_FMT_RGB24) memcpy(rgb, frame, (size_t)w * h * 3);
    else { fprintf(stderr, "Format %s nicht unterstuetzt, speichere Rohdaten\n", fc);
           FILE *f = fopen(outpath, "wb"); if (f){ fwrite(frame, 1, flen, f); fclose(f);} return 3; }

    FILE *f = fopen(outpath, "wb");
    if (!f) { perror(outpath); return 1; }
    fprintf(f, "P6\n%d %d\n255\n", w, h);
    fwrite(rgb, 1, (size_t)w * h * 3, f);
    fclose(f);
    printf("gespeichert: %s (%dx%d PPM)\n", outpath, w, h);
    free(frame); free(rgb);
    return 0;
}

int main(int argc, char **argv) {
    const char *mode = argc > 1 ? argv[1] : "list";
    if (!strcmp(mode, "list")) return do_list();
    if (!strcmp(mode, "grab")) {
        if (argc < 4) { fprintf(stderr, "grab <dev|/pfad> <out.ppm>\n"); return 1; }
        return do_grab(argv[2], argv[3]);
    }
    fprintf(stderr, "Modi: list | grab <dev> <out.ppm>\n");
    return 1;
}
