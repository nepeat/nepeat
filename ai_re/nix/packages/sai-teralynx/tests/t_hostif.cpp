// SPDX-License-Identifier: Apache-2.0
// sai-teralynx test, host interface: netdevs (harness: tests/harness.h).
#include "harness.h"

void test_hostif(Env& e)
{
    USE_ENV(e);
    sai_attribute_t a;
    // ---- hostif
    sai_attribute_t h[3];
    std::memset(h, 0, sizeof(h));
    h[0].id = SAI_HOSTIF_ATTR_TYPE;
    h[0].value.s32 = SAI_HOSTIF_TYPE_NETDEV;
    h[1].id = SAI_HOSTIF_ATTR_OBJ_ID;
    h[1].value.oid = p1;
    h[2].id = SAI_HOSTIF_ATTR_NAME;
    std::snprintf(h[2].value.chardata, sizeof(h[2].value.chardata), "Ethernet0");
    hif = 0;
    OK(ha->create_hostif(&hif, s, 3, h));
    CHECK(has(m_log(), "ifcs_netdev_create h=") && has(m_log(), "name=Ethernet0 devport=1"));
    CHECK(has(m_log(), " tag=1"));            // SAI default STRIP
    // system traps as isai sets them (once, before the first netdev): MTU and
    // L2 source miss off, TTL and IPv4 options on
    CHECK(has(m_log(), "ifcs_hostif_trap_attr_set h=0x18200017 id=1 u32=0"));
    CHECK(has(m_log(), "ifcs_hostif_trap_attr_set h=0x18200010 id=1 u32=0"));
    CHECK(has(m_log(), "ifcs_hostif_trap_attr_set h=0x18200011 id=1 u32=1"));
    CHECK(has(m_log(), "ifcs_hostif_trap_attr_set h=0x18200014 id=1 u32=1"));
    a.id = SAI_HOSTIF_ATTR_OPER_STATUS;
    a.value.booldata = true;
    OK(ha->set_hostif_attribute(hif, &a));

}
