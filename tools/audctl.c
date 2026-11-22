/* audctl – ALSA-Mixer + Testton für Ultar (Exynos ABOX / Cod3035). Statisch aarch64.
 * Modi:
 *   audctl dump                     – alle Controls: numid, Typ, Wert, Name
 *   audctl set <numid> <v>          – int/bool setzen (alle Kanäle)
 *   audctl setname <text> <v>       – alle Controls deren Name <text> enthält (int/bool)
 *   audctl setenum <text> <item>    – Enum-Control (Name enthält <text>) auf Item-String setzen
 *   audctl tone <pcmDev> <hz> <ms>  – Sinuston auf /dev/snd/pcmC0D<pcmDev>p (48k/16/Stereo)
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <fcntl.h>
#include <unistd.h>
#include <math.h>
#include <sys/ioctl.h>
#include <stdint.h>
#include <errno.h>

#define IOC(dir,ty,nr,sz) (((dir)<<30)|((sz)<<16)|((ty)<<8)|(nr))
#define CTL_LIST  IOC(3,'U',0x10,80)
#define CTL_INFO  IOC(3,'U',0x11,272)
#define CTL_READ  IOC(3,'U',0x12,1224)
#define CTL_WRITE IOC(3,'U',0x13,1224)
#define PCM_HW_PARAMS IOC(3,'A',0x11,608)
#define PCM_PREPARE   IOC(0,'A',0x40,0)
#define PCM_DRAIN     IOC(0,'A',0x44,0)
#define PCM_WRITEI    IOC(1,'A',0x50,16)   /* snd_xferi: result(long)+buf(ptr)+frames(ulong) */

static unsigned char list[80], ids[64*1024], info[272], val[1224];
static uint32_t u32(const unsigned char*b,int o){uint32_t v;memcpy(&v,b+o,4);return v;}
static int64_t  i64(const unsigned char*b,int o){int64_t v;memcpy(&v,b+o,8);return v;}

static int open_ctl(){int fd=open("/dev/snd/controlC0",O_RDWR);if(fd<0){perror("controlC0");exit(1);}return fd;}
static uint32_t load_list(int fd){
    memset(list,0,sizeof list);*(uint32_t*)(list+4)=1024;*(uint64_t*)(list+16)=(uint64_t)(uintptr_t)ids;
    if(ioctl(fd,CTL_LIST,list)<0){perror("LIST");exit(1);}
    uint32_t u=u32(list,8);return u>1024?1024:u;
}

static void do_tone(int dev,double hz,int ms){
    char path[64];snprintf(path,sizeof path,"/dev/snd/pcmC0D%dp",dev);
    printf("tone: oeffne %s\n",path);
    int fd=open(path,O_WRONLY);
    if(fd<0){printf("%s: %s\n",path,strerror(errno));exit(1);}
    /* hw_params 608 Byte: 3 Masken ab 4, 12 Intervalle ab 260 */
    unsigned char p[608];memset(p,0,sizeof p);
    #define PUT(o,v) do{uint32_t _t=(v);memcpy(p+(o),&_t,4);}while(0)
    PUT(4,1<<3);            /* ACCESS: RW_INTERLEAVED */
    PUT(4+32,1<<2);         /* FORMAT: S16_LE */
    PUT(4+64,1);            /* SUBFORMAT: STD */
    int base=4+8*32;
    for(int i=0;i<12;i++){PUT(base+i*12,0);PUT(base+i*12+4,0xffffffff);PUT(base+i*12+8,0);}
    #define FIX(i,v) do{PUT(base+(i)*12,v);PUT(base+(i)*12+4,v);PUT(base+(i)*12+8,4);}while(0)
    FIX(0,16);FIX(1,32);FIX(2,2);FIX(3,48000);
    PUT(base+5*12,512);PUT(base+5*12+4,4096);
    PUT(base+12*12+9*12,0xffffffff); /* rmask */
    if(ioctl(fd,PCM_HW_PARAMS,p)<0){printf("%s HW_PARAMS: %s\n",path,strerror(errno));exit(1);}
    if(ioctl(fd,PCM_PREPARE,0)<0){printf("%s PREPARE: %s\n",path,strerror(errno));exit(1);}
    int frames=48000*ms/1000;
    short*buf=malloc(frames*4);
    int fade=480;
    for(int i=0;i<frames;i++){
        double env=1.0;if(i<fade)env=(double)i/fade;if(i>frames-fade)env=(double)(frames-i)/fade;
        short s=(short)(sin(i*hz*2*M_PI/48000.0)*9000*env);
        buf[i*2]=s;buf[i*2+1]=s;
    }
    /* in Blöcken schreiben */
    int off=0;
    while(off<frames){
        int n=frames-off;if(n>1024)n=1024;
        unsigned char xf[16];long res=0;uint64_t pb=(uint64_t)(uintptr_t)(buf+off*2);uint64_t fr=n;
        memcpy(xf,&res,8);memcpy(xf+8,&pb,8); /* struct snd_xferi: result, buf, frames (frames über 2. ioctl-Form) */
        /* korrektes Layout snd_xferi {long result; void* buf; snd_pcm_uframes_t frames;} = 24 Byte */
        unsigned char x2[24];memset(x2,0,24);memcpy(x2+8,&pb,8);memcpy(x2+16,&fr,8);
        if(ioctl(fd,IOC(1,'A',0x50,24),x2)<0){printf("WRITEI @%d: %s\n",off,strerror(errno));break;}
        off+=n;
    }
    ioctl(fd,PCM_DRAIN,0);
    printf("Ton fertig auf %s (%d Rahmen)\n",path,frames);
    close(fd);free(buf);
}

int main(int argc,char**argv){
    setvbuf(stdout,NULL,_IONBF,0);
    const char*mode=argc>1?argv[1]:"dump";
    if(!strcmp(mode,"tone")){
        if(argc<5){printf("tone <dev> <hz> <ms>\n");return 1;}
        do_tone(atoi(argv[2]),atof(argv[3]),atoi(argv[4]));return 0;
    }
    int fd=open_ctl();uint32_t used=load_list(fd);
    const char*key=argc>2?argv[2]:"";
    long setv=argc>3?atol(argv[3]):0;
    const char*item=argc>3?argv[3]:"";
    long setnum=(!strcmp(mode,"set")&&argc>2)?atol(argv[2]):-1;
    int done=0;
    for(uint32_t i=0;i<used;i++){
        unsigned char*id=ids+i*64;uint32_t numid=u32(id,0);const char*name=(const char*)(id+16);
        memset(info,0,sizeof info);memcpy(info,id,64);
        if(ioctl(fd,CTL_INFO,info)<0)continue;
        uint32_t type=u32(info,64),count=u32(info,72);int64_t mn=i64(info,80),mx=i64(info,88);
        memset(val,0,sizeof val);memcpy(val,id,64);long long v0=0;int hv=0;
        if(ioctl(fd,CTL_READ,val)==0){hv=1;v0=i64(val,72);}
        if(!strcmp(mode,"dump")){
            const char*ts=type==1?"bool":type==2?"int":type==3?"enum":type==4?"bytes":"?";
            printf("#%u %s cnt=%u [%lld..%lld] = %lld  %s\n",numid,ts,count,(long long)mn,(long long)mx,hv?v0:-1,name);
        } else if(!strcmp(mode,"setenum")&&strstr(name,key)&&type==3){
            /* Items durchgehen, passenden Namen finden */
            uint32_t items=u32(info,80),found=0xffffffff; if(items>128)items=128;
            for(uint32_t it=0;it<items;it++){
                memset(info,0,sizeof info);memcpy(info,id,64);
                *(uint32_t*)(info+84)=it; /* value.enumerated.item = it */
                if(ioctl(fd,CTL_INFO,info)<0)continue;
                const char*en=(const char*)(info+88);
                if(!strcmp(en,item)){found=it;break;}
            }
            if(found==0xffffffff){printf("?? #%u %s: Item '%s' nicht gefunden\n",numid,name,item);continue;}
            memset(val,0,sizeof val);memcpy(val,id,64);*(uint32_t*)(val+72)=found;
            if(ioctl(fd,CTL_WRITE,val)==0){printf("enum #%u %s = %s(%u)\n",numid,name,item,found);done++;}
            else printf("FAIL #%u %s\n",numid,name);
        } else if((!strcmp(mode,"set")&&(long)numid==setnum)||(!strcmp(mode,"setname")&&strstr(name,key))){
            if(type!=1&&type!=2)continue;
            long cnt=count>8?8:count;memset(val,0,sizeof val);memcpy(val,id,64);
            for(long c=0;c<cnt;c++)*(int64_t*)(val+72+c*8)=setv;
            if(ioctl(fd,CTL_WRITE,val)==0){printf("set #%u %s = %ld\n",numid,name,setv);done++;}
            else printf("FAIL #%u %s\n",numid,name);
        }
    }
    if(strcmp(mode,"dump"))printf("== %d gesetzt ==\n",done);
    return 0;
}
