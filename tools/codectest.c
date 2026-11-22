/* codectest – A30 Codec/Amp-Aktivierung über ALSA-Mixer (SMA1301-Lautsprecher + COD3035-Mikro).
 * Statisch aarch64. Zum Einzeltest der Sequenz über die Tür, nachdem Audio-1 den Takt/Stream liefert.
 * Modi:
 *   codectest spk            – Lautsprecher-Amp (SMA1301) einschalten: Force-Down=0, Vol, Mute=0, Power Up=1
 *   codectest spkvol <0..167>- Lautsprecherlautstärke setzen
 *   codectest mic            – Mikrofonpfad (COD3035) einschalten
 *   codectest set "<name>" <v> – beliebiges int/bool/enum-Control per Name setzen
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <fcntl.h>
#include <unistd.h>
#include <sys/ioctl.h>
#include <stdint.h>

#define IOC(dir,ty,nr,sz) (((dir)<<30)|((sz)<<16)|((ty)<<8)|(nr))
#define CTL_LIST  IOC(3,'U',0x10,80)
#define CTL_INFO  IOC(3,'U',0x11,272)
#define CTL_WRITE IOC(3,'U',0x13,1224)

static unsigned char list[80], ids[64*1024], info[272], val[1224];
static uint32_t u32(const unsigned char*b,int o){uint32_t v;memcpy(&v,b+o,4);return v;}

/* Ein int/bool/enum-Control per exaktem Namen setzen. 1=ok, 0=nicht gefunden/fehlgeschlagen. */
static int set_ctl(int fd, const char*name, long v){
    memset(list,0,sizeof list);
    *(uint32_t*)(list+4)=1024;
    *(uint64_t*)(list+16)=(uint64_t)(uintptr_t)ids;
    if(ioctl(fd,CTL_LIST,list)<0){ perror("LIST"); return 0; }
    uint32_t used=u32(list,8); if(used>1024)used=1024;
    for(uint32_t i=0;i<used;i++){
        unsigned char*id=ids+i*64;
        const char*nm=(const char*)(id+16);
        if(strcmp(nm,name)!=0) continue;
        memset(info,0,sizeof info); memcpy(info,id,64);
        if(ioctl(fd,CTL_INFO,info)<0) return 0;
        uint32_t type=u32(info,64), count=u32(info,72);
        if(type!=1&&type!=2&&type!=3) return 0;
        long cnt=count>8?8:count; if(cnt<1)cnt=1;
        memset(val,0,sizeof val); memcpy(val,id,64);
        for(long c=0;c<cnt;c++) *(int64_t*)(val+72+c*8)=v;
        if(ioctl(fd,CTL_WRITE,val)==0){ printf("  set '%s' = %ld\n",name,v); return 1; }
        printf("  FAIL '%s'\n",name); return 0;
    }
    printf("  (nicht gefunden) '%s'\n",name);
    return 0;
}

int main(int argc,char**argv){
    const char*mode=argc>1?argv[1]:"";
    int fd=open("/dev/snd/controlC0",O_RDWR);
    if(fd<0){ perror("controlC0"); return 1; }
    if(!strcmp(mode,"spk")){
        printf("== Lautsprecher (SMA1301) an ==\n");
        set_ctl(fd,"Force AMP Power Down",0);
        set_ctl(fd,"Speaker Volume",0x38);
        set_ctl(fd,"Speaker Mute Switch(1:muted_0:un)",0);
        set_ctl(fd,"DAC Soft Mute",0);
        set_ctl(fd,"HP HP On",1);
        set_ctl(fd,"Power Up(1:Up_0:Down)",1);   /* KEY: startet sma1301_startup() */
    } else if(!strcmp(mode,"spkvol")&&argc>2){
        set_ctl(fd,"Speaker Volume",atol(argv[2]));
    } else if(!strcmp(mode,"mic")){
        printf("== Mikrofon (COD3035) an ==\n");
        set_ctl(fd,"MIC1 MIC1 On",1);
        set_ctl(fd,"MIC1 On",1);
        set_ctl(fd,"ADCL Mixer MIC1L Switch",1);
        set_ctl(fd,"ADCR Mixer MIC1R Switch",1);
    } else if(!strcmp(mode,"set")&&argc>3){
        set_ctl(fd,argv[2],atol(argv[3]));
    } else {
        printf("Modi: spk | spkvol <0..167> | mic | set \"<name>\" <v>\n");
    }
    close(fd);
    return 0;
}
