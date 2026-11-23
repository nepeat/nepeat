// SPDX-License-Identifier: GPL-2.0
/*
 * pktloop IFNAME COUNT MINLEN MAXLEN SEED
 *
 * Traffic for fake_nic's QEMU test: sends COUNT Ethernet frames (ethertype
 * 0x88b5, broadcast) of random lengths in [MINLEN, MAXLEN] through an
 * AF_PACKET socket and expects each to come back in order and byte-identical
 * (fake_nic loops TX to RX through the edu DMA engine). At most WINDOW frames
 * are outstanding. Exit 0 iff all came back intact.
 */
#include <arpa/inet.h>
#include <errno.h>
#include <linux/if_packet.h>
#include <net/ethernet.h>
#include <net/if.h>
#include <poll.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/socket.h>
#include <unistd.h>

#define ETYPE 0x88b5
#define WINDOW 6
#define MAXF 9100

static uint64_t rng;
static uint32_t next(void)
{
	rng ^= rng << 13;
	rng ^= rng >> 7;
	rng ^= rng << 17;
	return (uint32_t)rng;
}

static void fill(uint8_t *f, int len, uint32_t seq, const uint8_t *mac)
{
	uint32_t x = seq * 2654435761u + 1;
	memset(f, 0xff, 6);
	memcpy(f + 6, mac, 6);
	f[12] = ETYPE >> 8;
	f[13] = ETYPE & 0xff;
	memcpy(f + 14, &seq, 4);
	memcpy(f + 18, &len, 4);
	for (int i = 22; i < len; i++) {
		x = x * 1103515245u + 12345;
		f[i] = x >> 24;
	}
}

int main(int argc, char **argv)
{
	if (argc != 6) {
		fprintf(stderr, "usage: pktloop IFNAME COUNT MINLEN MAXLEN SEED\n");
		return 2;
	}
	const char *ifn = argv[1];
	int count = atoi(argv[2]), minlen = atoi(argv[3]), maxlen = atoi(argv[4]);
	rng = strtoull(argv[5], 0, 0) | 1;
	if (minlen < 22 || maxlen > MAXF || minlen > maxlen)
		return 2;

	int s = socket(AF_PACKET, SOCK_RAW, htons(ETYPE));
	if (s < 0) { perror("socket"); return 1; }
	struct ifreq ifr = { 0 };
	strncpy(ifr.ifr_name, ifn, IFNAMSIZ - 1);
	if (ioctl(s, SIOCGIFINDEX, &ifr) < 0) { perror("SIOCGIFINDEX"); return 1; }
	int ifindex = ifr.ifr_ifindex;
	if (ioctl(s, SIOCGIFHWADDR, &ifr) < 0) { perror("SIOCGIFHWADDR"); return 1; }
	uint8_t mac[6];
	memcpy(mac, ifr.ifr_hwaddr.sa_data, 6);
	struct sockaddr_ll sll = { .sll_family = AF_PACKET, .sll_protocol = htons(ETYPE), .sll_ifindex = ifindex };
	if (bind(s, (struct sockaddr *)&sll, sizeof(sll)) < 0) { perror("bind"); return 1; }

	static uint8_t tx[MAXF], want[MAXF], rx[MAXF + 64];
	int lens[4096];
	if (count > 4096) count = 4096;
	int sent = 0, got = 0, bad = 0, timeouts = 0;
	long bytes = 0;
	while (got + bad < count && timeouts < 3) {
		while (sent < count && sent - got - bad < WINDOW) {
			int len = minlen + (int)(next() % (uint32_t)(maxlen - minlen + 1));
			lens[sent] = len;
			fill(tx, len, sent, mac);
			struct sockaddr_ll to = sll;
			to.sll_halen = 6;
			memset(to.sll_addr, 0xff, 6);
			if (sendto(s, tx, len, 0, (struct sockaddr *)&to, sizeof(to)) != len) {
				if (errno == ENOBUFS) { usleep(100000); continue; }
				perror("sendto");
				return 1;
			}
			sent++;
		}
		struct pollfd pfd = { .fd = s, .events = POLLIN };
		int r = poll(&pfd, 1, 10000);
		if (r == 0) { timeouts++; fprintf(stderr, "pktloop: timeout (sent %d got %d)\n", sent, got); continue; }
		struct sockaddr_ll from;
		socklen_t fl = sizeof(from);
		int n = recvfrom(s, rx, sizeof(rx), 0, (struct sockaddr *)&from, &fl);
		if (n < 0) { perror("recvfrom"); return 1; }
		if (from.sll_pkttype == PACKET_OUTGOING)
			continue;
		uint32_t seq;
		memcpy(&seq, rx + 14, 4);
		int i = got + bad;
		if (n < 22 || seq != (uint32_t)i || n != lens[i]) {
			fprintf(stderr, "pktloop: frame %d: len %d seq %u (want len %d)\n", i, n, seq, lens[i]);
			bad++;
			continue;
		}
		fill(want, lens[i], i, mac);
		if (memcmp(want, rx, n)) {
			fprintf(stderr, "pktloop: frame %d: payload differs\n", i);
			bad++;
			continue;
		}
		got++;
		bytes += n;
	}
	printf("pktloop: %s sent %d received %d bad %d bytes %ld\n", ifn, sent, got, bad, bytes);
	return (got == count && bad == 0) ? 0 : 1;
}
