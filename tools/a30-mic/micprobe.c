/* micprobe – ALSA-Regler ansehen/setzen und Aufnahme pruefen (Exynos ABOX, A30). Statisch aarch64.
 *   micprobe dump [text]                 alle Regler (optional nur Namen mit <text>): Typ, Wert, Enum-Eintraege
 *   micprobe set "<genauer Name>" <v>    Zahl oder Enum-Eintrag (Text) setzen
 *   micprobe rec <geraet> <ms> <datei|-> [kanaele] [rate] [periode] [perioden]   von /dev/snd/pcmC0D<geraet>c aufnehmen, Pegel melden
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <fcntl.h>
#include <unistd.h>
#include <sys/ioctl.h>
#include <stdint.h>
#include <errno.h>
#include <math.h>
#include <time.h>
#define IOC(dir,ty,nr,sz) (((unsigned long)(dir)<<30)|((unsigned long)(sz)<<16)|((ty)<<8)|(nr))
#define CTL_LIST  IOC(3,'U',0x10,80)
#define CTL_INFO  IOC(3,'U',0x11,272)
#define CTL_READ  IOC(3,'U',0x12,1224)
#define CTL_WRITE IOC(3,'U',0x13,1224)
#define PCM_HW_PARAMS IOC(3,'A',0x11,608)
#define PCM_PREPARE   IOC(0,'A',0x40,0)
#define PCM_START     IOC(0,'A',0x42,0)
#define PCM_READI     IOC(2,'A',0x51,24)
#define MAXC 2048
static unsigned char list[80], ids[64*MAXC], info[272], val[1224];
static uint32_t u32(const unsigned char*b,int o){uint32_t v;memcpy(&v,b+o,4);return v;}
static int64_t i64(const unsigned char*b,int o){int64_t v;memcpy(&v,b+o,8);return v;}
static int cfd, used;
static int ctl_open(void){
    cfd=open("/dev/snd/controlC0",O_RDWR); if(cfd<0){perror("controlC0");return -1;}
    memset(list,0,80); uint32_t sp=MAXC; memcpy(list+4,&sp,4); uint64_t p=(uint64_t)(uintptr_t)ids; memcpy(list+16,&p,8);
    if(ioctl(cfd,CTL_LIST,list)<0){perror("LIST");return -1;}
    used=u32(list,8); if(used>MAXC)used=MAXC; return 0;
}
static int get_info(int i){ memset(info,0,272); memcpy(info,ids+i*64,64); return ioctl(cfd,CTL_INFO,info); }
static void enum_name(int i,unsigned item,char*out){
    unsigned char in2[272]; memset(in2,0,272); memcpy(in2,ids+i*64,64); memcpy(in2+84,&item,4);
    if(ioctl(cfd,CTL_INFO,in2)<0){strcpy(out,"?");return;} memcpy(out,in2+88,64); out[63]=0;
}
int main(int argc,char**argv){
    const char*mode=argc>1?argv[1]:"dump";
    if(!strcmp(mode,"rec")&&argc>=5){
        int dev=atoi(argv[2]),ms=atoi(argv[3]); int ch=argc>5?atoi(argv[5]):2; int rate=argc>6?atoi(argv[6]):48000;
        char path[64]; snprintf(path,64,"/dev/snd/pcmC0D%dc",dev);
        int fd=open(path,O_RDONLY); if(fd<0){printf("%s: %s\n",path,strerror(errno));return 1;}
        unsigned char p[608]; memset(p,0,608);
        #define PUT(o,v) do{uint32_t _t=(v);memcpy(p+(o),&_t,4);}while(0)
        PUT(4,1<<3); PUT(4+32,1<<2); PUT(4+64,1); int base=4+8*32;
        for(int i=0;i<12;i++){PUT(base+i*12,0);PUT(base+i*12+4,0xffffffff);PUT(base+i*12+8,0);}
        #define FIX(i,v) do{PUT(base+(i)*12,v);PUT(base+(i)*12+4,v);PUT(base+(i)*12+8,4);}while(0)
        int per=argc>7?atoi(argv[7]):960, nper=argc>8?atoi(argv[8]):8;
        FIX(0,16);FIX(1,16*ch);FIX(2,ch);FIX(3,rate); FIX(5,per); FIX(7,nper); PUT(base+12*12+9*12,0xffffffff);
        if(ioctl(fd,PCM_HW_PARAMS,p)<0){printf("%s HW_PARAMS: %s\n",path,strerror(errno));return 1;}
        if(ioctl(fd,PCM_PREPARE,0)<0){printf("PREPARE: %s\n",strerror(errno));return 1;}
        ioctl(fd,PCM_START,0);
        struct timespec t0,t1; clock_gettime(CLOCK_MONOTONIC,&t0);
        int frames=(long)rate*ms/1000, off=0; short*buf=malloc((size_t)frames*ch*2); memset(buf,0,(size_t)frames*ch*2);
        while(off<frames){
            int n=frames-off; if(n>960)n=960; unsigned char x[24]; memset(x,0,24);
            uint64_t pb=(uint64_t)(uintptr_t)(buf+(size_t)off*ch), fr=n; memcpy(x+8,&pb,8); memcpy(x+16,&fr,8);
            if(ioctl(fd,PCM_READI,x)<0){printf("READI @%d: %s\n",off,strerror(errno));break;}
            off+=n;
        }
        clock_gettime(CLOCK_MONOTONIC,&t1);
        printf("%s: %d Rahmen gelesen (%d Kanaele, %d Hz) in %ld ms\n",path,off,ch,rate,(long)((t1.tv_sec-t0.tv_sec)*1000+(t1.tv_nsec-t0.tv_nsec)/1000000));
        for(int c=0;c<ch;c++){
            double sum=0,mean=0; int pk=0; long zero=0;
            for(int i=0;i<off;i++) mean+=buf[i*ch+c]; mean/= off?off:1;
            for(int i=0;i<off;i++){ double v=buf[i*ch+c]-mean; sum+=v*v; int a=abs(buf[i*ch+c]); if(a>pk)pk=a; if(buf[i*ch+c]==0)zero++; }
            printf("  Kanal %d: Spitze %d, RMS %.1f, Mittel %.1f, Nullen %ld\n",c,pk,sqrt(sum/(off?off:1)),mean,zero);
        }
        /* Verlauf: RMS je 250 ms von Kanal 0 */
        printf("  Verlauf K0:"); int blk=rate/4;
        for(int b=0;b+blk<=off;b+=blk){ double s=0; for(int i=b;i<b+blk;i++){double v=buf[i*ch];s+=v*v;} printf(" %.0f",sqrt(s/blk)); } printf("\n");
        if(strcmp(argv[4],"-")){ int o=open(argv[4],O_WRONLY|O_CREAT|O_TRUNC,0644); if(o>=0){ if(write(o,buf,(size_t)off*ch*2)<0)perror("write"); close(o);} }
        close(fd); return 0;
    }
    if(ctl_open()<0) return 1;
    if(!strcmp(mode,"set")&&argc>=4){
        for(int i=0;i<used;i++){
            char nm[45]; memcpy(nm,ids+i*64+16,44); nm[44]=0; if(strcmp(nm,argv[2])) continue;
            if(get_info(i)<0){perror("INFO");return 1;}
            unsigned ty=u32(info,64), cnt=u32(info,72); if(cnt>8)cnt=8; if(cnt<1)cnt=1;
            memset(val,0,1224); memcpy(val,ids+i*64,64);
            if(ty==3){
                unsigned items=u32(info,80); long it=-1; char en[64];
                for(unsigned k=0;k<items;k++){ enum_name(i,k,en); if(!strcmp(en,argv[3])){it=k;break;} }
                if(it<0){ char*e; it=strtol(argv[3],&e,10); if(*e){printf("FEHLER %s: Eintrag '%s' gibt es nicht\n",nm,argv[3]);return 1;} }
                for(unsigned c=0;c<cnt;c++){uint32_t v=it;memcpy(val+72+c*4,&v,4);}
            } else { int64_t v=atoll(argv[3]); for(unsigned c=0;c<cnt;c++)memcpy(val+72+c*8,&v,8); }
            if(ioctl(cfd,CTL_WRITE,val)<0){printf("FEHLER %s = %s: %s\n",nm,argv[3],strerror(errno));return 1;}
            printf("ok %s = %s\n",nm,argv[3]); return 0;
        }
        printf("FEHLER kein Regler '%s'\n",argv[2]); return 1;
    }
    const char*flt=argc>2?argv[2]:"";
    for(int i=0;i<used;i++){
        char nm[45]; memcpy(nm,ids+i*64+16,44); nm[44]=0; if(*flt&&!strstr(nm,flt)) continue;
        if(get_info(i)<0) continue;
        unsigned ty=u32(info,64), cnt=u32(info,72);
        memset(val,0,1224); memcpy(val,ids+i*64,64); int rd=ioctl(cfd,CTL_READ,val);
        printf("%u|%s|t%u n%u|",u32(ids,i*64),nm,ty,cnt);
        if(rd<0){printf("?\n");continue;}
        if(ty==3){ unsigned items=u32(info,80), cur=u32(val,72); char en[64]; enum_name(i,cur,en); printf("%s [",en);
            for(unsigned k=0;k<items&&k<40;k++){enum_name(i,k,en);printf("%s%s",k?",":"",en);} printf("]\n"); }
        else if(ty==1||ty==2){ for(unsigned c=0;c<cnt&&c<4;c++)printf("%lld ",(long long)i64(val,72+c*8)); if(ty==2)printf("(%lld..%lld)",(long long)i64(info,80),(long long)i64(info,88)); printf("\n"); }
        else printf("-\n");
    }
    return 0;
}
