// EXP-010 follow-up instrument: over-approximating window detector.
// Interposes setenv/unsetenv (what std::env::set_var calls on glibc) and
// getaddrinfo. Logs whenever a setenv/unsetenv runs while some other thread is
// inside getaddrinfo (or a getaddrinfo starts while a writer is active).
// It cannot see the resolver read itself (glibc-internal getenv bypasses the
// PLT), so a logged WINDOW is a necessary condition for the race, not proof.
#define _GNU_SOURCE
#include <dlfcn.h>
#include <netdb.h>
#include <pthread.h>
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/syscall.h>
#include <unistd.h>
typedef int (*setenv_t)(const char*, const char*, int);
typedef int (*unsetenv_t)(const char*);
typedef int (*gai_t)(const char*, const char*, const struct addrinfo*, struct addrinfo**);
static setenv_t real_setenv; static unsetenv_t real_unsetenv; static gai_t real_gai;
static atomic_int in_gai = 0, writers = 0;
static atomic_long n_set = 0, n_gai = 0, n_win_set = 0, n_win_gai = 0;
static long widen_us = 0; static FILE *logf;
static void init(void){
  if (real_gai) return;
  real_setenv = (setenv_t)dlsym(RTLD_NEXT, "setenv");
  real_unsetenv = (unsetenv_t)dlsym(RTLD_NEXT, "unsetenv");
  real_gai = (gai_t)dlsym(RTLD_NEXT, "getaddrinfo");
  const char *w = getenv("GAIPROBE_WIDEN_US"); widen_us = w ? atol(w) : 0;
  const char *l = getenv("GAIPROBE_LOG"); logf = l ? fopen(l, "a") : NULL; if (!logf) logf = stderr;
}
static long tid(void){ return syscall(SYS_gettid); }
static void writer(const char *op, const char *n){
  atomic_fetch_add(&writers, 1); atomic_fetch_add(&n_set, 1);
  if (widen_us) usleep(widen_us);
  int g = atomic_load(&in_gai);
  if (g > 0) { atomic_fetch_add(&n_win_set, 1); fprintf(logf, "[gaiprobe] WINDOW(setenv): %s(\"%s\") on tid %ld while %d thread(s) inside getaddrinfo\n", op, n, tid(), g); fflush(logf); }
}
static void writer_done(void){ atomic_fetch_sub(&writers, 1); }
int setenv(const char *n, const char *v, int o){ if(!real_gai) init(); writer("setenv", n); int rc = real_setenv(n,v,o); writer_done(); return rc; }
int unsetenv(const char *n){ if(!real_gai) init(); writer("unsetenv", n); int rc = real_unsetenv(n); writer_done(); return rc; }
int getaddrinfo(const char *node, const char *svc, const struct addrinfo *h, struct addrinfo **res){
  if(!real_gai) init();
  atomic_fetch_add(&n_gai, 1);
  int w = atomic_load(&writers);
  if (w > 0) { atomic_fetch_add(&n_win_gai, 1); fprintf(logf, "[gaiprobe] WINDOW(getaddrinfo): getaddrinfo(\"%s\") on tid %ld while %d writer(s) active\n", node?node:"?", tid(), w); fflush(logf); }
  atomic_fetch_add(&in_gai, 1);
  int rc = real_gai(node, svc, h, res);
  atomic_fetch_sub(&in_gai, 1);
  return rc;
}
__attribute__((destructor)) static void summary(void){
  if (!logf) return;
  fprintf(logf, "[gaiprobe] summary: setenv+unsetenv=%ld getaddrinfo=%ld windows_setenv_during_gai=%ld windows_gai_during_setenv=%ld widen_us=%ld\n",
    atomic_load(&n_set), atomic_load(&n_gai), atomic_load(&n_win_set), atomic_load(&n_win_gai), widen_us);
  fflush(logf);
}
