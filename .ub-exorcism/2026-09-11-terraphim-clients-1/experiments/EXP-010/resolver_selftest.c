// EXP-010 instrument self-test: is a glibc-internal getenv (res_init reading
// RES_OPTIONS / LOCALDOMAIN during getaddrinfo) visible to the LD_PRELOAD shim?
#include <netdb.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
int main(void) {
  struct addrinfo hints, *res = NULL;
  memset(&hints, 0, sizeof hints);
  hints.ai_socktype = 1;
  int rc = getaddrinfo("api.github.com", "443", &hints, &res);
  printf("getaddrinfo rc=%d (%s)\n", rc, rc ? gai_strerror(rc) : "ok");
  if (res) freeaddrinfo(res);
  const char *o = getenv("RES_OPTIONS");
  printf("direct getenv(RES_OPTIONS)=%s\n", o ? o : "(null)");
  return 0;
}
