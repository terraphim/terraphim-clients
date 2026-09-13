#include <pthread.h>
#include <stdlib.h>
#include <stdio.h>
static void *w(void*_){ for(int i=0;i<2000;i++){ setenv("ENVPROBE_X","1",1); unsetenv("ENVPROBE_X"); } return 0; }
static void *r(void*_){ volatile char *p; for(int i=0;i<200000;i++){ p=getenv("PATH"); } return 0; }
int main(){ pthread_t a,b; pthread_create(&a,0,w,0); pthread_create(&b,0,r,0); pthread_join(a,0); pthread_join(b,0); puts("ok"); return 0; }
