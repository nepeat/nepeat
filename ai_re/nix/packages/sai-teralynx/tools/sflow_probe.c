// SPDX-License-Identifier: Apache-2.0
// sflow-probe: sFlow sample checks for the CS8210 SAI chip tests (sai-teralynx
// samplepacket + genetlink hostif). Sends COUNT IPv4/UDP frames (a tag and a
// 32-bit sequence number in the payload) out TX_IF and collects the samples
// the chip takes of them, either
//   netdev   the raw sample frames on RX_IF: Ethernet / IPv4 / GRE 0x8818,
//            8 B, the 16 B sFlow header, then the sampled frame (what the
//            ipd driver hands to the netdev when no sFlow netlink exists), or
//   psample  messages of the generic-netlink family "psample", multicast
//            group "packets" (the kernel's psample, or ipd's own family of
//            that name), psample group GROUP (0: any),
// and prints:
//   SENT ...                       what went out
//   SAMPLES n=.. one_in=.. ci95=..  count, sent / n with a 95% interval
//   GAPS mean=.. sd=.. cv=..         sequence gaps between consecutive samples
//                                    (cv ~1: random, ~0: every N-th frame)
//   META ...                       psample: first sample's metadata and how
//                                  many samples deviate from it; netdev: the
//                                  sample header of the first sample
//   sflow-probe netdev  TX_IF RX_IF COUNT PPS SIZE DMAC SRC_IP DST_IP
//   sflow-probe psample TX_IF GROUP COUNT PPS SIZE DMAC SRC_IP DST_IP
// COUNT 0: send nothing, listen LISTEN seconds (environment, default 10).
// HEX=1: hex dump of the first sample. Root.
#include <arpa/inet.h>
#include <errno.h>
#include <linux/genetlink.h>
#include <linux/if_packet.h>
#include <linux/netlink.h>
#include <math.h>
#include <net/ethernet.h>
#include <net/if.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/socket.h>
#include <time.h>
#include <unistd.h>

static const char TAG[8] = {'S', 'F', 'L', 'O', 'W', 'P', 'R', 'B'};
enum { A_IIF = 0, A_OIF = 1, A_ORIGSIZE = 2, A_GROUP = 3, A_GSEQ = 4, A_RATE = 5, A_DATA = 6, A_PROTO = 14 };
#define MAXS (4u << 20)

static volatile int stop_rx;
static int mode_psample, rxfd, hex;
static unsigned want_group;
static size_t fsize;
static uint32_t* seqs;
static unsigned nseq, other, dups_seen, nobufs, nmsg;
// psample metadata: the first sample's, and how many differ from it
static struct { int have; unsigned iif, oif, rate, origsize, group, datalen, proto; uint32_t gseq0, gseq1; } m0;
static unsigned diff_iif, diff_oif, diff_rate, diff_orig, rate_min = ~0u, rate_max;
// netdev: the first sample's header bytes
static uint8_t hdr0[64];
static int hdr0_len, inner_off0 = -1, sample_len0;
// the sFlow header's last two words: the port's sample count and packet count (pool)
static uint32_t cnt_first[2], cnt_last[2];
static unsigned ncnt;

static double now(void)
{
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return t.tv_sec + t.tv_nsec / 1e9;
}

static int up_and_index(const char* ifn, uint8_t mac[6])
{
    int s = socket(AF_INET, SOCK_DGRAM, 0);
    struct ifreq r;
    memset(&r, 0, sizeof(r));
    snprintf(r.ifr_name, IFNAMSIZ, "%s", ifn);
    if (ioctl(s, SIOCGIFFLAGS, &r) != 0) { perror(ifn); exit(2); }
    if (!(r.ifr_flags & IFF_UP)) {
        r.ifr_flags |= IFF_UP;
        if (ioctl(s, SIOCSIFFLAGS, &r) != 0) { perror("up"); exit(2); }
    }
    memset(&r, 0, sizeof(r));
    snprintf(r.ifr_name, IFNAMSIZ, "%s", ifn);
    if (ioctl(s, SIOCGIFHWADDR, &r) != 0) { perror("hwaddr"); exit(2); }
    memcpy(mac, r.ifr_hwaddr.sa_data, 6);
    close(s);
    return (int)if_nametoindex(ifn);
}

static void dump(const char* what, const uint8_t* b, size_t n)
{
    printf("HEX %s len=%zu\n", what, n);
    for (size_t i = 0; i < n && i < 160; i += 16) {
        printf("  %04zx ", i);
        for (size_t k = i; k < i + 16 && k < n; k++) printf(" %02x", b[k]);
        printf("\n");
    }
}

// the sampled frame: find our tag, record its sequence number
static void take_frame(const uint8_t* f, size_t n)
{
    const uint8_t* t = memmem(f, n, TAG, sizeof(TAG));
    if (!t || t + sizeof(TAG) + 4 > f + n) { other++; return; }
    uint32_t s;
    memcpy(&s, t + sizeof(TAG), 4);
    if (nseq < MAXS) seqs[nseq++] = s;
}

// ---------------------------------------------------------------- netdev
static void netdev_sample(const uint8_t* b, size_t n)
{
    // Ethernet (+ 802.1Q) / IPv4 / GRE
    size_t l2 = 14;
    if (n > 18 && b[12] == 0x81 && b[13] == 0x00) l2 = 18;
    if (n < l2 + 20 + 4 || b[l2 - 2] != 0x08 || b[l2 - 1] != 0x00 || b[l2 + 9] != 47) { other++; return; }
    size_t ihl = (size_t)(b[l2] & 15) * 4;
    const uint8_t* g = b + l2 + ihl;
    if (g + 28 > b + n || g[2] != 0x88 || g[3] != 0x18) { other++; return; }
    if (!hdr0_len) {
        hdr0_len = (int)((size_t)(g + 28 - b) < sizeof(hdr0) ? (size_t)(g + 28 - b) : sizeof(hdr0));
        memcpy(hdr0, b, (size_t)hdr0_len);
        const uint8_t* t = memmem(b, n, TAG, sizeof(TAG));
        // the inner frame starts 42 B before our tag (14 + 20 + 8)
        inner_off0 = t ? (int)(t - b) - 42 : -1;
        sample_len0 = (int)n;
        if (hex) dump("sample", b, n);
    }
    uint32_t w2 = (uint32_t)g[16] << 24 | g[17] << 16 | g[18] << 8 | g[19];
    uint32_t w3 = (uint32_t)g[20] << 24 | g[21] << 16 | g[22] << 8 | g[23];
    if (!ncnt++) { cnt_first[0] = w2; cnt_first[1] = w3; }
    cnt_last[0] = w2;
    cnt_last[1] = w3;
    take_frame(g + 28, (size_t)(b + n - (g + 28)));
}

// --------------------------------------------------------------- psample
static int nl_req(int fd, uint16_t type, uint16_t flags, uint8_t cmd, const void* attrs, size_t alen, uint8_t* out, size_t outsz)
{
    uint8_t buf[512];
    struct nlmsghdr* h = (struct nlmsghdr*)buf;
    memset(buf, 0, sizeof(buf));
    h->nlmsg_len = NLMSG_LENGTH(GENL_HDRLEN + alen);
    h->nlmsg_type = type;
    h->nlmsg_flags = NLM_F_REQUEST | flags;
    h->nlmsg_seq = 1;
    struct genlmsghdr* gh = NLMSG_DATA(h);
    gh->cmd = cmd;
    gh->version = 1;
    memcpy((uint8_t*)gh + GENL_HDRLEN, attrs, alen);
    if (send(fd, buf, h->nlmsg_len, 0) < 0) return -1;
    return (int)recv(fd, out, outsz, 0);
}

// family id and the id of multicast group "packets" of genetlink family "psample"
static int psample_ids(int fd, uint16_t* fam, uint32_t* grp)
{
    uint8_t a[64], out[8192];
    struct nlattr* na = (struct nlattr*)a;
    const char* name = "psample";
    na->nla_type = CTRL_ATTR_FAMILY_NAME;
    na->nla_len = (uint16_t)(NLA_HDRLEN + strlen(name) + 1);
    memcpy(a + NLA_HDRLEN, name, strlen(name) + 1);
    int n = nl_req(fd, GENL_ID_CTRL, 0, CTRL_CMD_GETFAMILY, a, NLA_ALIGN(na->nla_len), out, sizeof(out));
    if (n <= 0) return -1;
    struct nlmsghdr* h = (struct nlmsghdr*)out;
    if (h->nlmsg_type == NLMSG_ERROR) return -1;
    uint8_t* p = (uint8_t*)NLMSG_DATA(h) + GENL_HDRLEN;
    uint8_t* end = out + h->nlmsg_len;
    *fam = 0;
    *grp = 0;
    while (p + NLA_HDRLEN <= end) {
        struct nlattr* x = (struct nlattr*)p;
        if (x->nla_len < NLA_HDRLEN) break;
        if (x->nla_type == CTRL_ATTR_FAMILY_ID) memcpy(fam, p + NLA_HDRLEN, 2);
        if ((x->nla_type & NLA_TYPE_MASK) == CTRL_ATTR_MCAST_GROUPS) {
            uint8_t* q = p + NLA_HDRLEN;
            while (q + NLA_HDRLEN <= p + x->nla_len) {
                struct nlattr* gr = (struct nlattr*)q;
                if (gr->nla_len < NLA_HDRLEN) break;
                uint8_t* r = q + NLA_HDRLEN;
                uint32_t id = 0;
                const char* gname = NULL;
                while (r + NLA_HDRLEN <= q + gr->nla_len) {
                    struct nlattr* y = (struct nlattr*)r;
                    if (y->nla_len < NLA_HDRLEN) break;
                    if (y->nla_type == CTRL_ATTR_MCAST_GRP_ID) memcpy(&id, r + NLA_HDRLEN, 4);
                    if (y->nla_type == CTRL_ATTR_MCAST_GRP_NAME) gname = (const char*)(r + NLA_HDRLEN);
                    r += NLA_ALIGN(y->nla_len);
                }
                if (gname && !strcmp(gname, "packets")) *grp = id;
                q += NLA_ALIGN(gr->nla_len);
            }
        }
        p += NLA_ALIGN(x->nla_len);
    }
    return *fam && *grp ? 0 : -1;
}

static void psample_msg(const uint8_t* p, const uint8_t* end)
{
    unsigned iif = 0, oif = 0, rate = 0, orig = 0, group = 0, proto = 0;
    uint32_t gseq = 0;
    const uint8_t* data = NULL;
    size_t dlen = 0;
    while (p + NLA_HDRLEN <= end) {
        const struct nlattr* x = (const struct nlattr*)p;
        if (x->nla_len < NLA_HDRLEN) break;
        const uint8_t* v = p + NLA_HDRLEN;
        uint16_t u16;
        uint32_t u32;
        switch (x->nla_type & NLA_TYPE_MASK) {
        case A_IIF: memcpy(&u16, v, 2); iif = u16; break;
        case A_OIF: memcpy(&u16, v, 2); oif = u16; break;
        case A_PROTO: memcpy(&u16, v, 2); proto = u16; break;
        case A_ORIGSIZE: memcpy(&u32, v, 4); orig = u32; break;
        case A_GROUP: memcpy(&u32, v, 4); group = u32; break;
        case A_GSEQ: memcpy(&u32, v, 4); gseq = u32; break;
        case A_RATE: memcpy(&u32, v, 4); rate = u32; break;
        case A_DATA: data = v; dlen = x->nla_len - NLA_HDRLEN; break;
        }
        p += NLA_ALIGN(x->nla_len);
    }
    if (want_group && group != want_group) return;
    nmsg++;
    if (!m0.have) {
        m0.have = 1;
        m0.iif = iif; m0.oif = oif; m0.rate = rate; m0.origsize = orig; m0.group = group;
        m0.datalen = (unsigned)dlen; m0.proto = proto; m0.gseq0 = gseq;
        if (hex && data) dump("psample data", data, dlen);
    } else {
        diff_iif += iif != m0.iif;
        diff_oif += oif != m0.oif;
        diff_rate += rate != m0.rate;
    }
    m0.gseq1 = gseq;
    if (rate < rate_min) rate_min = rate;
    if (rate > rate_max) rate_max = rate;
    if (data) {
        // the frame as sent is fsize bytes; the sample's original size
        // should say so (no FCS)
        if (fsize && orig != fsize) diff_orig++;
        take_frame(data, dlen);
    } else {
        other++;
    }
}

static void* rx_loop(void* arg)
{
    (void)arg;
    static uint8_t b[70000];
    while (!stop_rx) {
        if (mode_psample) {
            ssize_t n = recv(rxfd, b, sizeof(b), 0);
            if (n < 0) { if (errno == ENOBUFS) nobufs++; continue; }
            for (struct nlmsghdr* h = (struct nlmsghdr*)b; NLMSG_OK(h, (size_t)n); h = NLMSG_NEXT(h, n)) {
                if (h->nlmsg_type == NLMSG_ERROR || h->nlmsg_type < NLMSG_MIN_TYPE) continue;
                const uint8_t* p = (const uint8_t*)NLMSG_DATA(h) + GENL_HDRLEN;
                psample_msg(p, (const uint8_t*)h + h->nlmsg_len);
            }
        } else {
            struct sockaddr_ll from;
            socklen_t fl = sizeof(from);
            ssize_t n = recvfrom(rxfd, b, sizeof(b), 0, (struct sockaddr*)&from, &fl);
            if (n <= 0 || from.sll_pkttype == PACKET_OUTGOING) continue;
            netdev_sample(b, (size_t)n);
        }
    }
    return NULL;
}

static uint16_t csum(const void* p, size_t n)
{
    const uint8_t* b = p;
    uint32_t s = 0;
    for (size_t i = 0; i + 1 < n; i += 2) s += (uint32_t)(b[i] << 8 | b[i + 1]);
    if (n & 1) s += (uint32_t)(b[n - 1] << 8);
    while (s >> 16) s = (s & 0xffff) + (s >> 16);
    return (uint16_t)~s;
}

static int cmp_u32(const void* a, const void* b)
{
    uint32_t x = *(const uint32_t*)a, y = *(const uint32_t*)b;
    return x < y ? -1 : x > y;
}

static const char* ifname(unsigned ix, char* buf)
{
    if (!ix || !if_indextoname(ix, buf)) snprintf(buf, IF_NAMESIZE, "-");
    return buf;
}

int main(int argc, char** argv)
{
    if (argc < 10 || (strcmp(argv[1], "netdev") && strcmp(argv[1], "psample"))) {
        fprintf(stderr, "usage: %s netdev|psample TX_IF RX_IF|GROUP COUNT PPS SIZE DMAC SRC_IP DST_IP\n", argv[0]);
        return 2;
    }
    mode_psample = !strcmp(argv[1], "psample");
    hex = getenv("HEX") && atoi(getenv("HEX"));
    const char* tx = argv[2];
    unsigned count = (unsigned)strtoul(argv[4], NULL, 0);
    double pps = atof(argv[5]);
    fsize = (size_t)atoi(argv[6]);
    if (fsize < 64) fsize = 64;
    if (fsize > 9000) fsize = 9000;
    uint8_t dmac[6], txmac[6];
    unsigned x[6];
    if (sscanf(argv[7], "%x:%x:%x:%x:%x:%x", &x[0], &x[1], &x[2], &x[3], &x[4], &x[5]) != 6) return 2;
    for (int i = 0; i < 6; i++) dmac[i] = (uint8_t)x[i];
    struct in_addr sip, dip;
    if (inet_pton(AF_INET, argv[8], &sip) != 1 || inet_pton(AF_INET, argv[9], &dip) != 1) return 2;
    seqs = calloc(MAXS, sizeof(uint32_t));
    int big = 256 << 20;
    if (mode_psample) {
        want_group = (unsigned)strtoul(argv[3], NULL, 0);
        rxfd = socket(AF_NETLINK, SOCK_RAW, NETLINK_GENERIC);
        struct sockaddr_nl sa;
        memset(&sa, 0, sizeof(sa));
        sa.nl_family = AF_NETLINK;
        if (rxfd < 0 || bind(rxfd, (struct sockaddr*)&sa, sizeof(sa)) != 0) { perror("netlink"); return 2; }
        uint16_t fam;
        uint32_t grp;
        if (psample_ids(rxfd, &fam, &grp) != 0) { printf("SAMPLES n=0 error=no-psample-family\n"); return 3; }
        if (setsockopt(rxfd, SOL_NETLINK, NETLINK_ADD_MEMBERSHIP, &grp, sizeof(grp)) != 0) { perror("join"); return 2; }
        printf("PSAMPLE family=%u group_packets=%u filter_group=%u\n", fam, grp, want_group);
    } else {
        uint8_t m[6];
        int ix = up_and_index(argv[3], m);
        rxfd = socket(AF_PACKET, SOCK_RAW, htons(ETH_P_ALL));
        struct sockaddr_ll a;
        memset(&a, 0, sizeof(a));
        a.sll_family = AF_PACKET;
        a.sll_protocol = htons(ETH_P_ALL);
        a.sll_ifindex = ix;
        if (bind(rxfd, (struct sockaddr*)&a, sizeof(a)) != 0) { perror("bind rx"); return 2; }
    }
    setsockopt(rxfd, SOL_SOCKET, SO_RCVBUFFORCE, &big, sizeof(big));
    struct timeval tv = {0, 100000};
    setsockopt(rxfd, SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof(tv));
    pthread_t th;
    pthread_create(&th, NULL, rx_loop, NULL);
    usleep(200000);

    unsigned sent = 0, failed = 0;
    double t0 = now(), t1 = t0;
    if (count) {
        int txi = up_and_index(tx, txmac);
        int txfd = socket(AF_PACKET, SOCK_RAW, 0);
        struct sockaddr_ll to;
        memset(&to, 0, sizeof(to));
        to.sll_family = AF_PACKET;
        to.sll_ifindex = txi;
        to.sll_halen = 6;
        memcpy(to.sll_addr, dmac, 6);
        static uint8_t f[9100];
        memset(f, 0, sizeof(f));
        memcpy(f, dmac, 6);
        memcpy(f + 6, txmac, 6);
        f[12] = 0x08;
        uint8_t* ip = f + 14;
        ip[0] = 0x45;
        uint16_t tot = htons((uint16_t)(fsize - 14));
        memcpy(ip + 2, &tot, 2);
        ip[8] = 64;
        ip[9] = 17;
        memcpy(ip + 12, &sip, 4);
        memcpy(ip + 16, &dip, 4);
        uint16_t ic = htons(csum(ip, 20));
        memcpy(ip + 10, &ic, 2);
        uint8_t* udp = ip + 20;
        uint16_t sp = htons(40001), dp = htons(6343), ul = htons((uint16_t)(fsize - 34));
        memcpy(udp, &sp, 2);
        memcpy(udp + 2, &dp, 2);
        memcpy(udp + 4, &ul, 2);
        memcpy(udp + 8, TAG, sizeof(TAG));
        t0 = now();
        for (unsigned i = 0; i < count; i++) {
            memcpy(udp + 16, &i, 4);
            if (sendto(txfd, f, fsize, 0, (struct sockaddr*)&to, sizeof(to)) == (ssize_t)fsize) sent++;
            else failed++;
            if (pps > 0) {
                double due = t0 + (i + 1) / pps;
                while (now() < due) usleep(10);
            }
        }
        t1 = now();
        usleep(1500000);
    } else {
        double l = getenv("LISTEN") ? atof(getenv("LISTEN")) : 10;
        usleep((useconds_t)(l * 1e6));
    }
    stop_rx = 1;
    pthread_join(th, NULL);

    printf("SENT tx=%s sent=%u send_errors=%u seconds=%.3f pps=%.0f size=%zu\n", tx, sent, failed, t1 - t0,
           t1 > t0 ? sent / (t1 - t0) : 0.0, fsize);
    qsort(seqs, nseq, sizeof(uint32_t), cmp_u32);
    unsigned uniq = 0;
    for (unsigned i = 0; i < nseq; i++)
        if (!i || seqs[i] != seqs[i - 1]) seqs[uniq++] = seqs[i];
    dups_seen = nseq - uniq;
    double n = uniq;
    printf("SAMPLES mode=%s n=%u other=%u dups=%u", argv[1], uniq, other, dups_seen);
    if (mode_psample) printf(" messages=%u enobufs=%u", nmsg, nobufs);
    if (sent && uniq) {
        double lo = n + 1.96 * sqrt(n), hi = n - 1.96 * sqrt(n);
        printf(" one_in=%.1f ci95=%.1f-%s", sent / n, sent / lo, hi > 0 ? "" : "inf");
        if (hi > 0) printf("%.1f", sent / hi);
    }
    printf("\n");
    if (uniq > 2) {
        double s = 0, s2 = 0;
        unsigned mn = ~0u, mx = 0;
        for (unsigned i = 1; i < uniq; i++) {
            unsigned d = seqs[i] - seqs[i - 1];
            s += d;
            s2 += (double)d * d;
            if (d < mn) mn = d;
            if (d > mx) mx = d;
        }
        double k = uniq - 1, mean = s / k, sd = sqrt(s2 / k - mean * mean);
        printf("GAPS n=%.0f mean=%.1f sd=%.1f cv=%.2f min=%u max=%u first_seq=%u last_seq=%u\n", k, mean, sd,
               mean > 0 ? sd / mean : 0, mn, mx, seqs[0], seqs[uniq - 1]);
    }
    char a[IF_NAMESIZE], b2[IF_NAMESIZE];
    if (mode_psample && m0.have) {
        printf("META iif=%u(%s) oif=%u(%s) rate=%u rate_range=%u-%u origsize=%u datalen=%u group=%u proto=0x%04x"
               " group_seq=%u-%u diff_iif=%u diff_oif=%u diff_rate=%u origsize_not_%zu=%u\n",
               m0.iif, ifname(m0.iif, a), m0.oif, ifname(m0.oif, b2), m0.rate, rate_min, rate_max, m0.origsize,
               m0.datalen, m0.group, m0.proto, m0.gseq0, m0.gseq1, diff_iif, diff_oif, diff_rate, fsize, diff_orig);
    }
    if (!mode_psample && hdr0_len) {
        // after Ethernet / IPv4: GRE (4), 8 B, the sFlow header (16), the frame
        size_t l2 = hdr0[12] == 0x81 ? 18 : 14, ihl = (size_t)(hdr0[l2] & 15) * 4;
        const uint8_t* g = hdr0 + l2 + ihl;
        printf("META sample_len=%d inner_offset=%d gre=", sample_len0, inner_off0);
        for (int i = 0; i < 4; i++) printf("%02x", g[i]);
        printf(" base=");
        for (int i = 4; i < 8; i++) printf("%02x", g[i]);
        printf(" sflow_hdr=");
        for (int i = 8; i < 24; i++) printf("%02x", g[i]);
        uint32_t w0 = (uint32_t)g[8] << 24 | g[9] << 16 | g[10] << 8 | g[11];
        uint32_t w1 = (uint32_t)g[12] << 24 | g[13] << 16 | g[14] << 8 | g[15];
        printf(" ext_type=%u dir=%u sp=%u sp2=%u\n", w0 >> 26, (w0 >> 15) & 1, w0 & 0x7fff, w1 & 0x7fff);
        // chip counters in the header: samples taken and packets seen on the port, first and last sample
        uint32_t ds = cnt_last[0] - cnt_first[0], dp = cnt_last[1] - cnt_first[1];
        printf("CHIP samples=%u-%u packets=%u-%u d_samples=%u d_packets=%u one_in=%.1f lost=%d\n", cnt_first[0], cnt_last[0],
               cnt_first[1], cnt_last[1], ds, dp, ds ? (double)dp / ds : 0.0, (int)ds + 1 - (int)ncnt);
    }
    fflush(stdout);
    return 0;
}
