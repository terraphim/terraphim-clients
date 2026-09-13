// envprobe_linux: LD_PRELOAD shim recording cross-thread overlap between
// setenv/unsetenv (writers) and getenv (readers). Observational only.
// Env: ENVPROBE_WIDEN_US (writer sleeps that long, with the window flagged,
// BEFORE calling the real setenv/unsetenv; 0 = no widening),
// ENVPROBE_LOG (path; default stderr), ENVPROBE_TRACE=1 logs every call.
// Overlap phases: "pre" = reader arrived during the artificial sleep,
// "call" = reader arrived while the real glibc setenv/unsetenv was executing.
#define _GNU_SOURCE
#include <dlfcn.h>
#include <stdlib.h>
#include <stdio.h>
#include <string.h>
#include <stdint.h>
#include <unistd.h>
#include <time.h>
#include <execinfo.h>
#include <stdatomic.h>
#include <sys/syscall.h>

typedef int (*setenv_t)(const char*, const char*, int);
typedef int (*unsetenv_t)(const char*);
typedef char *(*getenv_t)(const char*);
static setenv_t real_setenv; static unsetenv_t real_unsetenv; static getenv_t real_getenv;
static _Atomic(uint64_t) writer_tid = 0;
static _Atomic(int) writer_phase = 0;          // 1 = pre-sleep, 2 = inside real call
static _Atomic(long) n_set = 0, n_unset = 0, n_get = 0, n_overlap_pre = 0, n_overlap_call = 0, n_get_other = 0;
static _Atomic(uint64_t) first_writer_tid = 0;
static _Atomic(int) reported = 0;
static long widen_us = -1; static int trace = -1; static FILE *logf = NULL;
static __thread int in_probe = 0;

static uint64_t tid(void){ return (uint64_t)syscall(SYS_gettid); }
static uint64_t now_ns(void){ struct timespec ts; clock_gettime(CLOCK_MONOTONIC, &ts); return (uint64_t)ts.tv_sec*1000000000ull + ts.tv_nsec; }
static void init(void){
  if (real_getenv) return;
  real_setenv = (setenv_t)dlsym(RTLD_NEXT, "setenv");
  real_unsetenv = (unsetenv_t)dlsym(RTLD_NEXT, "unsetenv");
  real_getenv = (getenv_t)dlsym(RTLD_NEXT, "getenv");
  const char *w = real_getenv("ENVPROBE_WIDEN_US"); widen_us = w ? atol(w) : 0;
  const char *t = real_getenv("ENVPROBE_TRACE"); trace = t ? atoi(t) : 0;
  const char *l = real_getenv("ENVPROBE_LOG"); logf = l ? fopen(l, "a") : NULL; if (!logf) logf = stderr;
}
static void logline(const char *kind, const char *name, uint64_t t, const char *extra){
  fprintf(logf, "E %llu %llu %s %s%s\n", (unsigned long long)now_ns(), (unsigned long long)t, kind, name?name:"?", extra?extra:"");
}
static void dump_bt(const char *name, uint64_t w, uint64_t r, const char *phase){
  if (atomic_fetch_add(&reported,1) >= 12) return;
  void *bt[64]; int n = backtrace(bt, 64);
  fprintf(logf, "[envprobe] OVERLAP(%s): getenv(\"%s\") on tid %llu while writer active on tid %llu\n", phase, name?name:"?", (unsigned long long)r, (unsigned long long)w);
  for (int i=0;i<n;i++){ Dl_info di; if (dladdr(bt[i], &di) && di.dli_fname){ fprintf(logf, "  #%d %s+0x%lx %s\n", i, di.dli_fname, (unsigned long)((char*)bt[i]-(char*)di.dli_fbase), di.dli_sname?di.dli_sname:"?"); } else fprintf(logf, "  #%d %p\n", i, bt[i]); }
  fflush(logf);
}
static void writer_enter(void){ uint64_t me=tid(); uint64_t z=0; atomic_compare_exchange_strong(&first_writer_tid,&z,me); atomic_store(&writer_tid, me);
  if (widen_us>0){ atomic_store(&writer_phase,1); usleep((useconds_t)widen_us); } atomic_store(&writer_phase,2); }
static void writer_exit(void){ atomic_store(&writer_phase,0); atomic_store(&writer_tid,0); }

int setenv(const char *n, const char *v, int o){
  init(); atomic_fetch_add(&n_set,1); if (trace) logline("set", n, tid(), NULL);
  writer_enter(); int rc = real_setenv(n,v,o); writer_exit(); return rc;
}
int unsetenv(const char *n){
  init(); atomic_fetch_add(&n_unset,1); if (trace) logline("unset", n, tid(), NULL);
  writer_enter(); int rc = real_unsetenv(n); writer_exit(); return rc;
}
char *getenv(const char *n){
  if (!real_getenv) init();
  if (in_probe) return real_getenv(n);
  in_probe = 1;
  uint64_t me = tid(); atomic_fetch_add(&n_get,1);
  uint64_t w = atomic_load(&writer_tid); int ph = atomic_load(&writer_phase);
  uint64_t fw = atomic_load(&first_writer_tid);
  const char *extra = NULL;
  if (fw && fw != me) atomic_fetch_add(&n_get_other,1);
  if (w && w != me) {
    if (ph == 2) { atomic_fetch_add(&n_overlap_call,1); extra=" OVERLAP_CALL"; dump_bt(n, w, me, "call"); }
    else { atomic_fetch_add(&n_overlap_pre,1); extra=" OVERLAP_PRE"; dump_bt(n, w, me, "pre"); }
  }
  if (trace || extra) logline("get", n, me, extra);
  in_probe = 0;
  return real_getenv(n);
}
__attribute__((destructor)) static void summary(void){
  init();
  fprintf(logf, "[envprobe] summary: setenv=%ld unsetenv=%ld getenv=%ld getenv_on_other_thread_after_first_write=%ld overlaps_pre=%ld overlaps_call=%ld widen_us=%ld\n",
    atomic_load(&n_set), atomic_load(&n_unset), atomic_load(&n_get), atomic_load(&n_get_other), atomic_load(&n_overlap_pre), atomic_load(&n_overlap_call), widen_us);
  fflush(logf);
}
