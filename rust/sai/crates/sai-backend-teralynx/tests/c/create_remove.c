/* SPDX-License-Identifier: Apache-2.0
 * The IFCS call log of create_switch and remove_switch on the mock libifcs (sai-teralynx
 * tests/mock), for a libsai given on the command line: the C++ sai-teralynx one and the Rust one
 * print the same, line for line, or the call sequences differ (nix/libsai-teralynx.nix diffs them).
 * The profile and files are test_ifcs.cpp's main(). Between the two: the temperature list five
 * times and once more (round-robin, then cached), MAX_TEMP and AVAILABLE_IPV4_ROUTE_ENTRY (table
 * usage), and the learn-ring guard's state (installed by the loader's GOT redirects).
 * Usage: create_remove <libsai.so> <mock libifcs.so, absolute> <scratch dir, absolute> [regs file [watch file]]
 * With a watch file too (SAI_TERALYNX_DIAG_REGS): after the first create, the diag socket's "regs"
 * answer (the list's registers read now, then the watch list's), each line prefixed "diag: ".
 * With a regs file (SAI_TERALYNX_POST_INIT_REGS, the Rust library only): instead, three
 * create / remove rounds printing the mock's registers 0x13514 0x13530 0x13538 0x1353c after each
 * create: as written; re-applied after the registers were changed in between (read-modify-write
 * keeps the bits outside each mask); with two all-ones reads injected on 0x13530 (a read-back
 * mismatch, logged as an error; the switch is created all the same). */
#include <sai.h>

#include <dlfcn.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/un.h>
#include <unistd.h>

/* one diag socket request, the answer printed line by line with a prefix */
static void diag(const char* path, const char* cmd)
{
    int fd = socket(AF_UNIX, SOCK_STREAM, 0);
    struct sockaddr_un sa;
    memset(&sa, 0, sizeof(sa));
    sa.sun_family = AF_UNIX;
    snprintf(sa.sun_path, sizeof(sa.sun_path), "%s", path);
    if (fd < 0 || connect(fd, (struct sockaddr*)&sa, sizeof(sa)) != 0) {
        printf("diag: no socket\n");
        if (fd >= 0) close(fd);
        return;
    }
    if (write(fd, cmd, strlen(cmd)) < 0) printf("diag: write failed\n");
    shutdown(fd, SHUT_WR);
    char buf[8192];
    size_t len = 0;
    ssize_t n;
    while (len < sizeof(buf) - 1 && (n = read(fd, buf + len, sizeof(buf) - 1 - len)) > 0) len += (size_t)n;
    buf[len] = 0;
    close(fd);
    for (char* line = strtok(buf, "\n"); line; line = strtok(NULL, "\n")) printf("diag: %s\n", line);
}

static const char* keys[16][2];
static int nkeys, iter;
static const char* get(sai_switch_profile_id_t id, const char* k)
{
    (void)id;
    for (int i = 0; i < nkeys; i++)
        if (!strcmp(keys[i][0], k)) return keys[i][1];
    return NULL;
}
static int next(sai_switch_profile_id_t id, const char** k, const char** v)
{
    (void)id;
    if (!k || !v) { iter = 0; return 0; }
    if (iter >= nkeys) return -1;
    *k = keys[iter][0];
    *v = keys[iter][1];
    iter++;
    return 0;
}
static void put(const char* dir, const char* name, const char* text, char* path)
{
    snprintf(path, 512, "%s/%s", dir, name);
    FILE* f = fopen(path, "w");
    if (!f) { perror(path); exit(2); }
    fputs(text, f);
    fclose(f);
}
static void set(const char* k, const char* v) { keys[nkeys][0] = k; keys[nkeys][1] = v; nkeys++; }

int main(int argc, char** argv)
{
    if (argc < 4 || argc > 6) { fprintf(stderr, "usage: create_remove libsai.so mock.so dir [regs [watch]]\n"); return 2; }
    mkdir(argv[3], 0755);
    static char init[512], isg[512], map[512], c1[512], c2[512], cmds[1100], run[512];
    put(argv[3], "init.yaml", "# generated init config\nifcs:\n  options:\n    log_level: \"info\"\n", init);
    put(argv[3], "isg.yaml", "  isg:\n  - id: \"0\"\n", isg);
    put(argv[3], "ports.map", "# devport lanes speed fec\n1 249,250,251,252,253,254,255,256 400000 rs\n"
                              "2 241,242,243,244,245,246,247,248 400000 rs   # cage 2\n3 121 10000 none\n", map);
    put(argv[3], "a.cmd", "node create 0\nifcs set devport 1 rx_eq_fine_tune 1\nifcs set devport 1 tx_eq_pre1 [2,2,2,2,2,2,2,2]\r\n"
                          "ifcs set devport 2 rx_ctle_lf [1, 2, 3, 4]\npci write led_cfg sck_rate_f 0x8\npci write led_refresh_cliff 0x020\n", c1);
    put(argv[3], "b.cmd", "ifcs set devport 3 rx_gainshape1 [5]\nifcs set devport 1 link_training 1\n"
                          "ifcs set devport 1 bogus_attr 3\nport enable 1\n# comment\n", c2);
    snprintf(cmds, sizeof(cmds), "%s:%s", c1, c2);
    snprintf(run, sizeof(run), "%s/run", argv[3]);
    set("SAI_OPENBCM_BACKEND", "openbcm");
    set("SAI_INIT_CONFIG_FILE", init);
    set("SAI_TERALYNX_LIBIFCS", argv[2]);
    set("SAI_TERALYNX_RUN_DIR", run);
    set("SAI_TERALYNX_PORT_MAP", map);
    set("SAI_TERALYNX_CONFIG_APPEND", isg);
    set("SAI_TERALYNX_INIT_CMDS", cmds);
    set("SAI_TERALYNX_LINK_STATUS", "cached");
    set("SAI_TERALYNX_NETDEV", "create");
    set("SAI_TERALYNX_HASH_FIELDS", "src_ip,dst_ip,ip_protocol,l4_src_port,l4_dst_port,in_port");
    set("SAI_TERALYNX_HASH_SEED", "10");
    set("SAI_TERALYNX_SFLOW_CPU_QUEUE", "2");
    set("SAI_TERALYNX_SFLOW_METER", "on");
    static char sock[512];
    if (argc >= 5) set("SAI_TERALYNX_POST_INIT_REGS", argv[4]);
    if (argc == 6) {
        set("SAI_TERALYNX_DIAG_REGS", argv[5]);
        snprintf(sock, sizeof(sock), "%s/diag.sock", argv[3]);
        if (strlen(sock) < 100) set("SAI_OPENBCM_DIAG_SOCKET", sock);
    }

    void* lib = dlopen(argv[1], RTLD_NOW | RTLD_GLOBAL);
    if (!lib) { fprintf(stderr, "%s\n", dlerror()); return 2; }
    sai_status_t (*init_api)(uint64_t, const sai_service_method_table_t*) = (sai_status_t (*)(uint64_t, const sai_service_method_table_t*))dlsym(lib, "sai_api_initialize");
    sai_status_t (*query)(sai_api_t, void**) = (sai_status_t (*)(sai_api_t, void**))dlsym(lib, "sai_api_query");
    sai_status_t (*uninit)(void) = (sai_status_t (*)(void))dlsym(lib, "sai_api_uninitialize");
    static sai_service_method_table_t smt = {get, next};
    if (init_api(0, &smt) != SAI_STATUS_SUCCESS) return 3;
    sai_switch_api_t* sw = NULL;
    query(SAI_API_SWITCH, (void**)&sw);
    sai_attribute_t ca[1];
    memset(ca, 0, sizeof(ca));
    ca[0].id = SAI_SWITCH_ATTR_INIT_SWITCH;
    ca[0].value.booldata = true;
    sai_object_id_t s = 0;
    sai_status_t st = sw->create_switch(&s, 1, ca);
    printf("create_switch: %d\n", st);
    void* mock = dlopen(argv[2], RTLD_NOW | RTLD_NOLOAD);
    if (!mock) { printf("mock not loaded\n"); return 4; }
    const char* (*mlog)(void) = (const char* (*)(void))dlsym(mock, "mock_ifcs_log");
    void (*mclear)(void) = (void (*)(void))dlsym(mock, "mock_ifcs_log_clear");
    int (*temp_reads)(void) = (int (*)(void))dlsym(mock, "mock_temp_reads");
    int (*inited)(void) = (int (*)(void))dlsym(mock, "mock_inited");
    const char* (*guard)(void) = (const char* (*)(void))dlsym(RTLD_DEFAULT, "tl_ifcs_guard_text");
    if (argc >= 5) {
        uint32_t (*reg_get)(int32_t) = (uint32_t (*)(int32_t))dlsym(mock, "mock_reg_get");
        void (*reg_set)(int32_t, uint32_t) = (void (*)(int32_t, uint32_t))dlsym(mock, "mock_reg_set");
        void (*reg_fail)(int32_t, unsigned) = (void (*)(int32_t, unsigned))dlsym(mock, "mock_reg_fail");
        static const int32_t offs[4] = {0x13514, 0x13530, 0x13538, 0x1353c};
        for (int round = 0; round < 3; round++) {
            if (round) {
                st = sw->create_switch(&s, 1, ca);
                printf("create_switch %d: %d\n", round + 1, st);
            }
            printf("regs %d:", round + 1);
            for (int i = 0; i < 4; i++) printf(" %#x=%#x", offs[i], reg_get(offs[i]));
            printf("\n");
            if (round == 0 && argc == 6) {
                if (strlen(sock) < 100) diag(sock, "regs\n");
                else printf("diag: SKIPPED (scratch dir too long for a unix socket)\n");
            }
            st = sw->remove_switch(s);
            printf("remove_switch %d: %d\n", round + 1, st);
            if (round == 0) { reg_set(0x13514, 0xffff0000u); reg_set(0x1353c, 0x5); reg_set(0x13530, 0x1); }
            if (round == 1) reg_fail(0x13530, 2);
        }
        printf("sai_api_uninitialize: %d\n", uninit());
        return 0;
    }
    printf("== create_switch IFCS calls\n%s", mlog());
    mclear();
    int32_t temps[8];
    sai_attribute_t a;
    for (int i = 0; i < 6; i++) {
        a.id = SAI_SWITCH_ATTR_TEMP_LIST;
        a.value.s32list.count = 8;
        a.value.s32list.list = temps;
        st = sw->get_switch_attribute(s, 1, &a);
        printf("TEMP_LIST %d: %d, %u monitors, reads %d:", i, st, a.value.s32list.count, temp_reads());
        for (uint32_t j = 0; st == SAI_STATUS_SUCCESS && j < a.value.s32list.count; j++) printf(" %d", temps[j]);
        printf("\n");
    }
    a.id = SAI_SWITCH_ATTR_MAX_TEMP;
    st = sw->get_switch_attribute(s, 1, &a);
    printf("MAX_TEMP: %d %d\n", st, a.value.s32);
    a.id = SAI_SWITCH_ATTR_AVAILABLE_IPV4_ROUTE_ENTRY;
    st = sw->get_switch_attribute(s, 1, &a);
    printf("AVAILABLE_IPV4_ROUTE_ENTRY: %d %u\n", st, a.value.u32);
    a.id = SAI_SWITCH_ATTR_AVAILABLE_FDB_ENTRY;
    st = sw->get_switch_attribute(s, 1, &a);
    printf("AVAILABLE_FDB_ENTRY: %d %u\n", st, a.value.u32);
    printf("== get IFCS calls\n%s", mlog());
    if (guard) {
        const char* g = guard();
        const char* inst = g ? strstr(g, "\nsai_teralynx_ifcs_guard_installed ") : NULL;
        const char* end = inst ? strchr(inst + 1, '\n') : NULL;
        if (inst && end) printf("guard: %.*s\n", (int)(end - inst - 1), inst + 1);
        else printf("guard: (no installed line)\n");
    } else {
        printf("guard: no tl_ifcs_guard_text\n");
    }
    mclear();
    st = sw->remove_switch(s);
    printf("remove_switch: %d\n== remove_switch IFCS calls\n%s", st, mlog());
    printf("mock_inited: %d\n", inited());
    printf("sai_api_uninitialize: %d\n", uninit());
    return 0;
}
