/* SPDX-License-Identifier: Apache-2.0
 * ASIC temperature files, see temp_files.h. */
#include "temp_files.h"

#include <errno.h>
#include <stdarg.h>
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>

int ob_temp_plausible(const int* cur, int n)
{
    int i;
    if (n <= 0) return 0;
    for (i = 0; i < n; i++)
        if (cur[i] < -400 || cur[i] > 1500) return 0;
    return 1;
}

int ob_temp_hottest(const int* cur, int n, long* sum)
{
    int i, hot = -1;
    long s = 0;
    for (i = 0; i < n; i++) {
        if (hot < 0 || cur[i] > cur[hot]) hot = i;
        s += cur[i];
    }
    if (sum) *sum = s;
    return hot;
}

static int mkparent(const char* path)
{
    char dir[4096];
    const char* s = strrchr(path, '/');
    size_t n;
    if (!s || s == path) return -1;
    n = (size_t)(s - path);
    if (n >= sizeof(dir)) return -1;
    memcpy(dir, path, n);
    dir[n] = 0;
    return (mkdir(dir, 0755) == 0 || errno == EEXIST) ? 0 : -1;
}

static int write_atomic(const char* path, const char* text)
{
    char tmp[4096];
    FILE* f;
    int e;
    if (snprintf(tmp, sizeof(tmp), "%s.tmp", path) >= (int)sizeof(tmp)) {
        errno = ENAMETOOLONG;
        return -1;
    }
    f = fopen(tmp, "w");
    if (!f && errno == ENOENT && mkparent(path) == 0) f = fopen(tmp, "w");
    if (!f) return -1;
    if (fputs(text, f) == EOF) {
        e = errno;
        fclose(f);
        unlink(tmp);
        errno = e;
        return -1;
    }
    if (fclose(f) != 0 || rename(tmp, path) != 0) {
        e = errno;
        unlink(tmp);
        errno = e;
        return -1;
    }
    return 0;
}

/* Append to a fixed buffer; *len stays <= cap - 1, *full set on truncation. */
static void app(char* b, size_t cap, size_t* len, int* full, const char* fmt, ...) __attribute__((format(printf, 5, 6)));
static void app(char* b, size_t cap, size_t* len, int* full, const char* fmt, ...)
{
    va_list ap;
    int r;
    if (*full) return;
    va_start(ap, fmt);
    r = vsnprintf(b + *len, cap - *len, fmt, ap);
    va_end(ap);
    if (r < 0 || (size_t)r >= cap - *len) *full = 1;
    else *len += (size_t)r;
}

int ob_temp_write(const char* out, const char* prom, int unit, const int* cur, const int* peak, int n)
{
    char line[64], text[8192];
    size_t len = 0;
    int i, full = 0, res = 0, err = 0, hot = ob_temp_hottest(cur, n, NULL);
    if (hot < 0) {
        errno = EINVAL;
        return (out ? OB_TEMP_W_OUT : 0) | (prom ? OB_TEMP_W_PROM : 0);
    }
    if (out) {
        snprintf(line, sizeof(line), "%.1f\n", cur[hot] / 10.0);
        if (write_atomic(out, line) != 0) {
            res |= OB_TEMP_W_OUT;
            err = errno;
        }
    }
    if (prom) {
        app(text, sizeof(text), &len, &full,
            "# HELP switch_asic_temperature_celsius ASIC die temperature monitor, current\n"
            "# TYPE switch_asic_temperature_celsius gauge\n");
        for (i = 0; i < n; i++)
            app(text, sizeof(text), &len, &full, "switch_asic_temperature_celsius{unit=\"%d\",sensor=\"pvtmon%d\"} %.1f\n",
                unit, i, cur[i] / 10.0);
        app(text, sizeof(text), &len, &full,
            "# HELP switch_asic_temperature_peak_celsius ASIC die temperature monitor, highest since the previous read\n"
            "# TYPE switch_asic_temperature_peak_celsius gauge\n");
        for (i = 0; i < n; i++)
            app(text, sizeof(text), &len, &full,
                "switch_asic_temperature_peak_celsius{unit=\"%d\",sensor=\"pvtmon%d\"} %.1f\n", unit, i,
                peak[i] / 10.0);
        app(text, sizeof(text), &len, &full,
            "# HELP switch_asic_temperature_max_celsius hottest current ASIC monitor (what fan control reads)\n"
            "# TYPE switch_asic_temperature_max_celsius gauge\n"
            "switch_asic_temperature_max_celsius{unit=\"%d\"} %.1f\n"
            "# HELP switch_asic_temperature_read_timestamp_seconds time of the last good read\n"
            "# TYPE switch_asic_temperature_read_timestamp_seconds gauge\n"
            "switch_asic_temperature_read_timestamp_seconds{unit=\"%d\"} %ld\n",
            unit, cur[hot] / 10.0, unit, (long)time(NULL));
        if (full) {
            res |= OB_TEMP_W_PROM;
            err = ENOSPC;
        } else if (write_atomic(prom, text) != 0) {
            res |= OB_TEMP_W_PROM;
            err = errno;
        }
    }
    if (res) errno = err;
    return res;
}

void ob_temp_remove(const char* out, const char* prom)
{
    if (out) unlink(out);
    if (prom) unlink(prom);
}
