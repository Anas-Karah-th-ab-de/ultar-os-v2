/* Shared parts of grenztest – tests for the resource groups of the Ultar kernel (plan §3.7, stage 2):
 * limits, dynamic protection, and that running out of memory never stops the machine. */
#ifndef GRENZTEST_H
#define GRENZTEST_H
#define _GNU_SOURCE
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <sched.h>
#include <signal.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/mount.h>
#include <sys/prctl.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/syscall.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#define PR_ULTAR_RIGHTS_GET 0x554c5200
#define PR_ULTAR_RIGHTS_DROP 0x554c5201
#define R_MANAGE_GROUPS 0x80UL
#define R_ALL 0x3fffUL
#define CG "/sys/fs/cgroup"
#define MB (1L << 20)
#ifndef F_SETPIPE_SZ
#define F_SETPIPE_SZ 1031
#define F_GETPIPE_SZ 1032
#endif

/* scratch buffers shared by the tests (one copy keeps the program image, which every fork copies, small) */
extern unsigned char block[1 << 20], back[1 << 20];
/* number of failed checks in this process */
extern int fails;
/* directory of the test programs ("/bin" on the FAT test image, "/boot/efi/bin" with a disk as root) */
extern const char *bin_dir;

void say(const char *fmt, ...);
/* build a path (rotating static buffers) */
const char *pf(const char *fmt, ...);
void check(const char *name, int ok);
/* the call must have failed with exactly this error */
void denied(const char *name, long r, int want);
/* first number in a file; -1 if it cannot be read, -2 for "max" */
long num_at(const char *path);
/* number after `key` (a word at the start of a line or after a space) in a file; -1 if missing */
long keyed_num(const char *path, const char *key);
/* write text to a file; 0 or the negative error number of open/write */
int put_text(const char *path, const char *text);
/* free memory of the machine in KiB (/proc/meminfo) */
long free_kb(void);
/* milliseconds on the monotonic clock */
long now_ms(void);
void nap_ms(long ms);
/* map and touch `mb` MiB; NULL if the mapping fails (a refused page ends the process instead) */
char *eat(long mb);
/* wait for a child for at most `ms`; returns 1 and the status, 0 on timeout */
int wait_ms(pid_t pid, int *status, long ms);
/* start a cell (new mount + pid namespace) running fn(arg); the child never returns */
pid_t cell(int (*fn)(long), long arg);
/* inside a cell: private root on a tmpfs with /bin (read-only), /proc, /dev and – if `small` is given –
 * a second tmpfs at /klein mounted with these options; then drop every right */
int enter_root(const char *small);
/* take 1 MiB of memory with a call that is refused (ENOMEM) instead of ending the process: a
 * populated file mapping. `fd`: any open regular file. Returns 1 if the memory was granted. */
int grab_mb(int fd);
/* memory charged to a group path below CG ("" = root) */
long group_mem(const char *group);
/* wait until the kernel has released everything of ended cells: `cells` back at `base` bytes and no
 * removed group left with memory; returns 1 on success */
int settled(long base);
/* run a normal program (hallo-glibc) and return 1 if it ended with success */
int normal_program_runs(void);

/* sections */
void positive_tests(void);
void limit_tests(void);
void attack_tests(void);
void dynamic_tests(void);

#endif
