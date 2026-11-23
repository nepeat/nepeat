// SPDX-License-Identifier: Apache-2.0
// The test port: the ONLY way tests/test_sai.cpp (and its .inc files) reach past the SAI C ABI.
//
// test_sai.cpp drives a libsai through the SAI C ABI (sai_api_initialize, sai_api_query, the
// API tables, sai_object_type_query, sai_query_*), and asks the questions below about what the
// library did underneath: what its backend was told to program, and a few facts about the
// library's own bookkeeping. Every function here has a meaning that does not depend on the C++
// implementation, so a second implementation of this header can answer it over another library
// (the Rust libsai's test-only C ABI and its virtual backend). Implementations:
//   tests/test_port_cpp.cpp   over the C++ sai-openbcm internals (ob::ctx(), ob::FakeBackend)
//
// Vocabulary:
//   "backend"        the switch-SDK layer under the SAI frontend. Every function below that talks
//                    about the backend refers to the VIRTUAL backend (in-memory model, the one
//                    selected by SAI_OPENBCM_BACKEND=fake); calling them with another backend is a
//                    test bug (the C++ implementation aborts).
//   "hw"             a backend port number (SDK logical port). For the virtual backend with L lanes
//                    per port: port i (0-based, SAI PORT_LIST order) is hw 1 + i*L, its first lane
//                    is 1 + i*L. The CPU port is hw 0.
//   "dest"           a backend destination: a hw port, or lag_dest(tid) for a LAG.
//   backend ids      numbers the backend handed out (L3 interface, egress object, ECMP group, field
//                    group/entry, policer, QoS map, WRED, mirror, sample, genetlink, trunk id).
//                    The virtual backend numbers them deterministically (each kind from its own
//                    counter: egress from 100002, ECMP from 200000, the rest from 1) and tests may
//                    compare them, never assume a value except where the comment says so.
//   absent           functions returning a backend id or a state return -1 when there is none.
//
// Threading: call only from the test's thread, never from a SAI notification callback. Every
// function takes whatever locks it needs; none may be called while the caller holds a lock
// the library could need. Lists come in the backend's own (deterministic) order unless stated.
#pragma once

extern "C" {
#include <sai.h>
}

#include <cstddef>
#include <cstdint>
#include <string>
#include <vector>

namespace tp {

// ================================================================== library and OID layout

// This libsai was built with a hardware backend (else only the virtual one: a switch create
// without SAI_OPENBCM_BACKEND=fake in the profile must be refused).
bool has_hardware_backend();
// A switch object exists (create_switch succeeded and no remove_switch / uninitialize since).
bool switch_created();
// The library's OID layout: bits 63..56 object type, 55..48 switch index, 47..0 object index.
// The PORT index is the hw port; VLAN index is the VID; others are per-type counters.
sai_object_id_t make_oid(sai_object_type_t type, uint64_t index, uint32_t switch_index = 0);
sai_object_type_t oid_type(sai_object_id_t oid);
uint64_t oid_index(sai_object_id_t oid);
uint32_t oid_switch(sai_object_id_t oid);
uint64_t oid_index_mask();   // the largest object index (2^48 - 1)

// Every (object type, attribute) the library advertises, with its create / set / get flags:
// the table sai_query_attribute_capability answers from. Order: as the library lists them.
struct AttrCap { sai_object_type_t ot; sai_attr_id_t id; bool create, set, get; };
std::vector<AttrCap> attr_caps();

// ======================================================= the library's view of its objects

// The backend id a SAI object maps to, -1 if it has none (e.g. not installed):
//   PORT: hw   LAG: trunk id   VIRTUAL_ROUTER: VRF (0 = default VR)   ROUTER_INTERFACE: L3 intf id
//   POLICER: policer id   ACL_TABLE: field group id   ACL_ENTRY: field entry id (-1 while not
//   installed: table unbound or entry disabled)   QOS_MAP / WRED / MIRROR_SESSION / SAMPLEPACKET:
//   the backend object (-1 while the backend does not program that feature)   HOSTIF: netif id
//   (NETDEV) or genetlink id (GENETLINK). Any other object type, or an unknown OID: -1.
int backend_id(sai_object_id_t oid);
// A router interface: its L3 interface id, its VRF, and the VLAN its L3 interface sits on (for a
// PORT / LAG RIF the internal VLAN the library allocated, counting down from 4094).
struct Rif { int intf = -1; int vrf = -1; uint16_t vid = 0; };
Rif rif(sai_object_id_t rif);
// The LAG a port is a member of (SAI_NULL_OBJECT_ID if none).
sai_object_id_t port_lag(sai_object_id_t port);
// The trap is programmed in the backend (trap control set; false: kept, not programmed).
bool trap_programmed(sai_object_id_t trap);
// Number of live SAI objects of a type the library holds (implemented for VIRTUAL_ROUTER,
// ACL_TABLE, ACL_ENTRY; others abort).
size_t object_count(sai_object_type_t type);
// Number of PORT/LAG objects with an ingress ACL (table or table group) bound.
size_t acl_binding_count();
// The backend field group the library keeps its CoPP policing rules in (-1: none exists), and
// how many CoPP rules the library has installed there.
int copp_group();
size_t copp_rule_count();
// The backend egress object of a neighbor (rif, ip); -1 if no such neighbor.
int neighbor_egress(sai_object_id_t rif, const sai_ip_address_t& ip);
// Events (link / FDB) raised by the backend that the library has finished processing
// (delivered to the SAI callbacks, or dropped as duplicates). Monotonic.
uint64_t events_delivered();

// ======================================================== library services (diag, logging)

// Start the diag socket server at path, as create_switch does with SAI_OPENBCM_DIAG_SOCKET
// (empty path: do nothing), and stop it again (removes the socket).
void diag_start(const std::string& socket_path);
void diag_stop();
// Library log lines to stderr on / off (as SAI_OPENBCM_LOG_STDERR=1 does).
void log_to_stderr(bool on);
// External-PHY runtime (SAI_OPENBCM_EXT_PHY_FILE): a table is attached to the live switch; its
// status text (the diag "extphy" answer: "no external PHY table" without one; a port whose PHY
// does not answer shows "FAULT", polls paused by raw MDIO show "PAUSED").
bool ext_phy_active();
std::string ext_phy_status();
// External-MACsec runtime (SAI_OPENBCM_EXT_MACSEC_FILE): attached, and its status text (the
// diag "extmacsec" answer; it names SAs "waiting for their flow", the last failed sequence by
// name, and the object counts "N macsec, N ports, N flows, N SCs, N SAs, N ACL tables, N ACL
// entries"). It never contains key material.
bool ext_macsec_active();
std::string ext_macsec_status();

// ================================================================ virtual backend: hooks

// Optional programming groups of the virtual backend, all off after create_switch: with a group
// off the backend reports it unsupported and the library keeps those objects itself.
//   FEATURE_QOS        QoS maps, schedulers, WRED, buffers, queue/PG/PFC counters
//   FEATURE_MIRROR     mirror sessions, sampling, genetlink host interfaces
//   FEATURE_L2_EXTRAS  LAG/ECMP hash, storm control, VLAN flood control
enum Feature { FEATURE_QOS, FEATURE_MIRROR, FEATURE_L2_EXTRAS };
void set_feature(Feature f, bool on);
// Link of a port changes in the backend (cable pulled / back), reported like a real link event.
void inject_link(int hw, bool up);
// One link-scan pass of the backend: every port's link re-evaluated (external-PHY gates asked),
// changes reported.
void linkscan();
// Hardware learned (learned=true) or aged (false) an L2 entry: the backend's table changes and
// it raises the FDB event.
void inject_fdb(bool learned, const uint8_t mac[6], uint16_t vid, int dest);
// The value the backend will report for a counter: SAI port stat id on a port (the IF_* ids and
// the PFC_<n>_RX/TX_PKTS ids), a SAI queue stat id on queue index q (0..7), a SAI ingress
// priority group stat id on PG index pg (0..7).
void set_port_stat(int hw, sai_stat_id_t id, uint64_t value);
void set_queue_stat(int hw, int queue, sai_stat_id_t id, uint64_t value);
void set_pg_stat(int hw, int pg, sai_stat_id_t id, uint64_t value);
// Traffic hit a field entry: its packet / byte counter grows.
void fp_hit(int eid, uint64_t packets, uint64_t bytes);
// ASIC temperature monitors in tenths of a degree C (the virtual default: 8 monitors 400 + 15*i),
// and whether reading them fails.
void set_temperatures(const std::vector<int>& tenths);
void set_temperature_fail(bool fail);
// The backend's MDIO bus (clause 45, through the switch chip): a register file (unset registers
// read 0), every access logged in order, and PHY addresses that do not answer (accesses fail).
void mdio_set(unsigned phy, unsigned dev, unsigned reg, unsigned value);
void mdio_set_dead(unsigned phy, bool dead);
struct MdioOp { char op; unsigned phy, dev, reg, val; };   // op 'R' / 'W'; val: read or written
std::vector<MdioOp> mdio_log(bool clear = false);         // clear: empty the log after reading it
// Commands the backend's diag shell ran, in order (the virtual backend runs none, it logs them).
std::vector<std::string> shell_log();
// Put an entry into / take it out of the backend's own tables directly, bypassing the library
// (to check that the library's AVAILABLE_* counts follow the backend's accounting).
void backend_host_insert(int vrf, const sai_ip_address_t& ip, int egress);
void backend_host_erase(int vrf, const sai_ip_address_t& ip);
void backend_l2_insert(const uint8_t mac[6], uint16_t vid, int dest);
void backend_l2_erase(const uint8_t mac[6], uint16_t vid);
// The backend's shared drop egress object (created on first use).
int drop_egress();
// Ask the backend itself to destroy an egress object (bypassing the library): its status
// (OBJECT_IN_USE while a host, route or ECMP group uses it).
sai_status_t backend_egress_destroy(int egress);
// The dest value of a LAG (trunk tid) in backend tables.
int lag_dest(int tid);

// ================================================================ virtual backend: state

// Tables whose size tests check (lists below give the others).
enum Table {
    T_STATIONS,          // L3 station (router MAC) entries
    T_QOS_MAPS, T_PORT_QOS_MAPS, T_QUEUE_SCHEDULERS, T_PORT_SCHEDULERS, T_QUEUE_BUFFERS, T_PG_BUFFERS,
    T_MIRRORS, T_PORT_MIRRORS, T_SAMPLES, T_PORT_SAMPLES, T_STORM, T_FLOOD,
    T_FP_GROUPS, T_FP_ENTRIES, T_POLICERS,
};
size_t count(Table t);

// A front-panel port as the backend has it. Learning modes: LEARN_*.
enum Learn { LEARN_HW, LEARN_DISABLE, LEARN_DROP, LEARN_CPU };
struct Port {
    int hw = -1;
    bool admin = false, link = false;
    int32_t fec = SAI_PORT_FEC_MODE_NONE;              // sai_port_fec_mode_t (NONE / RS / FC)
    bool autoneg = false;
    std::string iface;                                // interface type last programmed ("" never)
    uint32_t ifg = 0;                                 // inter-frame gap last programmed (0 never)
    int quirk_calls = 0;                              // interface + IFG programming calls received
    int32_t loopback = SAI_PORT_LOOPBACK_MODE_NONE;   // sai_port_loopback_mode_t (NONE / PHY / MAC)
    uint16_t pvid = 1;
    uint32_t frame_max = 0;                           // largest frame in bytes
    int learn = LEARN_HW;
};
std::vector<Port> ports();          // every front-panel port, by hw
Port port(int hw);                  // hw must exist
void port_forget_quirks(int hw);    // forget the recorded iface / ifg (quirk_calls unchanged)
uint32_t l2_age_seconds();          // the L2 age time last programmed
// VLANs: exists, and its members (the CPU port, hw 0, is a member of every VLAN the library
// creates), sorted by hw.
struct VlanMember { int hw; bool untagged; };
bool vlan_exists(uint16_t vid);
std::vector<VlanMember> vlan_members(uint16_t vid);
// L2 table.
struct L2Entry { uint8_t mac[6]; uint16_t vid; int dest; bool is_static, discard; };
std::vector<L2Entry> l2_entries();
// KNET-like netdevs, by id.
struct Netif { int id; std::string name; int hw; uint16_t vid; uint8_t mac[6]; bool keep_tag, carrier; int filter; };
std::vector<Netif> netifs();
// The backend steers the protocol class of this SAI trap type to the CPU (classes: ARP request,
// ARP response, DHCP, DHCPv6, IGMP, MLD, ND, LLDP, LACP, STP; any other type: false).
bool trap_to_cpu(int32_t sai_trap_type);
// L3.
struct Ip { bool v6 = false; uint8_t addr[16] = {0}; };   // network order; v4 in addr[0..3]
struct L3Intf { int id; uint8_t mac[6]; uint16_t vid; uint32_t mtu; int vrf; };
std::vector<L3Intf> l3_intfs();
struct Egress { int id = -1; int intf = 0; uint8_t mac[6] = {0}; int dest = 0; uint16_t vid = 0; bool cpu = false, drop = false; };
std::vector<Egress> egress_objects();
bool egress(int id, Egress& out);
struct Host { int vrf; Ip ip; int egress; };
std::vector<Host> hosts();
int host_egress(int vrf, const sai_ip_address_t& ip);          // -1: no host entry
struct Route { int vrf; Ip addr, mask; int target; };          // target: egress or ECMP id
std::vector<Route> routes();
int route_target(int vrf, const sai_ip_prefix_t& prefix);       // the prefix is canonicalised by its mask; -1: none
struct Ecmp { int id; std::vector<int> egress; };              // members in order, repeated by weight
std::vector<Ecmp> ecmp_groups();
// LAGs (trunks) and their members in order.
struct LagMember { int hw; bool egress_disable, ingress_disable; };
bool lag_members(int tid, std::vector<LagMember>& out);         // false: no such trunk
// Field processor. Qualifier bits (bit (1 << Q_*) in present / qset), in this order:
enum Qual {
    Q_IN_PORTS, Q_ETHER_TYPE, Q_IP_TYPE, Q_IP_PROTOCOL, Q_SRC_IP, Q_DST_IP, Q_SRC_IP6, Q_DST_IP6,
    Q_L4_SRC_PORT, Q_L4_DST_PORT, Q_TCP_FLAGS, Q_ICMP_TYPE_CODE, Q_TOS, Q_DST_MAC, Q_SRC_MAC,
    Q_OUTER_VLAN, Q_TTL, Q_DST_CPU,
};
struct FpGroup { uint32_t qset; int priority; };                 // priority: higher wins
bool fp_group(int gid, FpGroup& out);
struct FpEntry {
    int eid = -1, gid = -1;
    uint32_t priority = 0;
    uint32_t present = 0;           // qualifier bits
    std::vector<int> in_ports;      // Q_IN_PORTS
    uint16_t ether = 0;             // Q_ETHER_TYPE
    int32_t ip_type = 0;            // Q_IP_TYPE (sai_acl_ip_type_t)
    uint16_t l4_src = 0, l4_dst = 0;
    Ip dst, dst_mask;               // Q_DST_IP / Q_DST_IP6
    bool drop = false, drop_cancel = false, copy_cpu = false, counter = false;
    int policer = -1;
    bool red_drop = false, cpu_copy_only = false;
    std::vector<int> mirror_ingress, mirror_egress;   // mirror ids
};
std::vector<FpEntry> fp_entries();                               // by eid
bool fp_entry(int eid, FpEntry& out);
struct Policer { bool tr_tcm, packets, color_blind; uint64_t cir, cbs, pir, pbs; };
bool policer(int pid, Policer& out);
// QoS (FEATURE_QOS). Map types: sai_qos_map_type_t. A map holds a complete table (every key of
// its type, unset keys 0 / green), in key order; colours are sai_packet_color_t.
struct QosMapEntry { uint32_t key, key_color, value, value_color; };
struct QosMap { int32_t type; std::vector<QosMapEntry> entries; };
bool qos_map(int id, QosMap& out);
int port_qos_map(int hw, int32_t sai_qos_map_type);              // map id on the port, -1 none
int port_default_tc(int hw);                                     // -1: never set
struct Pfc { bool separate; uint8_t tx, rx; };                   // priority bit masks
bool port_pfc(int hw, Pfc& out);
struct Sched { bool strict; uint32_t weight; bool packets; uint64_t min_rate, min_burst, max_rate, max_burst; };
bool queue_scheduler(int hw, int queue, Sched& out);             // false: the default
bool port_scheduler(int hw, Sched& out);
struct Wred { bool enable[3]; uint32_t min[3], max[3], prob[3]; uint8_t weight; int32_t ecn; };   // [green, yellow, red]
bool wred(int id, Wred& out);
int queue_wred(int hw, int queue);
struct Buffer { bool ingress, dynamic; int8_t dynamic_th; uint64_t static_th, reserved, xoff, xon, xon_offset, pool_size, pool_xoff; };
bool queue_buffer(int hw, int queue, Buffer& out);
bool pg_buffer(int hw, int pg, Buffer& out);
// Mirroring and sampling (FEATURE_MIRROR).
struct Mirror {
    bool erspan; int monitor; uint16_t truncate; uint32_t sample_rate; uint8_t tos, ttl; Ip src, dst;
    uint8_t smac[6], dmac[6]; uint16_t gre_type; bool vlan; uint16_t vlan_tpid, vlan_id; uint8_t vlan_pri, vlan_cfi;
};
bool mirror(int id, Mirror& out);
std::vector<int> port_mirrors(int hw, bool ingress);             // mirror ids, empty: none
uint32_t sample_rate(int id);                                    // 0: no such sample
int port_sample(int hw, bool ingress);                           // sample id, -1 none
struct Genl { std::string name, mcgrp; std::vector<int32_t> traps; };   // traps: SAI trap types, ascending
bool genetlink(int id, Genl& out);
// L2 extras (FEATURE_L2_EXTRAS). Kinds named by the SAI attribute that sets them:
// port_storm: SAI_PORT_ATTR_{BROADCAST,MULTICAST,FLOOD}_STORM_CONTROL_POLICER_ID;
// vlan_flood: SAI_VLAN_ATTR_{UNKNOWN_UNICAST,UNKNOWN_MULTICAST,BROADCAST}_FLOOD_CONTROL_TYPE.
struct Hash { std::vector<int32_t> fields; uint32_t seed; };    // SAI_NATIVE_HASH_FIELD_*
bool hash(bool lag, Hash& out);                                  // lag: LAG hash, else ECMP hash
int port_storm(int hw, sai_attr_id_t kind);                      // policer id, -1 none
int vlan_flood(uint16_t vid, sai_attr_id_t kind);                // -1 never set, 0 off, 1 on

} // namespace tp
