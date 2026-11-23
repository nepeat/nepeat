/* SPDX-License-Identifier: Apache-2.0
 * Hardware smoke test of the Rust libsai.so with the openbcm backend: loads the library with
 * dlopen and drives the slice through the C ABI with the real SAI headers.
 *
 *   sai-openbcm-smoke --sonic-is-stopped --libsai <libsai.so> --profile <sai.profile>
 *                     [--expect-ports N] [--oper I,J,...] [--admin I] [--wait SECONDS]
 *                     [--vlan VID] [--backend NAME]
 *
 * CREATING THE SWITCH RESETS THE SWITCH CHIP ("init all"). The program therefore refuses to
 * run without --sonic-is-stopped, and refuses when the unit's SDK lock
 * (/run/openbcm-unit<N>.lock) is held by another process (syncd, openbcm-asic-temp,
 * bcm.user, the PHY bring-up).
 *
 * --profile     key=value lines (SONiC's sai.profile, as the C++ sai-openbcm reads it). The
 *               program adds SAI_RS_BACKEND=<NAME> (default openbcm).
 * Ports are named by their index in the switch's port list (0 = first, printed in the table).
 * --expect-ports N   fail unless the switch has N front-panel ports
 * --oper I,J,...     print the oper status of these ports once more at the end
 * --admin I          set port I admin up, wait --wait seconds (default 3) for oper up (reported,
 *                    not required), set it back to what it was and check that it reads back
 * --vlan VID         create VLAN VID, add port --admin (or port 0) untagged, check, remove both
 * Every front-panel port is admin down when the program ends, as the library creates them;
 * the VLAN is removed, the switch removed, the API uninitialised. Exit 0 and "SMOKE_OK" on
 * success, 1 on a failed check (after the same clean-up), 2 on usage or refusal (nothing was
 * touched). */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/file.h>
#include <time.h>
#include <unistd.h>

#include <sai.h>

#define MAX_PORTS 512
#define MAX_KEYS 128

static int checks, failures;
#define CHECK(c, ...)                                 \
    do {                                              \
        checks++;                                     \
        if (!(c)) {                                   \
            failures++;                               \
            fprintf(stderr, "FAIL %s:%d: ", __FILE__, __LINE__); \
            fprintf(stderr, __VA_ARGS__);             \
            fprintf(stderr, "\n");                    \
        }                                             \
    } while (0)
#define OK(expr) CHECK((st = (expr)) == SAI_STATUS_SUCCESS, "%s = %d", #expr, (int)st)

/* ---- the profile ---- */
static struct { char* key; char* value; } profile[MAX_KEYS];
static int nprofile;
static const char* backend = "openbcm";

static const char* profile_get_value(sai_switch_profile_id_t profile_id, const char* variable)
{
    (void)profile_id;
    if (!strcmp(variable, "SAI_RS_BACKEND")) return backend;
    for (int i = 0; i < nprofile; i++)
        if (!strcmp(profile[i].key, variable)) return profile[i].value;
    return NULL;
}
static int profile_get_next_value(sai_switch_profile_id_t profile_id, const char** variable, const char** value)
{
    (void)profile_id; (void)variable; (void)value;
    return -1;
}

static int load_profile(const char* path)
{
    char line[1024];
    FILE* f = fopen(path, "r");
    if (!f) {
        fprintf(stderr, "cannot read the profile %s: %s\n", path, strerror(errno));
        return -1;
    }
    while (fgets(line, sizeof(line), f)) {
        char* eq;
        line[strcspn(line, "\r\n")] = 0;
        if (!line[0] || line[0] == '#') continue;
        eq = strchr(line, '=');
        if (!eq || nprofile == MAX_KEYS) {
            fprintf(stderr, "%s: not a key=value line (or too many): %s\n", path, line);
            fclose(f);
            return -1;
        }
        *eq = 0;
        profile[nprofile].key = strdup(line);
        profile[nprofile].value = strdup(eq + 1);
        if (!profile[nprofile].key || !profile[nprofile].value) {
            fclose(f);
            return -1;
        }
        nprofile++;
    }
    fclose(f);
    return 0;
}

/* 0: nobody holds the unit's SDK lock (or there is no lock file: no SDK process ran since
 * boot). The lock is released again at once; the library takes it itself. */
static int sdk_lock_free(void)
{
    char path[64];
    const char* unit = profile_get_value(0, "SAI_OPENBCM_UNIT");
    int fd, busy;
    snprintf(path, sizeof(path), "/run/openbcm-unit%d.lock", unit ? atoi(unit) : 0);
    fd = open(path, O_RDWR | O_CLOEXEC);
    if (fd < 0) {
        if (errno == ENOENT) return 0;
        fprintf(stderr, "cannot open %s: %s\n", path, strerror(errno));
        return -1;
    }
    busy = flock(fd, LOCK_EX | LOCK_NB) != 0;
    close(fd);   /* releases it */
    if (busy) {
        fprintf(stderr, "refusing: another SDK process holds %s (syncd? openbcm-asic-temp? bcm.user?).\n"
                        "Stop it first: systemctl stop sonic.target openbcm-asic-temp\n", path);
        return -1;
    }
    return 0;
}

/* ---- notifications ---- */
static pthread_mutex_t ev_lock = PTHREAD_MUTEX_INITIALIZER;
static int nevents;
static void on_port_state(uint32_t count, const sai_port_oper_status_notification_t* data)
{
    pthread_mutex_lock(&ev_lock);
    for (uint32_t i = 0; i < count; i++) {
        nevents++;
        printf("EVENT port 0x%016llx oper %s\n", (unsigned long long)data[i].port_id,
               data[i].port_state == SAI_PORT_OPER_STATUS_UP ? "up" : "down");
    }
    fflush(stdout);
    pthread_mutex_unlock(&ev_lock);
}

static void nap(long ms)
{
    struct timespec ts = {ms / 1000, (ms % 1000) * 1000000L};
    nanosleep(&ts, NULL);
}

static int usage(void)
{
    fprintf(stderr, "usage: sai-openbcm-smoke --sonic-is-stopped --libsai <libsai.so> --profile <sai.profile>\n"
                    "         [--expect-ports N] [--oper I,J,...] [--admin I] [--wait SECONDS] [--vlan VID] [--backend NAME]\n"
                    "Creating the switch RESETS THE SWITCH CHIP: stop SONiC and every SDK process first, then pass\n"
                    "--sonic-is-stopped. Ports are indices into the switch's port list.\n");
    return 2;
}

#define SYM(name)                                                      \
    __typeof__(name)* p_##name = (__typeof__(name)*)dlsym(lib, #name); \
    if (!p_##name) {                                                   \
        fprintf(stderr, "missing symbol %s\n", #name);                 \
        return 2;                                                      \
    }

int main(int argc, char** argv)
{
    const char *libsai = NULL, *profile_path = NULL, *oper = NULL;
    int confirmed = 0, expect_ports = -1, admin = -1, vlan_id = -1, wait_s = 3;
    sai_status_t st;

    for (int i = 1; i < argc; i++) {
        const char* a = argv[i];
        const char* v = i + 1 < argc ? argv[i + 1] : NULL;
        if (!strcmp(a, "--sonic-is-stopped")) confirmed = 1;
        else if (!strcmp(a, "--libsai") && v) libsai = v, i++;
        else if (!strcmp(a, "--profile") && v) profile_path = v, i++;
        else if (!strcmp(a, "--backend") && v) backend = v, i++;
        else if (!strcmp(a, "--oper") && v) oper = v, i++;
        else if (!strcmp(a, "--expect-ports") && v) expect_ports = atoi(v), i++;
        else if (!strcmp(a, "--admin") && v) admin = atoi(v), i++;
        else if (!strcmp(a, "--vlan") && v) vlan_id = atoi(v), i++;
        else if (!strcmp(a, "--wait") && v) wait_s = atoi(v), i++;
        else return usage();
    }
    if (!libsai || !profile_path) return usage();
    if (vlan_id != -1 && (vlan_id < 2 || vlan_id > 4094)) {
        fprintf(stderr, "--vlan: 2..4094\n");
        return 2;
    }
    if (wait_s < 0 || wait_s > 120) wait_s = 3;
    if (!confirmed) {
        fprintf(stderr, "refusing: creating the switch resets the switch chip. Stop SONiC and every other SDK process\n"
                        "(systemctl stop sonic.target openbcm-asic-temp), then pass --sonic-is-stopped.\n");
        return 2;
    }
    if (load_profile(profile_path) != 0) return 2;
    if (sdk_lock_free() != 0) return 2;

    void* lib = dlopen(libsai, RTLD_NOW | RTLD_LOCAL);
    if (!lib) {
        fprintf(stderr, "dlopen: %s\n", dlerror());
        return 2;
    }
    SYM(sai_api_initialize)
    SYM(sai_api_uninitialize)
    SYM(sai_api_query)

    const sai_service_method_table_t services = {profile_get_value, profile_get_next_value};
    sai_switch_api_t* switch_api = NULL;
    sai_port_api_t* port_api = NULL;
    sai_vlan_api_t* vlan_api = NULL;
    sai_bridge_api_t* bridge_api = NULL;
    OK(p_sai_api_initialize(0, &services));
    if (failures) return 1;
    OK(p_sai_api_query(SAI_API_SWITCH, (void**)&switch_api));
    OK(p_sai_api_query(SAI_API_PORT, (void**)&port_api));
    OK(p_sai_api_query(SAI_API_VLAN, (void**)&vlan_api));
    OK(p_sai_api_query(SAI_API_BRIDGE, (void**)&bridge_api));
    if (failures || !switch_api || !port_api || !vlan_api || !bridge_api) {
        p_sai_api_uninitialize();
        return 1;
    }

    /* ---- the switch: this is "init all" ---- */
    sai_object_id_t sw = SAI_NULL_OBJECT_ID;
    sai_attribute_t create[2];
    memset(create, 0, sizeof(create));
    create[0].id = SAI_SWITCH_ATTR_INIT_SWITCH;
    create[0].value.booldata = true;
    create[1].id = SAI_SWITCH_ATTR_PORT_STATE_CHANGE_NOTIFY;
    create[1].value.ptr = (void*)on_port_state;
    printf("creating the switch (backend %s): the switch chip is reset now\n", backend);
    fflush(stdout);
    OK(switch_api->create_switch(&sw, 2, create));
    if (failures) {
        fprintf(stderr, "create_switch failed: see the library's lines above (sai-rs openbcm: ...)\n");
        p_sai_api_uninitialize();
        return 1;
    }

    static sai_object_id_t ports[MAX_PORTS];
    static bool admin_was[MAX_PORTS];
    uint32_t nports = 0;
    sai_object_id_t vlan = SAI_NULL_OBJECT_ID, member = SAI_NULL_OBJECT_ID;
    {
        sai_attribute_t g[3];
        memset(g, 0, sizeof(g));
        g[0].id = SAI_SWITCH_ATTR_PORT_NUMBER;
        g[1].id = SAI_SWITCH_ATTR_PORT_LIST;
        g[1].value.objlist.count = MAX_PORTS;
        g[1].value.objlist.list = ports;
        g[2].id = SAI_SWITCH_ATTR_CPU_PORT;
        OK(switch_api->get_switch_attribute(sw, 3, g));
        if (failures) goto cleanup;
        nports = g[1].value.objlist.count;
        CHECK(g[0].value.u32 == nports, "PORT_NUMBER %u, PORT_LIST has %u", g[0].value.u32, nports);
        printf("switch 0x%016llx: %u front-panel ports, CPU port 0x%016llx\n", (unsigned long long)sw, nports,
               (unsigned long long)g[2].value.oid);
        if (expect_ports >= 0) CHECK((int)nports == expect_ports, "%u ports, expected %d", nports, expect_ports);
        CHECK(nports > 0, "no ports");
    }
    printf("%5s %-18s %-16s %7s %5s %4s\n", "index", "port", "lanes", "speed", "admin", "oper");
    for (uint32_t i = 0; i < nports; i++) {
        uint32_t lanes[16];
        char text[64] = "";
        sai_attribute_t g[4];
        memset(g, 0, sizeof(g));
        g[0].id = SAI_PORT_ATTR_HW_LANE_LIST;
        g[0].value.u32list.count = 16;
        g[0].value.u32list.list = lanes;
        g[1].id = SAI_PORT_ATTR_SPEED;
        g[2].id = SAI_PORT_ATTR_ADMIN_STATE;
        g[3].id = SAI_PORT_ATTR_OPER_STATUS;
        OK(port_api->get_port_attribute(ports[i], 4, g));
        if (st != SAI_STATUS_SUCCESS) continue;
        for (uint32_t l = 0, at = 0; l < g[0].value.u32list.count && l < 16 && at < sizeof(text) - 8; l++)
            at += (uint32_t)snprintf(text + at, sizeof(text) - at, "%s%u", l ? "," : "", lanes[l]);
        admin_was[i] = g[2].value.booldata;
        /* the library creates every front-panel port admin down */
        CHECK(!admin_was[i], "port %u is admin up right after create_switch", i);
        printf("%5u 0x%016llx %-16s %7u %5s %4s\n", i, (unsigned long long)ports[i], text, g[1].value.u32,
               g[2].value.booldata ? "up" : "down", g[3].value.s32 == SAI_PORT_OPER_STATUS_UP ? "up" : "down");
    }
    fflush(stdout);

    /* ---- admin up and back on one port ---- */
    if (admin >= 0) {
        CHECK((uint32_t)admin < nports, "--admin %d: the switch has %u ports", admin, nports);
        if ((uint32_t)admin < nports) {
            sai_attribute_t set = {.id = SAI_PORT_ATTR_ADMIN_STATE, .value.booldata = true};
            sai_attribute_t get = {.id = SAI_PORT_ATTR_OPER_STATUS};
            int up = 0;
            OK(port_api->set_port_attribute(ports[admin], &set));
            for (int ms = 0; st == SAI_STATUS_SUCCESS && ms <= wait_s * 1000 && !up; ms += 100) {
                OK(port_api->get_port_attribute(ports[admin], 1, &get));
                up = get.value.s32 == SAI_PORT_OPER_STATUS_UP;
                if (!up) nap(100);
            }
            printf("ADMIN port %d up: oper %s within %d s (not a failure: needs a link partner and the PHY's line side on)\n",
                   admin, up ? "UP" : "down", wait_s);
            set.value.booldata = admin_was[admin];
            OK(port_api->set_port_attribute(ports[admin], &set));
            get.id = SAI_PORT_ATTR_ADMIN_STATE;
            OK(port_api->get_port_attribute(ports[admin], 1, &get));
            CHECK(get.value.booldata == admin_was[admin], "port %d: admin state not restored", admin);
            nap(300);   /* let the link-down notification arrive before the next step prints */
        }
    }

    /* ---- a VLAN with one member ---- */
    if (vlan_id > 0 && nports > 0) {
        uint32_t idx = admin >= 0 && (uint32_t)admin < nports ? (uint32_t)admin : 0;
        static sai_object_id_t bps[MAX_PORTS], members[MAX_PORTS];
        sai_object_id_t bridge_port = SAI_NULL_OBJECT_ID;
        sai_attribute_t g = {.id = SAI_SWITCH_ATTR_DEFAULT_1Q_BRIDGE_ID};
        OK(switch_api->get_switch_attribute(sw, 1, &g));
        sai_attribute_t l = {.id = SAI_BRIDGE_ATTR_PORT_LIST};
        l.value.objlist.count = MAX_PORTS;
        l.value.objlist.list = bps;
        OK(bridge_api->get_bridge_attribute(g.value.oid, 1, &l));
        for (uint32_t i = 0; st == SAI_STATUS_SUCCESS && i < l.value.objlist.count; i++) {
            sai_attribute_t bp = {.id = SAI_BRIDGE_PORT_ATTR_PORT_ID};
            if (bridge_api->get_bridge_port_attribute(bps[i], 1, &bp) == SAI_STATUS_SUCCESS && bp.value.oid == ports[idx])
                bridge_port = bps[i];
        }
        CHECK(bridge_port != SAI_NULL_OBJECT_ID, "no bridge port for port %u", idx);
        sai_attribute_t v = {.id = SAI_VLAN_ATTR_VLAN_ID, .value.u16 = (sai_vlan_id_t)vlan_id};
        OK(vlan_api->create_vlan(&vlan, sw, 1, &v));
        if (st != SAI_STATUS_SUCCESS) vlan = SAI_NULL_OBJECT_ID;
        if (vlan != SAI_NULL_OBJECT_ID && bridge_port != SAI_NULL_OBJECT_ID) {
            sai_attribute_t m[3];
            memset(m, 0, sizeof(m));
            m[0].id = SAI_VLAN_MEMBER_ATTR_VLAN_ID;
            m[0].value.oid = vlan;
            m[1].id = SAI_VLAN_MEMBER_ATTR_BRIDGE_PORT_ID;
            m[1].value.oid = bridge_port;
            m[2].id = SAI_VLAN_MEMBER_ATTR_VLAN_TAGGING_MODE;
            m[2].value.s32 = SAI_VLAN_TAGGING_MODE_UNTAGGED;
            OK(vlan_api->create_vlan_member(&member, sw, 3, m));
            if (st != SAI_STATUS_SUCCESS) member = SAI_NULL_OBJECT_ID;
            sai_attribute_t ml = {.id = SAI_VLAN_ATTR_MEMBER_LIST};
            ml.value.objlist.count = MAX_PORTS;
            ml.value.objlist.list = members;
            OK(vlan_api->get_vlan_attribute(vlan, 1, &ml));
            CHECK(ml.value.objlist.count == 1 && members[0] == member, "VLAN %d: %u members", vlan_id, ml.value.objlist.count);
            printf("VLAN %d created with port %u untagged\n", vlan_id, idx);
        }
    }

    if (oper) {
        char* list = strdup(oper);
        for (char* tok = list ? strtok(list, ",") : NULL; tok; tok = strtok(NULL, ",")) {
            int idx = atoi(tok);
            sai_attribute_t g = {.id = SAI_PORT_ATTR_OPER_STATUS};
            CHECK(idx >= 0 && (uint32_t)idx < nports, "--oper %d: the switch has %u ports", idx, nports);
            if (idx < 0 || (uint32_t)idx >= nports) continue;
            OK(port_api->get_port_attribute(ports[idx], 1, &g));
            printf("OPER port %d: %s\n", idx, g.value.s32 == SAI_PORT_OPER_STATUS_UP ? "up" : "down");
        }
        free(list);
    }

cleanup:
    /* Whatever failed above: members and VLAN go, every port back to admin down, the switch is removed. */
    if (member != SAI_NULL_OBJECT_ID) OK(vlan_api->remove_vlan_member(member));
    if (vlan != SAI_NULL_OBJECT_ID) OK(vlan_api->remove_vlan(vlan));
    for (uint32_t i = 0; i < nports; i++) {
        sai_attribute_t g = {.id = SAI_PORT_ATTR_ADMIN_STATE};
        if (port_api->get_port_attribute(ports[i], 1, &g) != SAI_STATUS_SUCCESS || g.value.booldata != admin_was[i]) {
            sai_attribute_t set = {.id = SAI_PORT_ATTR_ADMIN_STATE, .value.booldata = admin_was[i]};
            OK(port_api->set_port_attribute(ports[i], &set));
        }
    }
    OK(switch_api->remove_switch(sw));
    OK(p_sai_api_uninitialize());
    pthread_mutex_lock(&ev_lock);
    printf("%d link notifications\n", nevents);
    pthread_mutex_unlock(&ev_lock);
    if (failures) {
        printf("SMOKE_FAILED %d of %d checks\n", failures, checks);
        return 1;
    }
    printf("SMOKE_OK %d checks, %u ports, all admin down, switch removed\n", checks, nports);
    return 0;
}
