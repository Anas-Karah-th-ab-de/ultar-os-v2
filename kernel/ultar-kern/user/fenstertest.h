/* fenstertest: shared declarations of fenstertest.c and fenstertest-geraete.c */
#ifndef FENSTERTEST_H
#define FENSTERTEST_H

#define PR_ULTAR_RIGHTS_GET 0x554c5200
#define PR_ULTAR_RIGHTS_DROP 0x554c5201
#define SEAT_GROUP 812

extern int fails;
void say(const char *fmt, ...);
void check(const char *name, int ok);
void denied(const char *name, long r, int want);
long now_us(void);

void rights_checks(void);
void fb_checks(void);
void devnode_checks(void);
void input_checks(void);

#endif
