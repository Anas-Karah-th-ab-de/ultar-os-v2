// windec – Test: setzt per DECON S3CFB_WIN_CONFIG ein einfarbiges Vollbild-Fenster.
// Freistehend (kein libc), damit es klein bleibt und über die serielle Tür passt.
// Strukturen exakt wie im A30-Kernel (CONFIG_DPU_20=y, CONFIG_SUPPORT_MASK_LAYER=y).
typedef unsigned int u32; typedef unsigned char u8; typedef unsigned long dma_addr_t;
static long sc(long n,long a,long b,long c,long d,long e){ register long x8 __asm__("x8")=n,x0 __asm__("x0")=a,x1 __asm__("x1")=b,x2 __asm__("x2")=c,x3 __asm__("x3")=d,x4 __asm__("x4")=e; __asm__ volatile("svc 0":"+r"(x0):"r"(x8),"r"(x1),"r"(x2),"r"(x3),"r"(x4):"memory"); return x0; }
static int slen(const char*s){int n=0;while(s[n])n++;return n;}
static void pr(const char*s){ sc(64,1,(long)s,slen(s),0,0); }
static void prn(long v){ char b[24]; int i=22; b[23]=0; if(v==0){pr("0");return;} int neg=v<0; unsigned long u=neg?-v:v; while(u){b[i--]='0'+u%10;u/=10;} if(neg)b[i--]='-'; pr(b+i+1); }

#define MAX_PLANE_CNT 3
#define MAX_DECON_WIN 4
struct decon_win_rect { int x,y; u32 w,h; };
struct decon_frame { int x,y; u32 w,h,f_w,f_h; };
struct dpp_params { dma_addr_t addr[MAX_PLANE_CNT]; int flip,eq_mode,comp_src,hdr_std; u32 min_luminance,max_luminance; };
struct decon_win_config {
    int state;
    union {
        u32 color;
        struct {
            int fd_idma[3], fence_fd, rel_fence, plane_alpha, blending, idma_type, format;
            struct dpp_params dpp_parm;
            struct decon_win_rect block_area, transparent_area, opaque_area;
            struct decon_frame src;
        };
    };
    struct decon_frame dst;
    u8 protection, compression;
};
struct decon_win_config_extra { int remained_frames; u32 reserved[7]; };
struct decon_win_config_data {
    int fence, fd_odma;
    struct decon_win_config config[MAX_DECON_WIN + 1];
    struct decon_win_config_extra extra;
};
#define DECON_WIN_STATE_DISABLED 0
#define DECON_WIN_STATE_COLOR 1
#define _IOW_(t,nr,sz) ((1u<<30)|((unsigned)(sz)<<16)|((unsigned)(t)<<8)|(unsigned)(nr))
#define S3CFB_WIN_CONFIG _IOW_('F',209,sizeof(struct decon_win_config_data))

void _start(void){
    pr("sizeof(win_config)="); prn(sizeof(struct decon_win_config));
    pr(" sizeof(data)="); prn(sizeof(struct decon_win_config_data)); pr("\n");
    long fd = sc(56,-100,(long)"/dev/fb0",2,0,0);
    if(fd<0){ pr("open fb0 fehlgeschlagen\n"); sc(93,1,0,0,0,0); }
    struct decon_win_config_data d;
    { u8 *pp=(u8*)&d; for(unsigned i=0;i<sizeof(d);i++) pp[i]=0; }
    // alles 0; dann Fenster 0 = Farbe gruen, Vollbild
    d.fence=-1; d.fd_odma=-1;
    for(int i=0;i<=MAX_DECON_WIN;i++){ d.config[i].state=DECON_WIN_STATE_DISABLED; }
    int win=0;
    d.config[win].state=DECON_WIN_STATE_COLOR;
    d.config[win].color=0x0000ff00;  // gruen
    d.config[win].dst.x=0; d.config[win].dst.y=0; d.config[win].dst.w=1080; d.config[win].dst.h=2340; d.config[win].dst.f_w=1080; d.config[win].dst.f_h=2340;
    // 20x submitten (alle 500ms), fb0 bleibt offen -> Gruen steht ~10s
    long r=0;
    for(int k=0;k<20;k++){
        d.fence=-1; d.config[win].state=DECON_WIN_STATE_COLOR; d.config[win].color=0x0000ff00;
        r = sc(29,fd,(long)S3CFB_WIN_CONFIG,(long)&d,0,0);
        if(d.fence>=0) sc(57,d.fence,0,0,0,0);   // Freigabe-Fence schliessen
        long ts[2]={0,500000000}; sc(101,(long)ts,0,0,0,0);
    }
    pr("WIN_CONFIG rc="); prn(r); pr(" (20x gruen gehalten)\n");
    sc(93,0,0,0,0,0);
}
