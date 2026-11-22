// windec2 – ION-Bildpuffer + DECON S3CFB_WIN_CONFIG (BUFFER-Fenster). Zeichnet einen Verlauf.
typedef unsigned int u32; typedef unsigned char u8; typedef unsigned long ulong; typedef unsigned long dma_addr_t;
static long sc(long n,long a,long b,long c,long d,long e){ register long x8 __asm__("x8")=n,x0 __asm__("x0")=a,x1 __asm__("x1")=b,x2 __asm__("x2")=c,x3 __asm__("x3")=d,x4 __asm__("x4")=e; __asm__ volatile("svc 0":"+r"(x0):"r"(x8),"r"(x1),"r"(x2),"r"(x3),"r"(x4):"memory"); return x0; }
static long sc6(long n,long a,long b,long c,long d,long e,long f){ register long x8 __asm__("x8")=n,x0 __asm__("x0")=a,x1 __asm__("x1")=b,x2 __asm__("x2")=c,x3 __asm__("x3")=d,x4 __asm__("x4")=e,x5 __asm__("x5")=f; __asm__ volatile("svc 0":"+r"(x0):"r"(x8),"r"(x1),"r"(x2),"r"(x3),"r"(x4),"r"(x5):"memory"); return x0; }
static int slen(const char*s){int n=0;while(s[n])n++;return n;}
static void pr(const char*s){ sc(64,1,(long)s,slen(s),0,0); }
static void prn(long v){ char b[24]; int i=22; b[23]=0; if(v==0){pr("0");return;} int neg=v<0; ulong u=neg?-v:v; while(u){b[i--]='0'+u%10;u/=10;} if(neg)b[i--]='-'; pr(b+i+1); }

#define MAX_PLANE_CNT 3
#define MAX_DECON_WIN 4
struct decon_win_rect { int x,y; u32 w,h; };
struct decon_frame { int x,y; u32 w,h,f_w,f_h; };
struct dpp_params { dma_addr_t addr[MAX_PLANE_CNT]; int flip,eq_mode,comp_src,hdr_std; u32 min_luminance,max_luminance; };
struct decon_win_config { int state; union { u32 color; struct { int fd_idma[3], fence_fd, rel_fence, plane_alpha, blending, idma_type, format; struct dpp_params dpp_parm; struct decon_win_rect block_area, transparent_area, opaque_area; struct decon_frame src; }; }; struct decon_frame dst; u8 protection, compression; };
struct decon_win_config_extra { int remained_frames; u32 reserved[7]; };
struct decon_win_config_data { int fence, fd_odma; struct decon_win_config config[MAX_DECON_WIN + 1]; struct decon_win_config_extra extra; };
#define DECON_WIN_STATE_DISABLED 0
#define DECON_WIN_STATE_BUFFER 2
#define FMT_XRGB_8888 4
#define IDMA_G0 0
#define _IOW_(t,nr,sz) ((1u<<30)|((u32)(sz)<<16)|((u32)(t)<<8)|(u32)(nr))
#define _IOWR_(t,nr,sz) ((3u<<30)|((u32)(sz)<<16)|((u32)(t)<<8)|(u32)(nr))
#define S3CFB_WIN_CONFIG _IOW_('F',209,sizeof(struct decon_win_config_data))

struct ion_allocation_data { ulong len; ulong align; u32 heap_id_mask; u32 flags; int handle; };
struct ion_fd_data { int handle; int fd; };
#define ION_IOC_ALLOC _IOWR_('I',0,sizeof(struct ion_allocation_data))
#define ION_IOC_SHARE _IOWR_('I',4,sizeof(struct ion_fd_data))

#define W 1080
#define H 2340
__asm__(".global _start\n_start:\n mov x0, sp\n bl real_start\n");
void real_start(long*sp){
    long argc=sp[0]; char**argv=(char**)(sp+1);
    int FMT=4; u32 COL=0x00ff0000;
    if(argc>1){ FMT=0; for(char*p=argv[1]; *p>='0'&&*p<='9'; p++) FMT=FMT*10+(*p-'0'); }
    if(argc>2){ COL=0; for(char*p=argv[2]; *p; p++){ int d=(*p>='0'&&*p<='9')?*p-'0':((*p|32)>='a'&&(*p|32)<='f')?(*p|32)-'a'+10:-1; if(d<0)break; COL=(COL<<4)|d; } }
    pr("Format="); prn(FMT); pr(" Farbe=0x"); prn(COL); pr("\n");
    long ion = sc(56,-100,(long)"/dev/ion",2,0,0);
    if(ion<0){ pr("open /dev/ion fehlgeschlagen rc="); prn(ion); pr("\n"); sc(93,1,0,0,0,0); }
    struct ion_allocation_data a; { u8*p=(u8*)&a; for(unsigned i=0;i<sizeof(a);i++) p[i]=0; }
    a.len=(ulong)W*H*4; a.align=0; a.heap_id_mask=1; a.flags=0;
    long r = sc(29,ion,(long)ION_IOC_ALLOC,(long)&a,0,0);
    pr("ION_ALLOC rc="); prn(r); pr(" handle="); prn(a.handle); pr("\n");
    if(r<0) sc(93,2,0,0,0,0);
    struct ion_fd_data fdd; { u8*p=(u8*)&fdd; for(unsigned i=0;i<sizeof(fdd);i++) p[i]=0; }
    fdd.handle=a.handle;
    r = sc(29,ion,(long)ION_IOC_SHARE,(long)&fdd,0,0);
    pr("ION_SHARE rc="); prn(r); pr(" fd="); prn(fdd.fd); pr("\n");
    if(r<0||fdd.fd<0) sc(93,3,0,0,0,0);
    // mmap(222): addr0 len prot(3) flags(MAP_SHARED=1) fd off0
    long map = sc6(222,0,(ulong)W*H*4,3,1,fdd.fd,0);
    pr("mmap="); prn(map); pr("\n");
    if(map<0||map==-1) sc(93,4,0,0,0,0);
    u32*px=(u32*)map;
    for(int i=0;i<W*H;i++) px[i]=COL;
    long fb = sc(56,-100,(long)"/dev/fb0",2,0,0);
    if(fb<0){ pr("open fb0 rc="); prn(fb); pr("\n"); sc(93,5,0,0,0,0); }
    struct decon_win_config_data d;
    int win=3;
    for(int k=0;k<24;k++){
        { u8*p=(u8*)&d; for(unsigned i=0;i<sizeof(d);i++) p[i]=0; }
        d.fence=-1; d.fd_odma=-1;
        for(int i=0;i<=MAX_DECON_WIN;i++){ d.config[i].state=DECON_WIN_STATE_DISABLED; d.config[i].fd_idma[0]=-1; d.config[i].fd_idma[1]=-1; d.config[i].fd_idma[2]=-1; d.config[i].fence_fd=-1; d.config[i].rel_fence=-1; }
        struct decon_win_config*c=&d.config[win];
        c->state=DECON_WIN_STATE_BUFFER;
        c->fd_idma[0]=fdd.fd; c->fd_idma[1]=-1; c->fd_idma[2]=-1;
        c->fence_fd=-1; c->rel_fence=-1; c->plane_alpha=255; c->blending=0; c->idma_type=IDMA_G0; c->format=FMT;
        c->src.x=0;c->src.y=0;c->src.w=W;c->src.h=H;c->src.f_w=W;c->src.f_h=H;
        c->dst.x=0;c->dst.y=0;c->dst.w=W;c->dst.h=H;c->dst.f_w=W;c->dst.f_h=H;
        r = sc(29,fb,(long)S3CFB_WIN_CONFIG,(long)&d,0,0);
        if(k==0){ pr("WIN_CONFIG(buffer,win3) rc="); prn(r); pr(" rel_fence="); prn(d.fence); pr(" win_rel="); prn(c->rel_fence); pr("\n"); }
        if(d.fence>=0) sc(57,d.fence,0,0,0,0);
        long ts[2]={0,400000000}; sc(101,(long)ts,0,0,0,0);
    }
    pr("fertig (Verlauf 24x gehalten)\n");
    sc(93,0,0,0,0,0);
}
