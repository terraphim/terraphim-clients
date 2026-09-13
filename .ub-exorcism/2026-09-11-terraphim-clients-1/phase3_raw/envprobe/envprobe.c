// envprobe: DYLD interposition shim recording cross-thread overlap between
// setenv/unsetenv (writers) and getenv (readers). No source changes to the
// project; purely observational. ENVPROBE_WIDEN_US (default 0) makes each
// writer sleep inside its critical region to enlarge a pre-existing window.
#include <stdlib.h>
#include <stdio.h>
#include <string.h>
#include <pthread.h>
#include <unistd.h>
#include <execinfo.h>
#include <stdatomic.h>

#define DYLD_INTERPOSE(_r,_o) __attribute__((used)) static struct{const void*r;const void*o;} _interpose_##_o \
  __attribute__((section("__DATA,__interpose"))) = {(const void*)(unsigned long)&_r,(const void*)(unsigned long)&_o};

static _Atomic(uint64_t) writer_tid = 0;      // 0 = no writer inside setenv/unsetenv
static _Atomic(long) n_set = 0, n_unset = 0, n_get = 0, n_overlap = 0, n_get_other_thread = 0;
static _Atomic(uint64_t) first_writer_tid = 0;
static _Atomic(int) reported = 0;
static long widen_us = -1;

static uint64_t tid(void){ uint64_t t=0; pthread_threadid_np(NULL,&t); return t; }
static long widen(void){ if(widen_us<0){ const char*w=getenv("ENVPROBE_WIDEN_US"); widen_us = w? atol(w):0; } return widen_us; }

static void dump_bt(const char *what, const char *name, uint64_t w, uint64_t r){
  if (atomic_fetch_add(&reported,1) >= 8) return;               // cap output
  void *bt[48]; int n = backtrace(bt, 48);
  fprintf(stderr, "\n[envprobe] OVERLAP: %s(\"%s\") on thread %llu while writer active on thread %llu\n",
          what, name?name:"?", (unsigned long long)r, (unsigned long long)w);
  backtrace_symbols_fd(bt, n, 2);
}

static int my_setenv(const char *n, const char *v, int o){
  uint64_t me = tid(); atomic_fetch_add(&n_set,1);
  uint64_t z=0; atomic_compare_exchange_strong(&first_writer_tid,&z,me);
  atomic_store(&writer_tid, me);
  if (widen()) usleep((useconds_t)widen());
  int rc = setenv(n,v,o);
  atomic_store(&writer_tid, 0);
  return rc;
}
static int my_unsetenv(const char *n){
  uint64_t me = tid(); atomic_fetch_add(&n_unset,1);
  uint64_t z=0; atomic_compare_exchange_strong(&first_writer_tid,&z,me);
  atomic_store(&writer_tid, me);
  if (widen()) usleep((useconds_t)widen());
  int rc = unsetenv(n);
  atomic_store(&writer_tid, 0);
  return rc;
}
static char *my_getenv(const char *n){
  uint64_t me = tid(); atomic_fetch_add(&n_get,1);
  uint64_t w = atomic_load(&writer_tid);
  uint64_t fw = atomic_load(&first_writer_tid);
  if (fw && fw != me) atomic_fetch_add(&n_get_other_thread,1);
  if (w && w != me) { atomic_fetch_add(&n_overlap,1); dump_bt("getenv", n, w, me); }
  return getenv(n);
}
DYLD_INTERPOSE(my_setenv, setenv)
DYLD_INTERPOSE(my_unsetenv, unsetenv)
DYLD_INTERPOSE(my_getenv, getenv)

__attribute__((destructor)) static void summary(void){
  fprintf(stderr, "\n[envprobe] summary: setenv=%ld unsetenv=%ld getenv=%ld getenv_on_other_thread_after_first_write=%ld strict_overlaps=%ld widen_us=%ld\n",
    atomic_load(&n_set), atomic_load(&n_unset), atomic_load(&n_get), atomic_load(&n_get_other_thread), atomic_load(&n_overlap), widen());
}
