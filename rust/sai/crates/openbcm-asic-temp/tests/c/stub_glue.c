/* SPDX-License-Identifier: Apache-2.0
 * The glue functions asic_temp.c calls, answering as sdk-glue's Rust fake chip does
 * (crates/sdk-glue/src/rust_fake.rs: attach, temperatures, th_overtemp_shutdown, errmsg) from
 * the same scenario file, OPENBCM_ASIC_TEMP_FAKE_SCENARIO. Directives: name, init, temp,
 * temp-fail, "fail overtemp N CODE", comments; anything else fails the attach as the Rust
 * binary refuses it. Only for tests/differential.rs: the unmodified asic_temp.c on this, the
 * Rust binary on the Rust fake, same scenario, same expected bytes. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "sdk_glue.h"

#define MAX_MON 16

static char chip[32] = "FAKE56960_A0";
static char init_fail[16];
static int temps[MAX_MON], ntemps;
static unsigned long temp_fail, temp_calls;
static unsigned long ot_n, ot_seen;
static int ot_code, attached, pll_down;

const char* ob_sdk_errmsg(int rv)
{
    static const char* text[20] = {
        "Ok", "Internal error", "Out of memory", "Invalid unit", "Invalid parameter", "Table empty",
        "Table full", "Entry not found", "Entry exists", "Operation timed out", "Operation still running",
        "Operation failed", "Operation disabled", "Invalid identifier", "No resources for operation",
        "Invalid configuration", "Feature unavailable", "Feature not initialized", "Invalid port",
        "Unknown error"};
    if (rv == OB_E_MDIO_CLOCK) return "MDIO clock setup not as configured";
    return text[(rv <= 0 && rv > -19) ? -rv : 19];
}

/* a whole word as a decimal number (Rust's str::parse) */
static int dec(const char* w, long* v)
{
    char* end;
    if (!*w) return 0;
    *v = strtol(w, &end, 10);
    return *end == 0;
}

static int directive(char* line)
{
    char* w[MAX_MON + 2];
    int n = 0, i;
    long v, c;
    for (char* t = strtok(line, " \t\r"); t; t = strtok(NULL, " \t\r")) {
        if (n == MAX_MON + 2) return 0;
        w[n++] = t;
    }
    if (n == 2 && !strcmp(w[0], "name")) { snprintf(chip, sizeof(chip), "%s", w[1]); return 1; }
    if (n == 2 && !strcmp(w[0], "init")) { snprintf(init_fail, sizeof(init_fail), "%s", w[1]); return 1; }
    if (n == 2 && !strcmp(w[0], "temp-fail") && dec(w[1], &v) && v >= 0) { temp_fail = v; return 1; }
    if (n >= 2 && n - 1 <= MAX_MON && !strcmp(w[0], "temp")) {
        for (i = 1; i < n; i++) {
            if (!dec(w[i], &v)) return 0;
            temps[i - 1] = (int)v;
        }
        ntemps = n - 1;
        return 1;
    }
    if (n == 4 && !strcmp(w[0], "fail") && !strcmp(w[1], "overtemp") && dec(w[2], &v) && dec(w[3], &c) && v > 0 && c < 0) {
        ot_n = v; ot_code = (int)c; return 1;
    }
    return 0;
}

int ob_sdk_attach(int unit, char* err, int errlen)
{
    const char* path = getenv("OPENBCM_ASIC_TEMP_FAKE_SCENARIO");
    char line[512];
    int no = 0;
    FILE* f = path ? fopen(path, "r") : NULL;
    if (path && !f) { snprintf(err, errlen, "fake scenario %s: cannot open", path); return OB_E_CONFIG; }
    while (f && fgets(line, sizeof(line), f)) {
        char* t = line;
        no++;
        line[strcspn(line, "\n")] = 0;
        while (*t == ' ' || *t == '\t') t++;
        if (!*t || *t == '#') continue;
        if (!directive(t)) {
            snprintf(err, errlen, "fake glue: %s:%d: not a scenario line", path, no);
            fclose(f);
            return OB_E_CONFIG;
        }
    }
    if (f) fclose(f);
    if (!strcmp(init_fail, "busy")) {
        snprintf(err, errlen, "another SDK process owns the switch (lock /run/openbcm-unit%d.lock held; e.g. systemctl stop openbcm-asic-temp)", unit);
        return OB_E_BUSY;
    }
    if (unit != 0 || !strcmp(init_fail, "unit")) {
        snprintf(err, errlen, "no attached switch unit (soc_ndev 1)");
        return OB_E_UNIT;
    }
    if (*init_fail && strcmp(init_fail, "soc") && strcmp(init_fail, "misc") && strcmp(init_fail, "delay") && strcmp(init_fail, "divisor")) {
        snprintf(err, errlen, "fake glue: unknown init failure '%s'", init_fail);
        return OB_E_CONFIG;
    }
    attached = 1;
    snprintf(err, errlen, "attached %s", chip);
    return OB_E_NONE;
}

int ob_sdk_temperatures(int unit, int* cur, int* peak, int max, int* count)
{
    int i;
    *count = 0;
    if (max > MAX_MON) max = MAX_MON;
    if (max <= 0) return OB_E_PARAM;
    if (!attached || unit != 0) return OB_E_UNIT;
    temp_calls++;
    if (pll_down || (temp_fail != 0 && temp_calls >= temp_fail)) return OB_E_TIMEOUT;
    if (ntemps == 0) return OB_E_UNAVAIL;
    for (i = 0; i < ntemps && i < max; i++) {
        cur[i] = temps[i];
        peak[i] = temps[i] > 0x7fffffff - 15 ? 0x7fffffff : temps[i] + 15;
    }
    *count = i;
    return OB_E_NONE;
}

int ob_sdk_th_overtemp_shutdown(int unit)
{
    if (!attached || unit != 0) return OB_E_UNIT;
    if (ot_n && ++ot_seen == ot_n) return ot_code;
    pll_down = 1;
    return OB_E_NONE;
}
