/* SPDX-License-Identifier: Apache-2.0
 * Mock of the ipd driver's sFlow ioctls for tl_sflow.cpp: the backend calls
 * tl_ipd_ioctl when the loaded libifcs exports it (only this mock does).
 * Records one line per ioctl; mock_ipd_fail(nr, errno) makes the next call
 * with that nr fail (nr 0: none). ifcs_cpu_queue_attr_set (the sample queue's meter,
 * tl_sflow) is here too: one line per attribute. */
#include "mock.h"

#include <errno.h>
#include <stdint.h>
#include <sys/ioctl.h>

static unsigned fail_nr;
static int fail_errno;
static int calls;

void mock_ipd_fail(unsigned nr, int err)
{
    fail_nr = nr;
    fail_errno = err;
}

int mock_ipd_calls(void) { return calls; }
void mock_sflow_egress(int on) { g_egress_sflow = on; }   /* as if the init had set node EGRESS_SFLOW_ENABLE */

ifcs_status_t ifcs_cpu_queue_attr_set(ifcs_node_id_t n, ifcs_cpu_queue_t q, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    for (uint32_t i = 0; i < c; i++) mock_logf("ifcs_cpu_queue_attr_set q=%u id=%u u32=%u", q, a[i].id, a[i].value.u32);
    return q < 48 ? IFCS_SUCCESS : IFCS_PARAM;
}

int tl_ipd_ioctl(int fd, unsigned long req, void* arg)
{
    (void)fd;
    const uint8_t* b = arg;
    unsigned nr = _IOC_NR(req);
    uint16_t version;
    calls++;
    memcpy(&version, b, 2);
    if (_IOC_TYPE(req) != 'I' || version != 1) {
        mock_logf("ipd_ioctl BAD type=%c version=%u", (int)_IOC_TYPE(req), version);
        errno = EINVAL;
        return -1;
    }
    switch (nr) {
    case 26: {
        uint16_t id, sp;
        uint32_t rate;
        memcpy(&id, b + 8, 2);
        memcpy(&sp, b + 10, 2);
        memcpy(&rate, b + 12, 4);
        mock_logf("ipd_ioctl SET_NETDEV id=%u sysport=%u rate=%u", id, sp, rate);
        break;
    }
    case 27:
    case 28: {
        uint32_t type, app;
        memcpy(&type, b + 8, 4);
        memcpy(&app, b + 12, 4);
        mock_logf("ipd_ioctl %s type=%u app=%u name=%.16s mcgrp=%.15s", nr == 27 ? "CREATE_NETLINK" : "DELETE_NETLINK", type, app,
                  (const char*)b + 16, (const char*)b + 33);
        break;
    }
    case 31: {
        uint16_t proto;
        memcpy(&proto, b + 8, 2);
        mock_logf("ipd_ioctl SET_PARAMS shim_gre_proto=0x%04x", proto);
        break;
    }
    default:
        mock_logf("ipd_ioctl nr=%u", nr);
    }
    if (fail_nr && nr == fail_nr) {
        fail_nr = 0;
        errno = fail_errno;
        return -1;
    }
    return 0;
}
