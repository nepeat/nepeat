// SPDX-License-Identifier: Apache-2.0
// libipd-devmem.so under LD_PRELOAD (native x86_64): copies into and out of a
// "device" mapping (IPD_DEVMEM_TEST_PATH) take the aligned path and give the
// same bytes as glibc for every alignment and length; other memory is untouched.
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>
#include <dlfcn.h>

int main(void) {
    int (*hits)(const void *, size_t) = (int (*)(const void *, size_t))dlsym(RTLD_DEFAULT, "ipd_devmem_hits");
    if (!hits) { puts("preload not loaded"); return 1; }
    int fd = open(getenv("IPD_DEVMEM_TEST_PATH"), O_RDWR);
    unsigned char *dev = mmap(NULL, 8192, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    static unsigned char src[512], ref[512], back[512];
    int bad = 0;
    for (int i = 0; i < 512; i++) src[i] = (unsigned char)(i * 7 + 3);
    if (!hits(dev, 1) || !hits(dev + 8191, 1) || hits(src, sizeof src)) { puts("hits wrong"); return 1; }
    for (int d = 0; d < 16; d++)
        for (int s = 0; s < 16; s++)
            for (int n = 0; n < 80; n++) {
                memset(dev, 0xee, 256);
                memset(ref, 0xee, 256);
                memcpy(dev + d, src + s, n);
                for (int k = 0; k < n; k++) ref[d + k] = src[s + k];
                if (memcmp(dev, ref, 256)) bad++;
                memset(back, 0, sizeof back);
                memcpy(back + s, dev + d, n);
                if (memcmp(back + s, src + s, n)) bad++;
                memset(dev + d, 0x5a, n);
                for (int k = 0; k < n; k++) if (dev[d + k] != 0x5a) { bad++; break; }
            }
    for (int i = 0; i < 256; i++) dev[i] = (unsigned char)i;
    memmove(dev + 3, dev, 100);          /* overlapping, backwards */
    for (int i = 0; i < 100; i++) if (dev[3 + i] != (unsigned char)i) { bad++; break; }
    for (int i = 0; i < 256; i++) dev[i] = (unsigned char)i;
    memmove(dev, dev + 5, 100);          /* overlapping, forwards */
    for (int i = 0; i < 100; i++) if (dev[i] != (unsigned char)(i + 5)) { bad++; break; }
    munmap(dev + 4096, 4096);
    if (hits(dev + 4096, 1) || !hits(dev, 1)) bad++;
    printf("%s (%d mismatches)\n", bad ? "FAIL" : "devmem ok", bad);
    return bad != 0;
}
