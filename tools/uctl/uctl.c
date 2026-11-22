// uctl – kleines Display-/Speicher-Werkzeug für die Fehlersuche am Panel (statisch, läuft auf jedem Linux-Kernel).
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <fcntl.h>
#include <unistd.h>
#include <sys/ioctl.h>
#include <stdint.h>
#include <linux/fb.h>
#include <errno.h>
static int fbfill(const char *dev, uint32_t color){
    int fd=open(dev,O_RDWR); if(fd<0){perror("open fb");return 1;}
    struct fb_var_screeninfo v; struct fb_fix_screeninfo f;
    if(ioctl(fd,FBIOGET_VSCREENINFO,&v)||ioctl(fd,FBIOGET_FSCREENINFO,&f)){perror("ioctl info");return 1;}
    size_t line=f.line_length, h=v.yres, w=v.xres;
    uint32_t *row=malloc(line); for(size_t i=0;i<line/4;i++) row[i]=color;
    lseek(fd,0,SEEK_SET);
    for(size_t y=0;y<h;y++) if(write(fd,row,line)<0){perror("write");break;}
    v.xoffset=0; v.yoffset=0; v.activate=FB_ACTIVATE_FORCE;
    ioctl(fd,FBIOPAN_DISPLAY,&v);
    printf("fbfill %zux%zu line=%zu color=%08x ok\n",w,h,line,color); return 0;
}
static int fbblank(const char *dev,int mode){
    int fd=open(dev,O_RDWR); if(fd<0){perror("open");return 1;}
    int r=ioctl(fd,FBIOBLANK,mode); printf("FBIOBLANK %d -> %d (errno %d)\n",mode,r,errno); return 0;
}
static int fbinfo(const char *dev){
    int fd=open(dev,O_RDWR); if(fd<0){perror("open");return 1;}
    struct fb_var_screeninfo v; struct fb_fix_screeninfo f;
    ioctl(fd,FBIOGET_VSCREENINFO,&v); ioctl(fd,FBIOGET_FSCREENINFO,&f);
    printf("xres=%u yres=%u bpp=%u line=%u smem_start=%lx smem_len=%u activate=%u\n",
        v.xres,v.yres,v.bits_per_pixel,f.line_length,f.smem_start,f.smem_len,v.activate);
    printf("red(%u,%u) green(%u,%u) blue(%u,%u)\n",v.red.offset,v.red.length,v.green.offset,v.green.length,v.blue.offset,v.blue.length);
    return 0;
}
static int memfill(unsigned long phys, uint32_t color, int rows, int wpix){
    int fd=open("/dev/mem",O_RDWR|O_SYNC); if(fd<0){perror("open /dev/mem");return 1;}
    uint32_t *row=malloc(wpix*4); for(int i=0;i<wpix;i++) row[i]=color;
    if(lseek(fd,phys,SEEK_SET)==(off_t)-1){perror("lseek");return 1;}
    for(int y=0;y<rows;y++) if(write(fd,row,wpix*4)<0){perror("write mem");return 1;}
    printf("memfill phys=%lx rows=%d w=%d color=%08x ok\n",phys,rows,wpix,color); return 0;
}
static int fbshow(const char *dev, uint32_t color, int secs){
    int fd=open(dev,O_RDWR); if(fd<0){perror("open");return 1;}
    struct fb_var_screeninfo v; struct fb_fix_screeninfo f;
    ioctl(fd,FBIOGET_VSCREENINFO,&v); ioctl(fd,FBIOGET_FSCREENINFO,&f);
    size_t line=f.line_length, h=v.yres;
    printf("blank4=%d ", ioctl(fd,FBIOBLANK,4)); fflush(stdout); usleep(200000);
    printf("blank0=%d ", ioctl(fd,FBIOBLANK,0)); fflush(stdout); usleep(300000);
    uint32_t *row=malloc(line); for(size_t i=0;i<line/4;i++) row[i]=color;
    lseek(fd,0,SEEK_SET); for(size_t y=0;y<h;y++) if(write(fd,row,line)<0){perror("write");break;}
    v.xoffset=0; v.yoffset=0; v.activate=FB_ACTIVATE_FORCE;
    printf("pan=%d ", ioctl(fd,FBIOPAN_DISPLAY,&v)); fflush(stdout);
    printf("-> fb offen, %ds halten\n", secs);
    for(int i=0;i<secs;i++){ // alle 1s neu pannen, damit DECON nicht einschlaeft
        usleep(1000000); lseek(fd,0,SEEK_SET); for(size_t y=0;y<h;y++) if(write(fd,row,line)<0)break;
        v.activate=FB_ACTIVATE_FORCE; ioctl(fd,FBIOPAN_DISPLAY,&v);
    }
    close(fd); printf("fertig\n"); return 0;
}

int main(int argc,char**argv){
    if(argc<2){fprintf(stderr,"uctl fbfill <dev> <hexcolor> | fbblank <dev> <n> | fbinfo <dev> | memfill <physhex> <hexcolor> <rows> <wpix>\n");return 2;}
    if(!strcmp(argv[1],"fbfill")&&argc>=4) return fbfill(argv[2],(uint32_t)strtoul(argv[3],0,16));
    if(!strcmp(argv[1],"fbblank")&&argc>=4) return fbblank(argv[2],atoi(argv[3]));
    if(!strcmp(argv[1],"fbinfo")&&argc>=3) return fbinfo(argv[2]);
    if(!strcmp(argv[1],"show")&&argc>=5) return fbshow(argv[2],(uint32_t)strtoul(argv[3],0,16),atoi(argv[4]));
    if(!strcmp(argv[1],"memfill")&&argc>=6) return memfill(strtoul(argv[2],0,16),(uint32_t)strtoul(argv[3],0,16),atoi(argv[4]),atoi(argv[5]));
    fprintf(stderr,"unbekannt\n"); return 2;
}
