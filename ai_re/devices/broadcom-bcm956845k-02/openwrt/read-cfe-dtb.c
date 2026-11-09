/* SPDX-License-Identifier: MIT
 * Read the CFE-generated DTB left in RAM by the factory Linux boot.
 * This program opens /dev/mem read-only and prints a framed hex capture.
 */
#include <arpa/inet.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>

int main(void)
{
    const off_t base = 0x00ffc000;
    uint32_t header[2];
    int fd = open("/dev/mem", O_RDONLY);
    if (fd < 0) { perror("open /dev/mem"); return 1; }
    if (pread(fd, header, sizeof(header), base) != sizeof(header)) {
        perror("read DTB header"); close(fd); return 1;
    }
    size_t size = ntohl(header[1]);
    if (ntohl(header[0]) != 0xd00dfeed || size < 40 || size > 65536) {
        fprintf(stderr, "No valid DTB at 0x00ffc000: %08x %08x\n",
                ntohl(header[0]), ntohl(header[1]));
        close(fd); return 1;
    }
    unsigned char *data = malloc(size);
    if (!data || pread(fd, data, size, base) != (ssize_t)size) {
        perror("read DTB"); free(data); close(fd); return 1;
    }
    close(fd);
    printf("CFE_DTB_BEGIN %zu\n", size);
    for (size_t i = 0; i < size; i++) {
        printf("%02x", data[i]);
        if (i % 32 == 31 || i == size - 1) putchar('\n');
    }
    puts("CFE_DTB_END");
    free(data);
    return 0;
}
