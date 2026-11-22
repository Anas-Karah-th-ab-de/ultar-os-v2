/* Prüfprogramm: epoll mit eventfd, timerfd und Rohr (wie es Ereignisschleifen von Rust/Python/Node nutzen). */
#include <stdint.h>
#include <stdio.h>
#include <sys/epoll.h>
#include <sys/eventfd.h>
#include <sys/timerfd.h>
#include <time.h>
#include <unistd.h>
int main(void) {
    int ep = epoll_create1(0), ev = eventfd(0, 0), tf = timerfd_create(CLOCK_MONOTONIC, 0), pf[2];
    pipe(pf);
    struct epoll_event e = {.events = EPOLLIN};
    e.data.u32 = 1; epoll_ctl(ep, EPOLL_CTL_ADD, ev, &e);
    e.data.u32 = 2; epoll_ctl(ep, EPOLL_CTL_ADD, tf, &e);
    e.data.u32 = 3; epoll_ctl(ep, EPOLL_CTL_ADD, pf[0], &e);
    struct itimerspec its = {.it_interval = {0, 100000000}, .it_value = {0, 100000000}};
    timerfd_settime(tf, 0, &its, 0);
    uint64_t one = 5; write(ev, &one, 8);
    write(pf[1], "x", 1);
    int got[4] = {0}, ticks = 0;
    struct timespec t0, t1; clock_gettime(CLOCK_MONOTONIC, &t0);
    while (ticks < 5) {
        struct epoll_event out[8];
        int n = epoll_wait(ep, out, 8, 2000);
        if (n <= 0) { printf("epoll_wait: Zeitablauf\n"); return 1; }
        for (int i = 0; i < n; i++) {
            uint64_t v; char c;
            got[out[i].data.u32]++;
            if (out[i].data.u32 == 1) read(ev, &v, 8);
            if (out[i].data.u32 == 2) { read(tf, &v, 8); ticks += v; }
            if (out[i].data.u32 == 3) read(pf[0], &c, 1);
        }
    }
    clock_gettime(CLOCK_MONOTONIC, &t1);
    long ms = (t1.tv_sec - t0.tv_sec) * 1000 + (t1.tv_nsec - t0.tv_nsec) / 1000000;
    int ok = got[1] == 1 && got[3] == 1 && ticks == 5 && ms >= 450 && ms < 1500;
    printf("epoll: eventfd %d, Rohr %d, timerfd 5 Takte à 100 ms in %ld ms – %s\n", got[1], got[3], ms, ok ? "OK" : "FEHLER");
    return !ok;
}
