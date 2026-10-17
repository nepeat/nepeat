/*
 * OpenCL motion-search probe: exhaustive +-R full-pel 16x16 SAD search for
 * every macroblock of a frame (cur vs ref, two frames of a y4m). Reports GPU
 * ms/frame and the same search on one CPU core (SSE2 psadbw), to see whether
 * offloading motion estimation to a small GPU is worth it.
 *
 * usage: clme <in.y4m> <range> [iterations]   (PLAT=rusticl|clover)
 * build: gcc -O2 -msse2 -o clme clme.c -lOpenCL
 */
#define CL_TARGET_OPENCL_VERSION 120
#include <CL/cl.h>
#include <emmintrin.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

static const char *src =
"__kernel void me(__global const uchar *cur, __global const uchar *ref,\n"
"                 int w, int h, int R, __global int *out)\n"
"{\n"
"    __local uchar c[256];\n"
"    __local uchar r[64 * 64];\n"
"    __local int best[64];\n"
"    int mx = get_group_id(0), my = get_group_id(1), l = get_local_id(0);\n"
"    int x0 = mx * 16, y0 = my * 16, S = 16 + 2 * R, n = 2 * R + 1;\n"
"    for (int i = l; i < 256; i += 64)\n"
"        c[i] = cur[(y0 + i / 16) * w + x0 + i % 16];\n"
"    for (int i = l; i < S * S; i += 64) {\n"
"        int x = clamp(x0 - R + i % S, 0, w - 1), y = clamp(y0 - R + i / S, 0, h - 1);\n"
"        r[i] = ref[y * w + x];\n"
"    }\n"
"    barrier(CLK_LOCAL_MEM_FENCE);\n"
"    int b = 0x7fffffff;\n"
"    for (int k = l; k < n * n; k += 64) {\n"
"        int dx = k % n, dy = k / n, s = 0;\n"
"        for (int y = 0; y < 16; y++)\n"
"            for (int x = 0; x < 16; x++)\n"
"                s += abs((int)c[y * 16 + x] - (int)r[(dy + y) * S + dx + x]);\n"
"        b = min(b, (s << 11) | k);\n"
"    }\n"
"    best[l] = b;\n"
"    barrier(CLK_LOCAL_MEM_FENCE);\n"
"    for (int o = 32; o; o >>= 1) {\n"
"        if (l < o) best[l] = min(best[l], best[l + o]);\n"
"        barrier(CLK_LOCAL_MEM_FENCE);\n"
"    }\n"
"    if (!l) out[my * get_num_groups(0) + mx] = best[0];\n"
"}\n";

static double now(void)
{
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return t.tv_sec + t.tv_nsec * 1e-9;
}

static int sad16(const unsigned char *a, int as, const unsigned char *b, int bs)
{
    __m128i acc = _mm_setzero_si128();
    for (int y = 0; y < 16; y++)
        acc = _mm_add_epi64(acc, _mm_sad_epu8(_mm_loadu_si128((const __m128i *)(a + y * as)),
                                              _mm_loadu_si128((const __m128i *)(b + y * bs))));
    return _mm_cvtsi128_si32(acc) + _mm_extract_epi16(acc, 4);
}

/* y4m: read the luma of frames 0 and 1 */
static int load(const char *fn, unsigned char **f0, unsigned char **f1, int *w, int *h)
{
    FILE *f = fopen(fn, "rb");
    char line[256];
    if (!f || !fgets(line, sizeof(line), f))
        return -1;
    sscanf(strstr(line, " W") + 2, "%d", w);
    sscanf(strstr(line, " H") + 2, "%d", h);
    size_t y = (size_t)*w * *h, fr = y * 3 / 2;
    unsigned char *tmp = malloc(fr);
    *f0 = malloc(y); *f1 = malloc(y);
    for (int i = 0; i < 2; i++) {
        if (!fgets(line, sizeof(line), f) || fread(tmp, 1, fr, f) != fr)
            return -1;
        memcpy(i ? *f1 : *f0, tmp, y);
    }
    free(tmp);
    fclose(f);
    return 0;
}

#define CK(x) do { cl_int e_ = (x); if (e_) { fprintf(stderr, "%s: %d\n", #x, e_); return 1; } } while (0)

int main(int argc, char **argv)
{
    if (argc < 3) {
        fprintf(stderr, "usage: %s in.y4m range [iters]\n", argv[0]);
        return 1;
    }
    int R = atoi(argv[2]), iters = argc > 3 ? atoi(argv[3]) : 20, w, h;
    unsigned char *cur, *ref;
    if (R < 1 || R > 24 || load(argv[1], &ref, &cur, &w, &h))
        return 1;
    int mbw = w / 16, mbh = h / 16, nmb = mbw * mbh, n = 2 * R + 1;

    /* pick the platform */
    cl_platform_id plats[8], p = NULL;
    cl_uint np;
    CK(clGetPlatformIDs(8, plats, &np));
    const char *want = getenv("PLAT") ? getenv("PLAT") : "rusticl";
    for (cl_uint i = 0; i < np; i++) {
        char name[128];
        clGetPlatformInfo(plats[i], CL_PLATFORM_NAME, sizeof(name), name, NULL);
        if (strcasestr(name, want))
            p = plats[i];
    }
    if (!p)
        return fprintf(stderr, "no platform %s\n", want), 1;
    cl_device_id d;
    CK(clGetDeviceIDs(p, CL_DEVICE_TYPE_GPU, 1, &d, NULL));
    char dn[256];
    clGetDeviceInfo(d, CL_DEVICE_NAME, sizeof(dn), dn, NULL);
    cl_int e;
    cl_context ctx = clCreateContext(NULL, 1, &d, NULL, NULL, &e); CK(e);
    cl_command_queue q = clCreateCommandQueue(ctx, d, CL_QUEUE_PROFILING_ENABLE, &e); CK(e);
    cl_program prog = clCreateProgramWithSource(ctx, 1, &src, NULL, &e); CK(e);
    if (clBuildProgram(prog, 1, &d, "", NULL, NULL)) {
        char log[8192];
        clGetProgramBuildInfo(prog, d, CL_PROGRAM_BUILD_LOG, sizeof(log), log, NULL);
        return fprintf(stderr, "%s\n", log), 1;
    }
    cl_kernel k = clCreateKernel(prog, "me", &e); CK(e);
    size_t ysz = (size_t)w * h;
    cl_mem bc = clCreateBuffer(ctx, CL_MEM_READ_ONLY | CL_MEM_COPY_HOST_PTR, ysz, cur, &e); CK(e);
    cl_mem br = clCreateBuffer(ctx, CL_MEM_READ_ONLY | CL_MEM_COPY_HOST_PTR, ysz, ref, &e); CK(e);
    cl_mem bo = clCreateBuffer(ctx, CL_MEM_WRITE_ONLY, nmb * 4, NULL, &e); CK(e);
    CK(clSetKernelArg(k, 0, sizeof(bc), &bc)); CK(clSetKernelArg(k, 1, sizeof(br), &br));
    CK(clSetKernelArg(k, 2, 4, &w)); CK(clSetKernelArg(k, 3, 4, &h));
    CK(clSetKernelArg(k, 4, 4, &R)); CK(clSetKernelArg(k, 5, sizeof(bo), &bo));
    size_t gs[2] = { (size_t)mbw * 64, (size_t)mbh }, ls[2] = { 64, 1 };

    int *gpu = malloc(nmb * 4);
    double best = 1e9;
    for (int it = 0; it < iters + 1; it++) {
        cl_event ev;
        CK(clEnqueueNDRangeKernel(q, k, 2, NULL, gs, ls, 0, NULL, &ev));
        CK(clWaitForEvents(1, &ev));
        cl_ulong t0, t1;
        clGetEventProfilingInfo(ev, CL_PROFILING_COMMAND_START, 8, &t0, NULL);
        clGetEventProfilingInfo(ev, CL_PROFILING_COMMAND_END, 8, &t1, NULL);
        if (it && (t1 - t0) * 1e-6 < best)   /* first run warms up */
            best = (t1 - t0) * 1e-6;
        clReleaseEvent(ev);
    }
    CK(clEnqueueReadBuffer(q, bo, CL_TRUE, 0, nmb * 4, gpu, 0, NULL, NULL));

    /* the same search on one CPU core */
    int mism = 0;
    double c0 = now();
    for (int my = 0; my < mbh; my++)
        for (int mx = 0; mx < mbw; mx++) {
            int b = 0x7fffffff;
            for (int kk = 0; kk < n * n; kk++) {
                int x = mx * 16 - R + kk % n, y = my * 16 - R + kk / n;
                if (x < 0 || y < 0 || x + 16 > w || y + 16 > h)
                    continue;   /* GPU clamps; compare interior MBs only */
                int s = sad16(cur + my * 16 * w + mx * 16, w, ref + y * w + x, w);
                if (((s << 11) | kk) < b)
                    b = (s << 11) | kk;
            }
            int interior = mx * 16 >= R && my * 16 >= R && mx * 16 + 16 + R <= w && my * 16 + 16 + R <= h;
            if (interior && b != gpu[my * mbw + mx])
                mism++;
        }
    double cpu = (now() - c0) * 1e3;
    double ops = (double)nmb * n * n * 256;
    printf("%s, %dx%d, +-%d (%d candidates/MB): GPU %.2f ms/frame (%.1f Gabsdiff/s), "
           "1 CPU core SSE2 %.2f ms/frame, interior mismatches %d\n",
           dn, w, h, R, n * n, best, ops / best * 1e-6, cpu, mism);
    return 0;
}
