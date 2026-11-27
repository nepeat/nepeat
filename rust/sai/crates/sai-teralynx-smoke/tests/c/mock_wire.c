/* SPDX-License-Identifier: Apache-2.0
 * mock-wire: cables for the mock libifcs (the teralynx mock, rust/sai/tests/teralynx-mock), for the
 * smoke program's test on the mock (crate sai-teralynx-smoke, tests/mock-smoke.sh). LD_PRELOADed: a thread
 * finds the mock's test hooks once the library under test has loaded it (RTLD_GLOBAL), then every 5 ms
 * looks at the admin state of the wired devports (mock_admin) and fires the mock's link scan event for
 * both ends of a pair (mock_fire_link) when the pair's state changes: up when both ends are admin up,
 * down otherwise. Nothing of the SAI is called; the smoke program sees link events and LINK_STATUS as
 * from a cable.
 *   MOCK_WIRE   pairs "A-B,C-D,..." (unset or empty: no cables) */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

typedef uint32_t (*admin_fn)(uint32_t);
typedef int (*fire_fn)(uint32_t, int);

#define MAX_PAIRS 32
static struct { uint32_t a, b; int up; } pairs[MAX_PAIRS];
static int npairs;

static void* wire(void* arg)
{
    (void)arg;
    admin_fn admin = NULL;
    fire_fn fire = NULL;
    for (;;) {
        usleep(5000);
        if (!admin || !fire) {
            admin = (admin_fn)dlsym(RTLD_DEFAULT, "mock_admin");
            fire = (fire_fn)dlsym(RTLD_DEFAULT, "mock_fire_link");
            continue;
        }
        for (int i = 0; i < npairs; i++) {
            int want = admin(pairs[i].a) == 1 && admin(pairs[i].b) == 1;
            if (want == pairs[i].up) continue;
            /* no link scan registered (before init, after remove_switch): try again later */
            if (fire(pairs[i].a, want) != 0) continue;
            fire(pairs[i].b, want);
            pairs[i].up = want;
        }
    }
    return NULL;
}

__attribute__((constructor)) static void start(void)
{
    const char* s = getenv("MOCK_WIRE");
    if (!s || !*s) return;
    char buf[512];
    snprintf(buf, sizeof(buf), "%s", s);
    char* save = NULL;
    for (char* t = strtok_r(buf, ",", &save); t && npairs < MAX_PAIRS; t = strtok_r(NULL, ",", &save)) {
        unsigned a, b;
        if (sscanf(t, "%u-%u", &a, &b) == 2) {
            pairs[npairs].a = a;
            pairs[npairs].b = b;
            pairs[npairs].up = 0;
            npairs++;
        }
    }
    pthread_t th;
    if (pthread_create(&th, NULL, wire, NULL) == 0) pthread_detach(th);
}
