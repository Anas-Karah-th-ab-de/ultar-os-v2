// camraw – Rohbild (Bayer) direkt vom Kamerasensor des A30 holen, ohne Samsungs Bildprozessor.
// Weg: Exynos fimc-is „Sensor allein" (SENSOR_SCENARIO_EXTERNAL): Leitknoten ssN (Aufträge mit Metadaten)
// + Aufnahmeknoten ssNvc0 (Bayer per CSI-DMA in ION-Puffer).
// Aufruf: camraw <pos 0=hinten 1=vorn 2=weit> <breite> <höhe> <ausgabe.raw> [ss-index] [format 10|12|16] [szenario]
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <fcntl.h>
#include <unistd.h>
#include <errno.h>
#include <poll.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <linux/videodev2.h>
#define _LINUX_TYPES_H
#include <stdint.h>
#include "fimc-is-metadata.h"

#define V4L2_CID_FIMC_IS_BASE (V4L2_CTRL_CLASS_CAMERA | 0x1000)
#define V4L2_CID_IS_S_STREAM (V4L2_CID_FIMC_IS_BASE + 14)
#define SPARE (64 * 1024)
#define NBUF 4

struct ion_alloc { uint64_t len, align; uint32_t heap_id_mask, flags; int32_t handle; };
struct ion_fd { int32_t handle, fd; };
static int ion = -1;
static int ion_buf(size_t len, void **map) {
    struct ion_alloc a = { len, 0, 1, 0, 0 };
    if (ioctl(ion, _IOWR('I', 0, struct ion_alloc), &a) < 0) { perror("ION_ALLOC"); return -1; }
    struct ion_fd f = { a.handle, -1 };
    if (ioctl(ion, _IOWR('I', 4, struct ion_fd), &f) < 0) { perror("ION_SHARE"); return -1; }
    *map = mmap(0, len, PROT_READ | PROT_WRITE, MAP_SHARED, f.fd, 0);
    if (*map == MAP_FAILED) { perror("mmap"); return -1; }
    memset(*map, 0, len);
    return f.fd;
}
static int xi(int fd, unsigned long req, void *arg, const char *name) {
    int r = ioctl(fd, req, arg);
    fprintf(stderr, "%-28s -> %d%s%s\n", name, r, r < 0 ? " " : "", r < 0 ? strerror(errno) : "");
    fflush(stderr); if (getenv("CAMSYNC")) sync();
    return r;
}
static int qbuf(int fd, int type, int idx, int fd0, size_t l0, int fd1) {
    struct v4l2_plane pl[2]; struct v4l2_buffer b;
    memset(pl, 0, sizeof pl); memset(&b, 0, sizeof b);
    b.type = type; b.memory = V4L2_MEMORY_DMABUF; b.index = idx; b.m.planes = pl; b.length = 2;
    pl[0].m.fd = fd0; pl[0].length = l0; pl[1].m.fd = fd1; pl[1].length = SPARE;
    int r = ioctl(fd, VIDIOC_QBUF, &b);
    if (r < 0) fprintf(stderr, "QBUF typ %d idx %d -> %s\n", type, idx, strerror(errno));
    return r;
}

_Static_assert(sizeof(struct camera2_shot_ext) == 38368, "shot_ext muss 38368 Bytes haben (CAPTURE_NODE_MAX 12)");
int main(int argc, char **argv) {
    if (argc < 5) { fprintf(stderr, "camraw <pos> <b> <h> <aus.raw> [ss] [10|12|16] [szenario]\n"); return 2; }
    int pos = atoi(argv[1]), w = atoi(argv[2]), h = atoi(argv[3]);
    int ltype = argc > 8 && atoi(argv[8]) == 2 ? V4L2_BUF_TYPE_VIDEO_OUTPUT_MPLANE : V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE;
    int usec = argc > 9 ? atoi(argv[9]) : 1;
    int ss = argc > 5 ? atoi(argv[5]) : 0, bits = argc > 6 ? atoi(argv[6]) : 10, scn = argc > 7 ? atoi(argv[7]) : 2;
    uint32_t fmt = bits == 16 ? V4L2_PIX_FMT_SBGGR16 : bits == 12 ? V4L2_PIX_FMT_SBGGR12 : V4L2_PIX_FMT_SBGGR10;
    size_t stride = bits == 16 ? (size_t)w * 2 : bits == 12 ? (((size_t)w * 3 / 2) + 15) & ~15 : (((size_t)w * 5 / 4) + 15) & ~15;
    size_t isz = stride * h;
    size_t sst = (((size_t)w * 3 / 2) + 15) & ~(size_t)15, ssz = sst * h;   /* CSI-DMA liefert 12 Bit gepackt (2 Pixel in 3 Bytes) */
    size_t asz = (size_t)w * h * 2 + 65536;   /* Reserve: CSI-DMA schreibt ueber das gepackte Mass hinaus */
    char pl[64], pc[64];
    int lvid = 101 + ss, cvid = 210 + ss * 4;
    snprintf(pl, sizeof pl, "/dev/video%d", lvid); snprintf(pc, sizeof pc, "/dev/video%d", cvid);
    alarm(25);
    if (getenv("CAMSYNC")) { int lf = open("/data/ultar/cam.log", O_WRONLY | O_APPEND | O_CREAT | O_SYNC, 0644); if (lf >= 0) dup2(lf, 2); }
    ion = open("/dev/ion", O_RDWR); if (ion < 0) { perror("/dev/ion"); return 1; }
    int L = open(pl, O_RDWR); fprintf(stderr, "open %s -> %d %s\n", pl, L, L < 0 ? strerror(errno) : "");
    if (L < 0) return 1;
    int C = open(pc, O_RDWR); fprintf(stderr, "open %s -> %d %s\n", pc, C, C < 0 ? strerror(errno) : "");
    if (C < 0) return 1;

    int A = -1, P = -1, I = -1, M = -1, V = -1;
    if (getenv("CAM3AA")) {
        A = open("/dev/video110", O_RDWR); fprintf(stderr, "open 30s -> %d %s\n", A, A < 0 ? strerror(errno) : "");
        if (A < 0) return 1;
        P = open("/dev/video112", O_RDWR); I = open("/dev/video130", O_RDWR);
        M = open("/dev/video160", O_RDWR); V = open("/dev/video180", O_RDWR);
        fprintf(stderr, "open 30p -> %d, i0s -> %d, m0s -> %d, vra -> %d\n", P, I, M, V); if (P < 0 || I < 0 || M < 0 || V < 0) return 1;
        struct v4l2_control eos = { V4L2_CID_FIMC_IS_BASE + 58, 1 };   /* V4L2_CID_IS_END_OF_STREAM: Bildprozessor wirklich oeffnen */
        if (xi(A, VIDIOC_S_CTRL, &eos, "S_CTRL Ende-der-Oeffnung") < 0) return 1;
    }
    unsigned in = ((unsigned)scn << 26) | ((unsigned)pos << 16) | ((unsigned)(lvid - 100) << 8) | (1u << 4) | 1u;   /* vindex != 0, sonst wird die Sensorgruppe nicht verkettet */
    if (xi(L, VIDIOC_S_INPUT, &in, "S_INPUT Leitknoten") < 0) return 1;
    unsigned inc = ((unsigned)scn << 26) | ((unsigned)pos << 16) | ((unsigned)(lvid - 100) << 8) | (1u << 4);
    xi(C, VIDIOC_S_INPUT, &inc, "S_INPUT Aufnahme");

    if (A >= 0) {
        unsigned in3 = (0u << 24) | ((unsigned)pos << 16) | ((unsigned)(lvid - 100) << 8) | (1u << 4) | 0u;
        if (xi(A, VIDIOC_S_INPUT, &in3, "S_INPUT 30s (OTF)") < 0) return 1;
        unsigned inp = (0u << 24) | ((unsigned)pos << 16) | (10u << 8) | (1u << 4) | 0u;
        xi(P, VIDIOC_S_INPUT, &inp, "S_INPUT 30p");
        unsigned ini = (0u << 24) | ((unsigned)pos << 16) | (12u << 8) | (0u << 4) | 0u;   /* Quelle = 30p (12), Speicher (M2M): 3AA braucht einen Nachfolger, ISP bleibt aber aus */
        if (xi(I, VIDIOC_S_INPUT, &ini, "S_INPUT i0s (M2M)") < 0) return 1;
        /* Skalierer (MCSC) und Gesichtserkennung (VRA) muessen mit geoeffnet sein:
         * - ohne VRA ruft die Bibliothek nach dem ersten Bild eine VRA-Funktion ohne Kontext auf -> Absturz
         * - ohne MCSC verrutscht die Zuordnung der Einstellungsbloecke -> VRA bekommt falsche Groesse -> Absturz */
        unsigned inm = (0u << 24) | ((unsigned)pos << 16) | (30u << 8) | (1u << 4) | 0u;
        if (xi(M, VIDIOC_S_INPUT, &inm, "S_INPUT m0s (OTF)") < 0) return 1;
        unsigned inv = (0u << 24) | ((unsigned)pos << 16) | (60u << 8) | (1u << 4) | 0u;
        if (xi(V, VIDIOC_S_INPUT, &inv, "S_INPUT vra (OTF)") < 0) return 1;
    }
    struct v4l2_control sz = { V4L2_CID_FIMC_IS_BASE + 69, (w << 16) | h };   /* V4L2_CID_IS_S_SENSOR_SIZE */
    xi(L, VIDIOC_S_CTRL, &sz, "S_CTRL Sensorgroesse");
    struct v4l2_format f; memset(&f, 0, sizeof f);
    f.type = ltype;
    f.fmt.pix_mp.width = w; f.fmt.pix_mp.height = h; f.fmt.pix_mp.pixelformat = fmt; f.fmt.pix_mp.num_planes = 2;
    if (xi(L, VIDIOC_S_FMT, &f, "S_FMT Leitknoten") < 0) return 1;
    struct v4l2_streamparm sp; memset(&sp, 0, sizeof sp);
    sp.type = V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE; sp.parm.capture.timeperframe.numerator = 1; sp.parm.capture.timeperframe.denominator = 30;
    xi(L, VIDIOC_S_PARM, &sp, "S_PARM 30 B/s");
    f.type = V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE;
    if (usec && xi(C, VIDIOC_S_FMT, &f, "S_FMT Aufnahme") < 0) return 1;

    if (A >= 0) {
        struct v4l2_format fa = f; fa.type = V4L2_BUF_TYPE_VIDEO_OUTPUT_MPLANE;
        if (xi(A, VIDIOC_S_FMT, &fa, "S_FMT 30s") < 0) return 1;
        struct v4l2_requestbuffers ra; memset(&ra, 0, sizeof ra);
        ra.count = NBUF; ra.memory = V4L2_MEMORY_DMABUF; ra.type = V4L2_BUF_TYPE_VIDEO_OUTPUT_MPLANE;
        if (xi(A, VIDIOC_REQBUFS, &ra, "REQBUFS 30s") < 0) return 1;
        struct v4l2_format fp = f; fp.type = V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE; fp.fmt.pix_mp.pixelformat = V4L2_PIX_FMT_SBGGR12;
        if (xi(P, VIDIOC_S_FMT, &fp, "S_FMT 30p") < 0) return 1;
        struct v4l2_format fi = f; fi.type = V4L2_BUF_TYPE_VIDEO_OUTPUT_MPLANE; fi.fmt.pix_mp.pixelformat = V4L2_PIX_FMT_SBGGR12;
        if (xi(I, VIDIOC_S_FMT, &fi, "S_FMT i0s") < 0) return 1;
        if (xi(I, VIDIOC_REQBUFS, &ra, "REQBUFS i0s") < 0) return 1;
        struct v4l2_format fm = f; fm.type = V4L2_BUF_TYPE_VIDEO_OUTPUT_MPLANE; fm.fmt.pix_mp.pixelformat = V4L2_PIX_FMT_YUYV;
        if (xi(M, VIDIOC_S_FMT, &fm, "S_FMT m0s") < 0) return 1;
        if (xi(M, VIDIOC_REQBUFS, &ra, "REQBUFS m0s") < 0) return 1;
        struct v4l2_format fv; memset(&fv, 0, sizeof fv); fv.type = V4L2_BUF_TYPE_VIDEO_OUTPUT_MPLANE;
        fv.fmt.pix_mp.width = 640; fv.fmt.pix_mp.height = 480; fv.fmt.pix_mp.pixelformat = V4L2_PIX_FMT_NV21; fv.fmt.pix_mp.num_planes = 2;
        if (xi(V, VIDIOC_S_FMT, &fv, "S_FMT vra") < 0) return 1;
        if (xi(V, VIDIOC_REQBUFS, &ra, "REQBUFS vra") < 0) return 1;
        int tv = V4L2_BUF_TYPE_VIDEO_OUTPUT_MPLANE;
        if (xi(V, VIDIOC_STREAMON, &tv, "STREAMON vra") < 0) return 1;
        if (xi(M, VIDIOC_STREAMON, &tv, "STREAMON m0s") < 0) return 1;
        int ta = V4L2_BUF_TYPE_VIDEO_OUTPUT_MPLANE;
        if (xi(I, VIDIOC_STREAMON, &ta, "STREAMON i0s") < 0) return 1;   /* Gruppe muss gestartet sein, bekommt aber keine Bilder */
        if (xi(A, VIDIOC_STREAMON, &ta, "STREAMON 30s") < 0) return 1;
    }
    struct v4l2_requestbuffers rb; memset(&rb, 0, sizeof rb);
    rb.count = NBUF; rb.memory = V4L2_MEMORY_DMABUF; rb.type = V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE;
    if (usec && xi(C, VIDIOC_REQBUFS, &rb, "REQBUFS Aufnahme") < 0) return 1;
    rb.count = NBUF; rb.type = ltype;
    if (xi(L, VIDIOC_REQBUFS, &rb, "REQBUFS Leitknoten") < 0) return 1;

    void *cimg[NBUF], *cmeta[NBUF], *limg[NBUF], *lmeta[NBUF];
    int cfd[NBUF], cmfd[NBUF], lfd[NBUF], lmfd[NBUF];
    for (int i = 0; i < NBUF; i++) {
        cfd[i] = ion_buf(asz, &cimg[i]); cmfd[i] = ion_buf(SPARE, &cmeta[i]); lmfd[i] = ion_buf(SPARE, &lmeta[i]); lfd[i] = ion_buf(asz, &limg[i]);
        if (cfd[i] < 0 || cmfd[i] < 0 || lmfd[i] < 0 || lfd[i] < 0) return 1;
    }
    for (int i = 0; usec && i < NBUF; i++) if (qbuf(C, V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE, i, cfd[i], asz, cmfd[i]) < 0) return 1;
    int t = V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE;
    if (usec && xi(C, VIDIOC_STREAMON, &t, "STREAMON Aufnahme") < 0) return 1;

    for (int i = 0; i < NBUF; i++) {
        struct camera2_shot_ext *se = lmeta[i];
        se->shot.magicNumber = SHOT_MAGIC_NUMBER;   /* Stock-Kern: CAPTURE_NODE_MAX 12 -> shot_ext 38368 Bytes */
        se->shot.ctl.aa.mode = AA_CONTROL_AUTO; se->shot.ctl.aa.aeMode = AA_AEMODE_CENTER;
        se->shot.ctl.aa.awbMode = AA_AWBMODE_WB_AUTO; se->shot.ctl.aa.afMode = AA_AFMODE_OFF;
        se->shot.ctl.aa.aeLock = AA_AE_LOCK_OFF; se->shot.ctl.aa.awbLock = AA_AWB_LOCK_OFF;
        se->shot.ctl.aa.captureIntent = AA_CAPTURE_INTENT_PREVIEW; se->shot.ctl.aa.sceneMode = AA_SCENE_MODE_DISABLED;
        se->shot.ctl.aa.aeAntibandingMode = AA_AE_ANTIBANDING_AUTO; se->shot.ctl.aa.vendor_isoMode = AA_ISOMODE_AUTO;
        se->shot.ctl.request.frameCount = i + 1;
        se->shot.dm.request.frameCount = i + 1;
        se->shot.ctl.edge.mode = PROCESSING_MODE_FAST; se->shot.ctl.noise.mode = PROCESSING_MODE_FAST;
        se->shot.ctl.hotpixel.mode = PROCESSING_MODE_FAST; se->shot.ctl.shading.mode = PROCESSING_MODE_FAST;
        se->shot.ctl.color.mode = COLORCORRECTION_MODE_FAST; se->shot.ctl.stats.faceDetectMode = FACEDETECT_MODE_OFF;
        se->shot.ctl.aa.aeTargetFpsRange[0] = 15; se->shot.ctl.aa.aeTargetFpsRange[1] = 30;
        se->shot.ctl.sensor.frameDuration = 33333333ULL;
        se->shot.ctl.sensor.exposureTime = 20000000ULL;
        se->shot.ctl.sensor.sensitivity = 400;
        se->node_group.leader.vid = lvid - 100; se->node_group.leader.request = 1; se->node_group.leader.pixelformat = fmt;
        se->node_group.leader.input.cropRegion[2] = w; se->node_group.leader.input.cropRegion[3] = h;
        se->node_group.leader.output.cropRegion[2] = w; se->node_group.leader.output.cropRegion[3] = h;
        se->node_group.capture[0].vid = usec ? cvid - 100 : 0; se->node_group.capture[0].request = usec; se->node_group.capture[0].pixelformat = fmt;
        se->node_group.capture[0].input.cropRegion[2] = w; se->node_group.capture[0].input.cropRegion[3] = h;
        se->node_group.capture[0].output.cropRegion[2] = w; se->node_group.capture[0].output.cropRegion[3] = h;
        if (A >= 0) { se->node_group.capture[1].vid = 12; se->node_group.capture[1].request = 0; se->node_group.capture[1].pixelformat = V4L2_PIX_FMT_SBGGR12;
            se->node_group.capture[1].input.cropRegion[2] = w; se->node_group.capture[1].input.cropRegion[3] = h;
            se->node_group.capture[1].output.cropRegion[2] = w; se->node_group.capture[1].output.cropRegion[3] = h; }
        if (getenv("CAMPAT")) { uint32_t *u = lmeta[i]; for (unsigned k = 0; k < SPARE / 4; k++) u[k] = k * 4; }
        if (qbuf(L, ltype, i, lfd[i], asz, lmfd[i]) < 0) return 1;
    }
    t = ltype;
    if (xi(L, VIDIOC_STREAMON, &t, "STREAMON Leitknoten") < 0) return 1;
    if (getenv("CAMPAT")) { usleep(300000); xi(L, VIDIOC_STREAMOFF, &t, "STREAMOFF (Mustertest)"); return 0; }
    struct v4l2_control c = { V4L2_CID_IS_S_STREAM, 1 };
    if (xi(L, VIDIOC_S_CTRL, &c, "S_STREAM an") < 0) return 1;

    int got = 0, fc = NBUF + 1;
    for (int n = 0; n < 40 && got < 6; n++) {
        struct pollfd p[2] = { { usec ? C : -1, POLLIN, 0 }, { L, POLLOUT | POLLIN, 0 } };
        if (poll(p, 2, 3000) <= 0) { fprintf(stderr, "kein Bild (3 s)\n"); break; }
        fprintf(stderr, "poll: Aufnahme 0x%x Leit 0x%x\n", p[0].revents, p[1].revents);
        struct v4l2_plane pln[2]; struct v4l2_buffer b;
        if (p[0].revents & (POLLIN | POLLERR)) {
            memset(pln, 0, sizeof pln); memset(&b, 0, sizeof b);
            b.type = V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE; b.memory = V4L2_MEMORY_DMABUF; b.m.planes = pln; b.length = 2;
            if (ioctl(C, VIDIOC_DQBUF, &b) == 0) {
                struct camera2_stream *st = cmeta[b.index];
                unsigned long sum = 0; unsigned char *d = cimg[b.index];
                for (size_t k = 0; k < isz; k += 997) sum += d[k];
                unsigned long nz = 0; for (size_t k = isz; k < asz; k += 4099) nz += d[k] != 0;
                fprintf(stderr, "Bild %d: Puffer %d fcount %u fvalid %u flags 0x%x Mittel %lu, jenseits gepackt ungleich 0: %lu, bytesused %u\n", got, b.index, st->fcount, st->fvalid, b.flags, sum / (isz / 997 + 1), nz, pln[0].bytesused);
                got++;
                if (got == 1 || got == 4) {   /* sofort gestuft sichern: der Kern kann kurz danach abstuerzen */
                    char nm[160]; snprintf(nm, sizeof nm, "%s.klein%d", argv[4], got);
                    int o = open(nm, O_WRONLY | O_CREAT | O_TRUNC, 0644);
                    if (o >= 0) { for (int r = 0; r + 1 < h; r += 16) write(o, d + (size_t)r * sst, sst * 2); fsync(o); close(o); }
                    snprintf(nm, sizeof nm, "%s.ende%d", argv[4], got);
                    o = open(nm, O_WRONLY | O_CREAT | O_TRUNC, 0644);
                    if (o >= 0) { write(o, d + ssz - 131072, 262144); fsync(o); close(o); }
                    fprintf(stderr, "gesichert (klein, %d Zeilenpaare zu je 2x%zu Bytes)\n", h / 16, sst); sync();
                }
                if (got == 5) { int o = open(argv[4], O_WRONLY | O_CREAT | O_TRUNC, 0644); if (o >= 0) { write(o, d, ssz); fsync(o); close(o); fprintf(stderr, "gespeichert: %s (%zu Bytes, %dx%d, Zeile %zu)\n", argv[4], ssz, w, h, sst); sync(); } }
                qbuf(C, V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE, b.index, cfd[b.index], asz, cmfd[b.index]);
            } else fprintf(stderr, "DQBUF Aufnahme: %s\n", strerror(errno));
        }
        if (p[1].revents & (POLLOUT | POLLIN | POLLERR)) {
            memset(pln, 0, sizeof pln); memset(&b, 0, sizeof b);
            b.type = ltype; b.memory = V4L2_MEMORY_DMABUF; b.m.planes = pln; b.length = 2;
            if (ioctl(L, VIDIOC_DQBUF, &b) == 0) {
                struct camera2_shot_ext *se = lmeta[b.index];
                if (!usec) {
                    unsigned long sum = 0; unsigned char *d = limg[b.index];
                    for (size_t k = 0; k < isz; k += 997) sum += d[k];
                    fprintf(stderr, "Leitbild %d: Puffer %d flags 0x%x Mittel %lu fcount %u\n", got, b.index, b.flags, sum / (isz / 997 + 1), se->shot.dm.request.frameCount);
                    got++;
                    if (got == 5) { int o = open(argv[4], O_WRONLY | O_CREAT | O_TRUNC, 0644); if (o >= 0) { write(o, d, isz); close(o); fprintf(stderr, "gespeichert: %s (%zu Bytes)\n", argv[4], isz); } }
                }
                se->shot.ctl.request.frameCount = fc; se->shot.dm.request.frameCount = fc; fc++;
                se->node_group.leader.request = 1; se->node_group.capture[0].request = 1;
                se->node_group.capture[0].request = usec; qbuf(L, ltype, b.index, lfd[b.index], asz, lmfd[b.index]);
            } else fprintf(stderr, "DQBUF Leitknoten: %s\n", strerror(errno));
        }
    }
    c.value = 0; xi(L, VIDIOC_S_CTRL, &c, "S_STREAM aus");
    t = ltype; xi(L, VIDIOC_STREAMOFF, &t, "STREAMOFF Leitknoten");
    t = V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE; if (usec) xi(C, VIDIOC_STREAMOFF, &t, "STREAMOFF Aufnahme");
    close(C); close(L);
    return got >= 5 ? 0 : 3;
}
