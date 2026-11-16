/*
 * wpa_supplicant MACsec driver for the Celestica CS8210's Credo CMS50216
 * PHYs (ports fpp1..fpp16), through the cs8210-macsec shim in the vendor
 * phy_server. Structure follows driver_macsec_qca.c: wired EAPOL path from
 * driver_wired_common, MACsec SecY/SC/SA calls mapped onto the shim's
 * cmx_* commands.
 *
 * Transport: the vendor phy_server command FIFO (the protocol phy_shell
 * uses), one command at a time, guarded by phy_shell's cls_phy_cmd_running
 * flag and our flock. SAKs never travel through the FIFO: each one goes
 * into a 0600 file in the shim's key directory, which the shim reads once
 * and unlinks. Nothing here logs key material.
 *
 * Copyright (c) 2026, nepeat. This software may be distributed under the
 * terms of the BSD license (same as hostap).
 */

#include "includes.h"
#include <sys/file.h>
#include <sys/stat.h>
#include <time.h>

#include "utils/common.h"
#include "utils/eloop.h"
#include "common/defs.h"
#include "common/ieee802_1x_defs.h"
#include "crypto/aes.h"
#include "crypto/random.h"
#include "pae/ieee802_1x_kay.h"
#include "driver.h"
#include "driver_wired_common.h"

#define CMX_TIMEOUT_MS 30000

/* the SDK chroot's /tmp on the host; CS8210_SDK_TMP overrides (tests) */
static char CMX_FIFO[160], CMX_RUNNING[160], CMX_DONE[160], CMX_BASE[160],
	CMX_KEYS[160];

static void cmx_paths(void)
{
	const char *t = getenv("CS8210_SDK_TMP");

	if (!t || !*t || os_strlen(t) > 100)
		t = "/run/cs8210-sdk/tmp";
	os_snprintf(CMX_FIFO, sizeof(CMX_FIFO), "%s/cls_phy_fifo", t);
	os_snprintf(CMX_RUNNING, sizeof(CMX_RUNNING), "%s/cls_phy_cmd_running", t);
	os_snprintf(CMX_DONE, sizeof(CMX_DONE), "%s/cls_phy_cmd_done", t);
	os_snprintf(CMX_BASE, sizeof(CMX_BASE), "%s/cs8210-macsec", t);
	os_snprintf(CMX_KEYS, sizeof(CMX_KEYS), "%s/cs8210-macsec/keys", t);
}

struct macsec_cs8210_data {
	struct driver_wired_common_data common;
	int slice;
	bool up;
	const char *cipher;
	u8 tx_sci[8];
};


static void sci_bytes(const struct ieee802_1x_mka_sci *sci, u8 *b)
{
	os_memcpy(b, sci->addr, ETH_ALEN);
	os_memcpy(b + ETH_ALEN, &sci->port, 2);	/* be16 = wire order */
}


static void sci_hex(const u8 *b, char *hex)
{
	wpa_snprintf_hex(hex, 17, b, 8);
}


static int cmx_safe(const char *s)
{
	if (os_strncmp(s, "cmx_", 4) != 0)
		return 0;
	for (; *s; s++)
		if (!((*s >= 'a' && *s <= 'z') || (*s >= 'A' && *s <= 'Z') ||
		      (*s >= '0' && *s <= '9') || *s == '_' || *s == '=' ||
		      *s == ' ' || *s == '.' || *s == ':' || *s == '-'))
			return 0;
	return 1;
}


static void sleep_ms(int ms)
{
	struct timespec ts = { ms / 1000, (ms % 1000) * 1000000L };

	nanosleep(&ts, NULL);
}


/* Send one command; on success copy the first response line ("ok ...") to
 * out. Returns 0 on "ok", -1 otherwise. */
static int cmx(struct macsec_cs8210_data *drv, char *out, size_t outlen,
	       const char *fmt, ...)
{
	char line[900], resp[128], buf[2048];
	va_list ap;
	u8 rnd[6];
	unsigned long long req;
	int lock, fd, waited, ret = -1, n;
	struct stat st;
	FILE *f;

	va_start(ap, fmt);
	n = vsnprintf(line, sizeof(line) - 40, fmt, ap);
	va_end(ap);
	if (n < 0 || (size_t) n >= sizeof(line) - 40)
		return -1;
	if (os_get_random(rnd, sizeof(rnd)) < 0)
		return -1;
	req = ((unsigned long long) WPA_GET_BE16(rnd) << 32) |
		WPA_GET_BE32(rnd + 2);
	os_snprintf(line + n, sizeof(line) - n, " req=%llu", req);
	if (!cmx_safe(line)) {
		wpa_printf(MSG_ERROR, "macsec_cs8210: refusing command");
		return -1;
	}
	os_snprintf(resp, sizeof(resp), "%s/r-%llu", CMX_BASE, req);

	mkdir(CMX_BASE, 0700);
	if (lstat(CMX_BASE, &st) < 0 || !S_ISDIR(st.st_mode) ||
	    st.st_uid != 0 || (st.st_mode & 077)) {
		wpa_printf(MSG_ERROR,
			   "macsec_cs8210: %s is not a root-only directory",
			   CMX_BASE);
		return -1;
	}
	os_snprintf(buf, sizeof(buf), "%s/lock", CMX_BASE);
	lock = open(buf, O_RDWR | O_CREAT | O_CLOEXEC, 0600);
	if (lock < 0 || flock(lock, LOCK_EX) < 0) {
		if (lock >= 0)
			close(lock);
		return -1;
	}
	for (waited = 0; mkfifo(CMX_RUNNING, 0600) < 0; waited += 20) {
		if (errno != EEXIST || waited > CMX_TIMEOUT_MS) {
			wpa_printf(MSG_ERROR,
				   "macsec_cs8210: cannot take %s", CMX_RUNNING);
			goto unlock;
		}
		sleep_ms(20);
	}
	unlink(CMX_DONE);
	fd = open(CMX_FIFO, O_WRONLY | O_NONBLOCK | O_CLOEXEC);
	if (fd < 0) {
		wpa_printf(MSG_ERROR, "macsec_cs8210: %s: %s (phy_server down?)",
			   CMX_FIFO, strerror(errno));
		goto release;
	}
	n = os_strlen(line);
	line[n++] = '\r';
	if (write(fd, line, n) != n) {
		close(fd);
		goto release;
	}
	close(fd);
	for (waited = 0; access(CMX_DONE, F_OK) < 0; waited += 2) {
		if (waited > CMX_TIMEOUT_MS) {
			wpa_printf(MSG_ERROR, "macsec_cs8210: no answer");
			goto release;
		}
		sleep_ms(2);
	}
	unlink(CMX_DONE);
	ret = 0;
release:
	unlink(CMX_RUNNING);
unlock:
	close(lock);
	if (ret < 0)
		return -1;

	f = fopen(resp, "r");
	if (!f) {
		wpa_printf(MSG_ERROR, "macsec_cs8210: no response (shim not loaded?)");
		return -1;
	}
	n = fread(buf, 1, sizeof(buf) - 1, f);
	fclose(f);
	unlink(resp);
	buf[n > 0 ? n : 0] = '\0';
	ret = os_strncmp(buf, "ok", 2) == 0 && os_strstr(buf, "\nend rc=0") ?
		0 : -1;
	if (ret < 0) {
		char *nl = os_strchr(buf, '\n');

		if (nl)
			*nl = '\0';
		wpa_printf(MSG_ERROR, "macsec_cs8210: %.40s...: %s", line, buf);
	} else if (out && outlen) {
		char *nl = os_strchr(buf, '\n');

		if (nl)
			*nl = '\0';
		os_strlcpy(out, buf, outlen);
	}
	return ret;
}


static unsigned long long resp_u64(const char *resp, const char *key)
{
	char pat[32];
	const char *p;

	os_snprintf(pat, sizeof(pat), " %s=", key);
	p = os_strstr(resp, pat);
	return p ? strtoull(p + os_strlen(pat), NULL, 10) : 0;
}


/* key file for the shim: SAK + GCM hash subkey H = AES_K(0^128) */
static int write_keyfile(const struct data_key *key, char *token, size_t tlen)
{
	u8 rnd[16], zero[16] = { 0 }, h[16];
	char path[200], hex[2 * 32 + 1], hhex[2 * 16 + 1], body[128];
	void *aes;
	int fd, n, ret = -1;

	if (key->key_len != 16 && key->key_len != 32)
		return -1;
	if (os_get_random(rnd, sizeof(rnd)) < 0)
		return -1;
	wpa_snprintf_hex(token, tlen, rnd, sizeof(rnd));
	aes = aes_encrypt_init(key->key, key->key_len);
	if (!aes)
		return -1;
	n = aes_encrypt(aes, zero, h);
	aes_encrypt_deinit(aes);
	if (n < 0)
		goto out;
	wpa_snprintf_hex(hex, sizeof(hex), key->key, key->key_len);
	wpa_snprintf_hex(hhex, sizeof(hhex), h, sizeof(h));
	n = os_snprintf(body, sizeof(body), "sak=%s\nhkey=%s\n", hex, hhex);
	os_snprintf(path, sizeof(path), "%s/%s", CMX_KEYS, token);
	mkdir(CMX_KEYS, 0700);
	fd = open(path, O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC,
		  0600);
	if (fd >= 0) {
		if (write(fd, body, n) == n)
			ret = 0;
		close(fd);
		if (ret < 0)
			unlink(path);
	}
out:
	forced_memzero(h, sizeof(h));
	forced_memzero(hex, sizeof(hex));
	forced_memzero(hhex, sizeof(hhex));
	forced_memzero(body, sizeof(body));
	return ret;
}


static void drop_keyfile(const char *token)
{
	char path[200];

	os_snprintf(path, sizeof(path), "%s/%s", CMX_KEYS, token);
	unlink(path);	/* normally already gone: the shim deletes it on read */
}


static void * macsec_cs8210_init(void *ctx, const char *ifname)
{
	struct macsec_cs8210_data *drv;
	int slice = 0;

	if (os_strncmp(ifname, "fpp", 3) == 0)
		slice = atoi(ifname + 3);
	if (slice < 1 || slice > 16 || os_strchr(ifname, '-')) {
		wpa_printf(MSG_ERROR,
			   "macsec_cs8210: %s is not a MACsec port (fpp1..fpp16, no breakout)",
			   ifname);
		return NULL;
	}
	cmx_paths();
	drv = os_zalloc(sizeof(*drv));
	if (!drv)
		return NULL;
	drv->slice = slice;
	drv->cipher = "gcm-aes-128";
	if (driver_wired_init_common(&drv->common, ifname, ctx) < 0) {
		os_free(drv);
		return NULL;
	}
	return drv;
}


static void macsec_cs8210_deinit(void *priv)
{
	struct macsec_cs8210_data *drv = priv;

	driver_wired_deinit_common(&drv->common);
	os_free(drv);
}


static int macsec_cs8210_macsec_init(void *priv,
				     struct macsec_init_params *params)
{
	struct macsec_cs8210_data *drv = priv;

	/* engine on with the controlled port closed; after a restart of
	 * wpa_supplicant the port is still up: reset it instead (no
	 * clear-text window) */
	if (cmx(drv, NULL, 0, "cmx_port_up slice=%d", drv->slice) < 0 &&
	    cmx(drv, NULL, 0, "cmx_port_reset slice=%d", drv->slice) < 0)
		return -1;
	drv->up = true;
	/* include_sci=1 whatever the config says: the receive side finds the
	 * SC by the explicit SCI (RxCAM), so the peer must send it too */
	return cmx(drv, NULL, 0,
		   "cmx_policy slice=%d controlled=0 open=0 include_sci=1 es=%d scb=%d",
		   drv->slice, params->use_es, params->use_scb);
}


static int macsec_cs8210_macsec_deinit(void *priv)
{
	struct macsec_cs8210_data *drv = priv;

	/* fail closed: SAs gone, data blocked, engine stays on. Going back
	 * to clear text is an explicit "cs8210-macsec down". */
	if (!drv->up)
		return 0;
	drv->up = false;
	return cmx(drv, NULL, 0, "cmx_port_reset slice=%d", drv->slice);
}


static int macsec_cs8210_get_capability(void *priv, enum macsec_cap *cap)
{
	*cap = MACSEC_CAP_INTEG_AND_CONF_0_30_50;
	return 0;
}


static int macsec_cs8210_enable_protect_frames(void *priv, bool enabled)
{
	struct macsec_cs8210_data *drv = priv;

	return cmx(drv, NULL, 0, "cmx_policy slice=%d protect=%d", drv->slice,
		   enabled);
}


static int macsec_cs8210_enable_encrypt(void *priv, bool enabled)
{
	struct macsec_cs8210_data *drv = priv;

	return cmx(drv, NULL, 0, "cmx_policy slice=%d encrypt=%d", drv->slice,
		   enabled);
}


static int macsec_cs8210_set_replay_protect(void *priv, bool enabled,
					    u32 window)
{
	struct macsec_cs8210_data *drv = priv;

	return cmx(drv, NULL, 0, "cmx_policy slice=%d replay=%d window=%u",
		   drv->slice, enabled, window);
}


static int macsec_cs8210_set_current_cipher_suite(void *priv, u64 cs)
{
	struct macsec_cs8210_data *drv = priv;

	if (cs == CS_ID_GCM_AES_128)
		drv->cipher = "gcm-aes-128";
	else if (cs == CS_ID_GCM_AES_256)
		drv->cipher = "gcm-aes-256";
	else {
		wpa_printf(MSG_ERROR, "macsec_cs8210: cipher suite %016llx not supported",
			   (unsigned long long) cs);
		return -1;
	}
	return 0;
}


static int macsec_cs8210_enable_controlled_port(void *priv, bool enabled)
{
	struct macsec_cs8210_data *drv = priv;

	return cmx(drv, NULL, 0, "cmx_policy slice=%d controlled=%d",
		   drv->slice, enabled);
}


static int macsec_cs8210_get_receive_lowest_pn(void *priv,
					       struct receive_sa *sa)
{
	struct macsec_cs8210_data *drv = priv;
	char r[256], sci[17];
	u8 b[8];

	sci_bytes(&sa->sc->sci, b);
	sci_hex(b, sci);
	if (cmx(drv, r, sizeof(r), "cmx_sa_pn slice=%d dir=rx an=%u sci=%s",
		drv->slice, sa->an, sci) < 0)
		return -1;
	sa->lowest_pn = resp_u64(r, "next_pn");
	return 0;
}


static int macsec_cs8210_get_transmit_next_pn(void *priv,
					      struct transmit_sa *sa)
{
	struct macsec_cs8210_data *drv = priv;
	char r[256];

	if (cmx(drv, r, sizeof(r), "cmx_sa_pn slice=%d dir=tx an=%u",
		drv->slice, sa->an) < 0)
		return -1;
	sa->next_pn = resp_u64(r, "next_pn");
	return 0;
}


static int macsec_cs8210_set_transmit_next_pn(void *priv,
					      struct transmit_sa *sa)
{
	struct macsec_cs8210_data *drv = priv;

	return cmx(drv, NULL, 0, "cmx_sa_pn slice=%d dir=tx an=%u set=%u",
		   drv->slice, sa->an, sa->next_pn ? sa->next_pn : 1);
}


static int macsec_cs8210_set_receive_lowest_pn(void *priv,
					       struct receive_sa *sa)
{
	struct macsec_cs8210_data *drv = priv;
	char sci[17];
	u8 b[8];

	sci_bytes(&sa->sc->sci, b);
	sci_hex(b, sci);
	return cmx(drv, NULL, 0, "cmx_sa_pn slice=%d dir=rx an=%u sci=%s set=%u",
		   drv->slice, sa->an, sci, sa->lowest_pn);
}


static unsigned int conf_bytes(unsigned int conf_offset)
{
	return conf_offset == CONFIDENTIALITY_OFFSET_30 ? 30 :
		conf_offset == CONFIDENTIALITY_OFFSET_50 ? 50 : 0;
}


static int macsec_cs8210_create_receive_sc(void *priv, struct receive_sc *sc,
					   unsigned int conf_offset,
					   int validation)
{
	struct macsec_cs8210_data *drv = priv;

	return cmx(drv, NULL, 0, "cmx_policy slice=%d validate=%d conf_offset=%u",
		   drv->slice, validation == Strict ? 2 :
		   validation == Checked ? 1 : 0, conf_bytes(conf_offset));
}


static int macsec_cs8210_delete_receive_sc(void *priv, struct receive_sc *sc)
{
	return 0;	/* the shim frees an SC with its last SA */
}


static int macsec_cs8210_create_receive_sa(void *priv, struct receive_sa *sa)
{
	struct macsec_cs8210_data *drv = priv;
	char token[33], sci[17];
	u8 b[8];
	int ret;

	sci_bytes(&sa->sc->sci, b);
	sci_hex(b, sci);
	if (write_keyfile(sa->pkey, token, sizeof(token)) < 0)
		return -1;
	ret = cmx(drv, NULL, 0,
		  "cmx_sa_add slice=%d dir=rx an=%u sci=%s cipher=%s key=%s pn=%u enable=%d",
		  drv->slice, sa->an, sci, drv->cipher, token,
		  sa->next_pn ? sa->next_pn : 1, sa->enable_receive);
	drop_keyfile(token);
	return ret;
}


static int macsec_cs8210_delete_receive_sa(void *priv, struct receive_sa *sa)
{
	struct macsec_cs8210_data *drv = priv;
	char sci[17];
	u8 b[8];

	sci_bytes(&sa->sc->sci, b);
	sci_hex(b, sci);
	return cmx(drv, NULL, 0, "cmx_sa_del slice=%d dir=rx an=%u sci=%s",
		   drv->slice, sa->an, sci);
}


static int rx_enable(struct macsec_cs8210_data *drv, struct receive_sa *sa,
		     int on)
{
	char sci[17];
	u8 b[8];

	sci_bytes(&sa->sc->sci, b);
	sci_hex(b, sci);
	return cmx(drv, NULL, 0, "cmx_sa_enable slice=%d dir=rx an=%u sci=%s on=%d",
		   drv->slice, sa->an, sci, on);
}


static int macsec_cs8210_enable_receive_sa(void *priv, struct receive_sa *sa)
{
	return rx_enable(priv, sa, 1);
}


static int macsec_cs8210_disable_receive_sa(void *priv, struct receive_sa *sa)
{
	return rx_enable(priv, sa, 0);
}


static int macsec_cs8210_create_transmit_sc(void *priv, struct transmit_sc *sc,
					    unsigned int conf_offset)
{
	struct macsec_cs8210_data *drv = priv;

	sci_bytes(&sc->sci, drv->tx_sci);
	return cmx(drv, NULL, 0, "cmx_policy slice=%d conf_offset=%u", drv->slice,
		   conf_bytes(conf_offset));
}


static int macsec_cs8210_delete_transmit_sc(void *priv, struct transmit_sc *sc)
{
	return 0;
}


static int macsec_cs8210_create_transmit_sa(void *priv,
					    struct transmit_sa *sa)
{
	struct macsec_cs8210_data *drv = priv;
	char token[33], sci[17];
	int ret;

	sci_hex(drv->tx_sci, sci);
	if (write_keyfile(sa->pkey, token, sizeof(token)) < 0)
		return -1;
	/* not active until enable_transmit_sa: the shim then switches the SC
	 * to it with SecY_SA_Chain + SecY_SA_Switch (hitless) */
	ret = cmx(drv, NULL, 0,
		  "cmx_sa_add slice=%d dir=tx an=%u sci=%s cipher=%s key=%s pn=%u enable=%d",
		  drv->slice, sa->an, sci, drv->cipher, token,
		  sa->next_pn ? sa->next_pn : 1, sa->enable_transmit);
	drop_keyfile(token);
	return ret;
}


static int macsec_cs8210_delete_transmit_sa(void *priv,
					    struct transmit_sa *sa)
{
	struct macsec_cs8210_data *drv = priv;

	return cmx(drv, NULL, 0, "cmx_sa_del slice=%d dir=tx an=%u", drv->slice,
		   sa->an);
}


static int macsec_cs8210_enable_transmit_sa(void *priv,
					    struct transmit_sa *sa)
{
	struct macsec_cs8210_data *drv = priv;

	return cmx(drv, NULL, 0, "cmx_sa_enable slice=%d dir=tx an=%u on=1",
		   drv->slice, sa->an);
}


static int macsec_cs8210_disable_transmit_sa(void *priv,
					     struct transmit_sa *sa)
{
	struct macsec_cs8210_data *drv = priv;

	return cmx(drv, NULL, 0, "cmx_sa_enable slice=%d dir=tx an=%u on=0",
		   drv->slice, sa->an);
}


const struct wpa_driver_ops wpa_driver_macsec_cs8210_ops = {
	.name = "macsec_cs8210",
	.desc = "CS8210 Credo PHY MACsec (cs8210-macsec shim)",
	.get_ssid = driver_wired_get_ssid,
	.get_bssid = driver_wired_get_bssid,
	.get_capa = driver_wired_get_capa,
	.init = macsec_cs8210_init,
	.deinit = macsec_cs8210_deinit,

	.macsec_init = macsec_cs8210_macsec_init,
	.macsec_deinit = macsec_cs8210_macsec_deinit,
	.macsec_get_capability = macsec_cs8210_get_capability,
	.enable_protect_frames = macsec_cs8210_enable_protect_frames,
	.enable_encrypt = macsec_cs8210_enable_encrypt,
	.set_replay_protect = macsec_cs8210_set_replay_protect,
	.set_current_cipher_suite = macsec_cs8210_set_current_cipher_suite,
	.enable_controlled_port = macsec_cs8210_enable_controlled_port,
	.get_receive_lowest_pn = macsec_cs8210_get_receive_lowest_pn,
	.get_transmit_next_pn = macsec_cs8210_get_transmit_next_pn,
	.set_transmit_next_pn = macsec_cs8210_set_transmit_next_pn,
	.set_receive_lowest_pn = macsec_cs8210_set_receive_lowest_pn,
	.create_receive_sc = macsec_cs8210_create_receive_sc,
	.delete_receive_sc = macsec_cs8210_delete_receive_sc,
	.create_receive_sa = macsec_cs8210_create_receive_sa,
	.delete_receive_sa = macsec_cs8210_delete_receive_sa,
	.enable_receive_sa = macsec_cs8210_enable_receive_sa,
	.disable_receive_sa = macsec_cs8210_disable_receive_sa,
	.create_transmit_sc = macsec_cs8210_create_transmit_sc,
	.delete_transmit_sc = macsec_cs8210_delete_transmit_sc,
	.create_transmit_sa = macsec_cs8210_create_transmit_sa,
	.delete_transmit_sa = macsec_cs8210_delete_transmit_sa,
	.enable_transmit_sa = macsec_cs8210_enable_transmit_sa,
	.disable_transmit_sa = macsec_cs8210_disable_transmit_sa,
};
