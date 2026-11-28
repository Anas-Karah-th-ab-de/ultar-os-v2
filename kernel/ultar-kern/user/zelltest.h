/* Shared parts of zelltest (escape tests for the Ultar kernel's process isolation): output, checks,
 * small file helpers and the state both sides (host and cell) use. */
#ifndef ZELLTEST_H
#define ZELLTEST_H
#define _GNU_SOURCE
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <sched.h>
#include <signal.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/mount.h>
#include <sys/prctl.h>
#include <sys/stat.h>
#include <sys/syscall.h>
#include <sys/time.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#define PR_ULTAR_RIGHTS_GET 0x554c5200
#define PR_ULTAR_RIGHTS_DROP 0x554c5201
#define R_MOUNT 1
#define R_NET_ADMIN 2
#define R_KILL_ANY 8
#define R_MANAGE_GROUPS 0x80
#define R_ALL 0x7fff
#define SECRET "/tmp/zell-geheim"

/* number of failed checks in this process */
extern int fails;
/* where the test programs and the file outside the cell live on this system ("/bin" + "/liesmich.txt" on
 * the FAT test image, "/boot/efi/bin" + "/boot/efi/liesmich.txt" when a disk system is the root) */
extern const char *bin_dir, *outside_path;
/* opened by the host before the cell starts, inherited by the cell */
extern int outside_dir, outside_file, host_mnt_ns;
extern pid_t host_parent, host_bystander;
extern int to_parent[2], to_cell[2];

void say(const char *fmt, ...);
/* build a path (rotating static buffers) */
const char *pf(const char *fmt, ...);
void check(const char *name, int ok);
/* the call must have failed with exactly this error */
void denied(const char *name, long r, int want);
int put_file(const char *path, const char *text);
int file_is(const char *path, const char *text);
/* run fn in a child; its failed checks count here */
void sub(int (*fn)(void));
int idle(void);
long rights(void);
/* everything the cell does; returns when pid 1 of the cell is done */
int cell_main(void);
/* after execve inside the cell (argument --rechte): 0 if no right came back */
int after_exec(void);
/* a small second cell (run several at once): returns 0 if it saw only its own things */
int mini_cell(int nr);

#endif
