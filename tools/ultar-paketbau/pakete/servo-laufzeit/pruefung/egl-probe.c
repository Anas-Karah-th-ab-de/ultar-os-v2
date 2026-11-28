// Check of the package servo-laufzeit: EGL on the surfaceless platform, an OpenGL context on
// llvmpipe, one cleared frame read back. Prints "EGL: llvmpipe ok" only when the renderer is
// llvmpipe and the pixel has the colour that was cleared.
#define EGL_EGLEXT_PROTOTYPES
#include <EGL/egl.h>
#include <EGL/eglext.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef const unsigned char *(*GetString)(unsigned int);
typedef void (*ClearColor)(float, float, float, float);
typedef void (*Clear)(unsigned int);
typedef void (*ReadPixels)(int, int, int, int, unsigned int, unsigned int, void *);
typedef void (*Finish)(void);

int main(void) {
    setenv("LIBGL_ALWAYS_SOFTWARE", "1", 1);
    setenv("EGL_PLATFORM", "surfaceless", 1);
    EGLDisplay dpy = eglGetPlatformDisplay(EGL_PLATFORM_SURFACELESS_MESA, EGL_DEFAULT_DISPLAY, NULL);
    if (dpy == EGL_NO_DISPLAY) { puts("EGL: no display"); return 1; }
    EGLint major = 0, minor = 0;
    if (!eglInitialize(dpy, &major, &minor)) { printf("EGL: initialize failed 0x%x\n", eglGetError()); return 1; }
    printf("EGL: version %d.%d vendor %s\n", major, minor, eglQueryString(dpy, EGL_VENDOR));
    if (!eglBindAPI(EGL_OPENGL_API)) { puts("EGL: no OpenGL"); return 1; }
    const EGLint want[] = {EGL_SURFACE_TYPE, EGL_PBUFFER_BIT, EGL_RENDERABLE_TYPE, EGL_OPENGL_BIT, EGL_NONE};
    EGLConfig cfg; EGLint n = 0;
    if (!eglChooseConfig(dpy, want, &cfg, 1, &n) || n < 1) { printf("EGL: no config 0x%x\n", eglGetError()); return 1; }
    EGLContext ctx = eglCreateContext(dpy, cfg, EGL_NO_CONTEXT, NULL);
    if (ctx == EGL_NO_CONTEXT) { printf("EGL: no context 0x%x\n", eglGetError()); return 1; }
    const EGLint pb[] = {EGL_WIDTH, 16, EGL_HEIGHT, 16, EGL_NONE};
    EGLSurface surf = eglCreatePbufferSurface(dpy, cfg, pb);
    if (!eglMakeCurrent(dpy, surf, surf, ctx)) { printf("EGL: make current failed 0x%x\n", eglGetError()); return 1; }
    GetString gs = (GetString)eglGetProcAddress("glGetString");
    ClearColor cc = (ClearColor)eglGetProcAddress("glClearColor");
    Clear cl = (Clear)eglGetProcAddress("glClear");
    ReadPixels rp = (ReadPixels)eglGetProcAddress("glReadPixels");
    Finish fin = (Finish)eglGetProcAddress("glFinish");
    if (!gs || !cc || !cl || !rp || !fin) { puts("EGL: GL entry points missing"); return 1; }
    const char *renderer = (const char *)gs(0x1F01);
    printf("EGL: renderer %s\n", renderer ? renderer : "?");
    cc(1.0f, 0.0f, 0.0f, 1.0f);
    cl(0x4000);
    fin();
    unsigned char px[4] = {0, 0, 0, 0};
    rp(3, 3, 1, 1, 0x1908, 0x1401, px);
    printf("EGL: pixel %u %u %u %u\n", px[0], px[1], px[2], px[3]);
    if (renderer && strstr(renderer, "llvmpipe") && px[0] == 255 && px[1] == 0 && px[2] == 0) {
        puts("EGL: llvmpipe ok");
        return 0;
    }
    puts("EGL: wrong renderer or pixel");
    return 1;
}
