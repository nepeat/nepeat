/* SPDX-License-Identifier: Apache-2.0
 * The ASIC temperature files that fan control (fand) and monitoring read.
 * Written by openbcm-asic-temp (attach-only reader, src/asic_temp.c) and by
 * libsai's publisher thread while syncd owns the chip (src/sai_temp.cpp):
 * one implementation, so the two cannot drift. Temperatures are tenths of a
 * degree C, as ob_sdk_temperatures returns them. Plain C, no SDK headers. */
#ifndef SAI_OPENBCM_TEMP_FILES_H
#define SAI_OPENBCM_TEMP_FILES_H

#ifdef __cplusplus
extern "C" {
#endif

#define OB_TEMP_MAX_MON 16
#define OB_TEMP_W_OUT 1     /* ob_temp_write: FILE could not be written */
#define OB_TEMP_W_PROM 2    /* ob_temp_write: PROM could not be written */

/* 1 if there is at least one monitor and every one is within -40..150 C. */
int ob_temp_plausible(const int* cur, int n);
/* Index of the hottest current monitor (-1 if n <= 0); *sum = sum of all. */
int ob_temp_hottest(const int* cur, int n, long* sum);
/* FILE: the hottest current monitor in C ("41.8\n"); PROM (may be NULL):
 * Prometheus textfile, switch_asic_* gauges. Each replaced atomically
 * (FILE.tmp + rename); a missing parent directory is created (one level,
 * e.g. /run/asic after systemd removed the reader's RuntimeDirectory).
 * Returns 0 or OB_TEMP_W_* bits, errno from the last failure. */
int ob_temp_write(const char* out, const char* prom, int unit, const int* cur, const int* peak, int n);
/* Remove both (NULL skipped), so consumers see "no reading", not an old one. */
void ob_temp_remove(const char* out, const char* prom);

#ifdef __cplusplus
}
#endif
#endif
