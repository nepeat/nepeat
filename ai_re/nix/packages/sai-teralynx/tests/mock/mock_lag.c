/* SPDX-License-Identifier: Apache-2.0
 * Mock libifcs: LAGs and entropy (hash) field lists (mock.h). A member must
 * not be in an L2VNI, behind an mc_dest or the dest of an L2 entry
 * (libifcs' rule; mock_l2_refs in mock_l2.c) nor in another LAG. */
#include "mock.h"

int mock_l2_refs(ifcs_handle_t h);

#define MAX_LAG 64
#define MAX_MEMBERS 32
static struct { int used; ifcs_handle_t h; int n; ifcs_handle_t m[MAX_MEMBERS]; } lags[MAX_LAG];
static struct { int used; ifcs_handle_t h; uint32_t n; uint32_t el[IFCS_EXTENDED_FIELD_MAX]; } efl[MAX_LAG];

static int lag_find(ifcs_handle_t h)
{
    for (int i = 0; i < MAX_LAG; i++)
        if (lags[i].used && lags[i].h == h) return i;
    return -1;
}
static int member_of(ifcs_handle_t sp)
{
    for (int i = 0; i < MAX_LAG; i++)
        for (int j = 0; lags[i].used && j < lags[i].n; j++)
            if (lags[i].m[j] == sp) return i;
    return -1;
}

/* ---------------------------------------------------------- test hooks */
int mock_lag_count(void) { int n = 0; for (int i = 0; i < MAX_LAG; i++) n += lags[i].used; return n; }
/* Members of the LAG with handle h (-1: no such LAG); *first = its first member. */
int mock_lag_members(uint32_t h, uint32_t* first)
{
    int i = lag_find(h);
    if (i < 0) return -1;
    if (first) *first = lags[i].n ? lags[i].m[0] : 0;
    return lags[i].n;
}
/* The elements of the field list with handle h, as "a,b,c" ("" if none). */
const char* mock_efl(uint32_t h)
{
    static char b[256];
    b[0] = 0;
    for (int i = 0; i < MAX_LAG; i++)
        if (efl[i].used && efl[i].h == h)
            for (uint32_t j = 0; j < efl[i].n; j++) snprintf(b + strlen(b), sizeof(b) - strlen(b), "%s%u", j ? "," : "", efl[i].el[j]);
    return b;
}
int mock_efl_count(void) { int n = 0; for (int i = 0; i < MAX_LAG; i++) n += efl[i].used; return n; }
/* 1 if a live field list has exactly these elements ("6,5,7"). */
int mock_efl_find(const char* elements)
{
    for (int i = 0; i < MAX_LAG; i++)
        if (efl[i].used && strcmp(mock_efl(efl[i].h), elements) == 0) return 1;
    return 0;
}

/* ------------------------------------------------------------------ LAG */
ifcs_status_t ifcs_lag_create(ifcs_node_id_t n, ifcs_handle_t* h, uint32_t c, ifcs_attr_t* a)
{
    (void)n; (void)a;
    for (int i = 0; i < MAX_LAG; i++)
        if (!lags[i].used) {
            lags[i].used = 1;
            lags[i].n = 0;
            *h = lags[i].h = 0x28200000u + next_handle++;
            mock_logf("ifcs_lag_create h=0x%x count=%u", *h, c);
            return IFCS_SUCCESS;
        }
    return IFCS_RESOURCE_FULL;
}
ifcs_status_t ifcs_lag_delete(ifcs_node_id_t n, ifcs_handle_t h)
{
    (void)n;
    int i = lag_find(h);
    mock_logf("ifcs_lag_delete h=0x%x", h);
    if (i < 0) return IFCS_NOTFOUND;
    if (lags[i].n || mock_l2_refs(h)) return IFCS_BUSY;
    lags[i].used = 0;
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_lag_member_add(ifcs_node_id_t n, ifcs_handle_t h, uint32_t m, ifcs_handle_t* l, uint32_t c, ifcs_attr_t* a)
{
    (void)n; (void)a;
    int i = lag_find(h);
    mock_logf("ifcs_lag_member_add lag=0x%x n=%u first=0x%x attrs=%u", h, m, m ? l[0] : 0, c);
    if (i < 0) return IFCS_NOTFOUND;
    for (uint32_t j = 0; j < m; j++) {
        if (l[j] == IFCS_HANDLE_SYSPORT(1000)) return IFCS_PARAM;          /* the CPU port */
        if (member_of(l[j]) >= 0) return IFCS_EXIST;
        if (mock_l2_refs(l[j])) return IFCS_BUSY;                          /* part of L2VNI / mc_dest / L2 entry */
    }
    for (uint32_t j = 0; j < m && lags[i].n < MAX_MEMBERS; j++) lags[i].m[lags[i].n++] = l[j];
    return IFCS_SUCCESS;
}
ifcs_status_t ifcs_lag_member_remove(ifcs_node_id_t n, ifcs_handle_t h, uint32_t m, ifcs_handle_t* l)
{
    (void)n;
    int i = lag_find(h);
    mock_logf("ifcs_lag_member_remove lag=0x%x n=%u first=0x%x", h, m, m ? l[0] : 0);
    if (i < 0) return IFCS_NOTFOUND;
    for (uint32_t j = 0; j < m; j++) {
        int k = 0;
        while (k < lags[i].n && lags[i].m[k] != l[j]) k++;
        if (k == lags[i].n) return IFCS_NOTFOUND;
        lags[i].m[k] = lags[i].m[--lags[i].n];
    }
    return IFCS_SUCCESS;
}

/* ---------------------------------------------------------------- hash */
ifcs_status_t ifcs_extended_field_list_create(ifcs_node_id_t n, ifcs_handle_t* h, uint32_t c, ifcs_attr_t* a)
{
    (void)n;
    if (c != 1 || a[0].id != IFCS_EXTENDED_FIELD_LIST_ATTR_ELEMENTS || a[0].value.u32_list.count > IFCS_EXTENDED_FIELD_MAX)
        return IFCS_PARAM;
    for (int i = 0; i < MAX_LAG; i++)
        if (!efl[i].used) {
            efl[i].used = 1;
            *h = efl[i].h = 0x0e200000u + next_handle++;
            efl[i].n = a[0].value.u32_list.count;
            memcpy(efl[i].el, a[0].value.u32_list.arr, efl[i].n * sizeof(uint32_t));
            mock_logf("ifcs_extended_field_list_create h=0x%x elements=%s", *h, mock_efl(*h));
            return IFCS_SUCCESS;
        }
    return IFCS_RESOURCE_FULL;
}
ifcs_status_t ifcs_extended_field_list_delete(ifcs_node_id_t n, ifcs_handle_t h)
{
    (void)n;
    mock_logf("ifcs_extended_field_list_delete h=0x%x", h);
    for (int i = 0; i < MAX_LAG; i++)
        if (efl[i].used && efl[i].h == h) { efl[i].used = 0; return IFCS_SUCCESS; }
    return IFCS_NOTFOUND;
}
