/* androidcheck – prüft, ob/wie eine ISOLIERTE Android-Laufzeit (Waydroid-Stil) auf Ultar
 * möglich ist: Android-Bausteine (binder/ashmem/ion/mali), SELinux, und vor allem die
 * erreichbare Isolation (Namespaces, seccomp). Rein lesend/forkend, verändert nichts.
 * Statisch aarch64. Modus: androidcheck [report]
 */
#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <fcntl.h>
#include <unistd.h>
#include <sched.h>
#include <sys/wait.h>
#include <sys/stat.h>
#include <sys/prctl.h>
#include <errno.h>

static int exists(const char*p){ struct stat s; return stat(p,&s)==0; }
static void node(const char*p){ printf("  %-22s %s\n", p, exists(p)?"[DA]":"[fehlt]"); }

/* Namespace-Erzeugung im Kindprozess testen (ohne den Hauptprozess zu verändern). */
static int try_unshare(int flags){
    pid_t pid=fork();
    if(pid==0){ _exit(unshare(flags)==0?0:1); }
    int st=0; waitpid(pid,&st,0);
    return WIFEXITED(st)&&WEXITSTATUS(st)==0;
}

static int has_fs(const char*name){
    FILE*f=fopen("/proc/filesystems","r"); if(!f) return -1;
    char line[128]; int found=0;
    while(fgets(line,sizeof line,f)){ if(strstr(line,name)){ found=1; break; } }
    fclose(f); return found;
}

int main(void){
    printf("== Ultar Android-Laufzeit: Machbarkeits- und Isolationsprüfung ==\n\n");

    printf("[Android-Bausteine]\n");
    node("/dev/binder"); node("/dev/hwbinder"); node("/dev/vndbinder");
    node("/dev/ashmem"); node("/dev/ion"); node("/dev/mali0");
    node("/dev/dri");   // DRM/KMS? (A30: wahrscheinlich fehlt -> SwiftShader)

    printf("\n[SELinux]\n");
    char buf[8]={0}; int fd=open("/sys/fs/selinux/enforce",O_RDONLY);
    if(fd<0) printf("  /sys/fs/selinux nicht gemountet -> SELinux inaktiv (gut: Host sauber)\n");
    else { int n=read(fd,buf,7); (void)n; close(fd); printf("  enforce=%s\n", buf); }

    printf("\n[Isolation: Namespaces (im Kindprozess getestet)]\n");
    printf("  mount(NEWNS)  %s\n", try_unshare(CLONE_NEWNS)?"ok":"NEIN");
    printf("  pid(NEWPID)   %s\n", try_unshare(CLONE_NEWPID)?"ok":"NEIN");
    printf("  net(NEWNET)   %s\n", try_unshare(CLONE_NEWNET)?"ok":"NEIN");
    printf("  ipc(NEWIPC)   %s\n", try_unshare(CLONE_NEWIPC)?"ok":"NEIN");
    printf("  uts(NEWUTS)   %s\n", try_unshare(CLONE_NEWUTS)?"ok":"NEIN");
    printf("  user(NEWUSER) %s  (A30-Kernel: USER_NS aus -> Container laeuft privilegiert, dafuer seccomp+capdrop)\n",
           try_unshare(CLONE_NEWUSER)?"ok":"NEIN");

    printf("\n[Isolation: seccomp]\n");
    int sc=prctl(PR_GET_SECCOMP);
    printf("  PR_GET_SECCOMP=%d (>=0 = seccomp im Kernel vorhanden)\n", sc);

    printf("\n[Dateisysteme fuer Container-Rootfs]\n");
    printf("  overlay  %s  (A30: aus -> ext4-Image + bind statt overlay)\n", has_fs("overlay")==1?"DA":"fehlt");
    printf("  squashfs %s  (A30: aus -> ext4-Image statt squashfs)\n", has_fs("squashfs")==1?"DA":"fehlt");
    printf("  fuse     %s\n", has_fs("fuse")==1?"DA":"fehlt");
    printf("  tmpfs    %s\n", has_fs("tmpfs")==1?"DA":"fehlt");

    printf("\n[Netz-Isolation]\n");
    node("/sys/class/net"); // nur Existenz; veth-Pruefung nur am Geraet
    printf("  Hinweis: veth im A30-Kernel AUS -> eigenes virtuelles IF per veth nicht direkt;\n");
    printf("  Optionen: (a) TUN-basiertes NAT, (b) Host-Netz NUR mit strenger Firewall (weniger isoliert).\n");

    printf("\n[Ultar-Datenschutz-Kopplung]\n");
    node("/run/ultar/privacy");
    fd=open("/run/ultar/privacy",O_RDONLY);
    if(fd>=0){ char p[256]={0}; int n=read(fd,p,255); (void)n; close(fd); printf("%s", p); }
    else printf("  (noch nicht vorhanden; ultar-shell schreibt es)\n");

    printf("\n[Android-System-Image]\n");
    node("/data/ultar/android/system.img");
    node("/data/ultar/android/data.img");

    printf("\nFazit: siehe docs/A30-ANDROID.md (Sicherheitsmodell + Testreihenfolge).\n");
    return 0;
}
