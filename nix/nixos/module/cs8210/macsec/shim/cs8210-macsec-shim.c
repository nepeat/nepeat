/*
 * cs8210-macsec-shim: MACsec SA / classifier control for the CS8210's Credo
 * CMS50216 PHYs, loaded into the vendor phy_server with LD_PRELOAD.
 *
 * Why this exists: the vendor CLI (libcredoCmd.so, cmds.c) only knows
 * "allocate an SA, set its fields, add a vPort + match-all rule + SA", once.
 * The engine underneath is the Rambus/Inside Secure EIP-163 (classifier) +
 * EIP-164 (transform) MACsec DDK in libdriver_164_u.so, which can do much
 * more (hitless egress rekey with SecY_SA_Chain + SecY_SA_Switch, four ANs
 * per receive SC, next-PN get/set, replay window, 64-bit counters, EAPOL
 * bypass rules). This file registers extra phy_server commands ("cmx_*")
 * that call that DDK directly, inside phy_server's single CLI thread, so
 * they are serialised with the vendor commands for free.
 *
 * Layouts and call sequences: ai_re/devices/cs8210-32x/macsec.md (derived
 * from the 2021 aarch64 libdriver_164_u.so and Credo's 2026 libSerdes, whose
 * DWARF has the same DDK structs).
 *
 * Rules this file keeps:
 *   - keys never travel through the command FIFO: the client writes a 0600
 *     file under KEYDIR and passes its name; we read it once, unlink it and
 *     wipe our copies. Nothing here prints key material.
 *   - fail closed: a vPort whose last SA is removed falls back to a BYPASS
 *     flow in the DDK (SAMFlow_Clear). So the data rule of a protected port
 *     is switched to fDrop before that can happen, and stays dropping while
 *     the port has no usable SA in that direction (unless policy open=1).
 *   - only libc, vendor symbols as weak references: the library is inert in
 *     processes other than phy_server (LD_PRELOAD reaches the loader's
 *     children too).
 *
 * Built against glibc 2.17 symbols only (the vendor chroot has 2.25).
 */
#define _GNU_SOURCE
#include <elf.h>
#include <link.h>
#include <sys/syscall.h>
#include <errno.h>
#include <fcntl.h>
#include <stdarg.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <time.h>
#include <unistd.h>

#define SHIM_VERSION "1"
#ifndef BASEDIR
#define BASEDIR "/tmp/cs8210-macsec"	/* inside the SDK chroot */
#endif
#define KEYDIR BASEDIR "/keys"
#define NSLICE 16
#define MAXRXSC 4

/* ---- DDK types, 2021 aarch64 layout (see macsec.md "layouts") ---- */

typedef struct { uint32_t lo, hi; } cnt_t;

typedef struct {
	bool fSAInUse;
	uint8_t ConfidentialityOffset;
	bool fProtectFrames;
	bool fIncludeSCI;
	bool fUseES;
	bool fUseSCB;
	bool fConfProtect;
	bool fAllowDataPkts;
	uint8_t PreSecTagAuthStart;
	uint8_t PreSecTagAuthLength;
} secy_sa_e_t;

typedef struct {
	bool fSAInUse;
	uint8_t ConfidentialityOffset;
	bool fReplayProtect;
	uint32_t ValidateFramesTagged;	/* 0 disable, 1 check, 2 strict */
	uint8_t *SCI_p;
	uint8_t AN;
	bool fAllowTagged;
	bool fAllowUntagged;
	bool fValidateUntagged;
	uint8_t PreSecTagAuthStart;
	uint8_t PreSecTagAuthLength;
	bool fRetainSecTAG;
	bool fRetainICV;
} secy_sa_i_t;

enum { ACT_BYPASS = 0, ACT_DROP = 1, ACT_INGRESS = 2, ACT_EGRESS = 3 };
enum { DROP_CRC = 0, DROP_PKT = 1, DROP_INTERNAL = 2, DROP_NONE = 3 };
enum { PORT_COMMON = 0, PORT_RESERVED = 1, PORT_CONTROLLED = 2, PORT_UNCONTROLLED = 3 };

typedef struct {
	unsigned int SA_WordCount;
	uint32_t *TransformRecord_p;
	union {
		secy_sa_e_t Egress;
		secy_sa_i_t Ingress;
		bool fSAInUse;
	} Params;
	uint32_t ActionType;
	uint32_t DropType;
	uint32_t DestPort;
} secy_sa_t;

typedef struct {
	uint32_t flags;
	uint32_t direction;	/* 0 egress, 1 ingress */
	uint32_t operation;	/* 0 MACsec */
	uint8_t AN;
	uint8_t *Key_p;
	unsigned int KeyByteCount;
	uint8_t *HKey_p;
	uint8_t *Salt_p;
	uint8_t *SSCI_p;
	uint8_t *SCI_p;
	uint32_t SeqNumLo;
	uint32_t SeqNumHi;
	uint32_t WindowSize;
	unsigned int ICVByteCount;
} sab_params_t;

#define SAB_FLAG_LONGSEQ 0x1	/* XPN (64-bit PN) */
/* 0x8 SAB_MACSEC_FLAG_ROLLOVER lets the PN wrap: GCM nonce reuse. Never. */

typedef struct {
	uint32_t PacketType;
	uint8_t NumTags;
	uint16_t ChannelID;
	uint32_t MPacketType;
	uint8_t MNumTags;
	uint16_t MChannelID;
	uint32_t Data[4];
	uint32_t DataMask[4];
	void *vPortHandle;
	uint8_t Priority;	/* 0..7, 7 wins */
	bool fDrop;
	bool fControlPacket;
} cfye_rule_t;

typedef struct {
	uint8_t SecTagOffset;
	uint8_t PktExtension;
} cfye_vport_t;

_Static_assert(sizeof(secy_sa_t) == 0x38, "SecY_SA_t");
_Static_assert(__builtin_offsetof(secy_sa_t, ActionType) == 0x28, "ActionType");
_Static_assert(__builtin_offsetof(secy_sa_i_t, SCI_p) == 0x8, "SCI_p");
_Static_assert(__builtin_offsetof(secy_sa_i_t, fRetainICV) == 0x17, "fRetainICV");
_Static_assert(sizeof(sab_params_t) == 0x50, "SABuilder_Params_t");
_Static_assert(__builtin_offsetof(sab_params_t, SeqNumLo) == 0x40, "SeqNumLo");
_Static_assert(sizeof(cfye_rule_t) == 0x40, "CfyE_Rule_t");
_Static_assert(__builtin_offsetof(cfye_rule_t, vPortHandle) == 0x30, "Policy");

/* ---- vendor entry points ----
 * Weak undefined references, bound by the dynamic linker from phy_server's
 * global scope (phy_server itself, libcredoCmd, libcredo, libdriver_164_u).
 * NULL in any other process, where the library then does nothing. No
 * dlsym(): its glibc 2.34+ version would not load in the 2.25 chroot. */

extern void register_cmds(void *tbl, int n) __attribute__((weak));
extern char credo_cmds[] __attribute__((weak));

#define DDK(ret, name, args) extern ret name args __attribute__((weak)); static ret (*p_##name) args;
#define DDK_FUNCS \
	DDK(int, SecY_SA_Add, (unsigned, unsigned, void **, secy_sa_t *)) \
	DDK(int, SecY_SA_Chain, (unsigned, void *, void **, secy_sa_t *)) \
	DDK(int, SecY_SA_Switch, (unsigned, void *, void *)) \
	DDK(int, SecY_SA_Remove, (unsigned, void *)) \
	DDK(int, SecY_SA_Update, (unsigned, void *, secy_sa_t *)) \
	DDK(int, SecY_SA_NextPN_Get, (unsigned, void *, uint32_t *, uint32_t *, bool *)) \
	DDK(int, SecY_SA_NextPN_Update, (unsigned, void *, uint32_t, uint32_t, bool *)) \
	DDK(int, SecY_SA_WindowSize_Update, (unsigned, void *, uint32_t)) \
	DDK(int, SecY_SA_Active_E_Get, (unsigned, unsigned, void **)) \
	DDK(int, SecY_SA_Statistics_E_Get, (unsigned, void *, cnt_t *, bool)) \
	DDK(int, SecY_SA_Statistics_I_Get, (unsigned, void *, cnt_t *, bool)) \
	DDK(int, SecY_SecY_Statistics_E_Get, (unsigned, unsigned, cnt_t *, bool)) \
	DDK(int, SecY_SecY_Statistics_I_Get, (unsigned, unsigned, cnt_t *, bool)) \
	DDK(int, SecY_Ifc_Statistics_E_Get, (unsigned, unsigned, cnt_t *, bool)) \
	DDK(int, SecY_Ifc_Statistics_I_Get, (unsigned, unsigned, cnt_t *, bool)) \
	DDK(int, SecY_SAIndex_Get, (void *, unsigned *, unsigned *)) \
	DDK(int, SecY_Device_Role_Get, (unsigned, int *)) \
	DDK(int, SecY_Device_Limits, (unsigned, unsigned *, unsigned *, unsigned *, unsigned *)) \
	DDK(int, CfyE_Device_Limits, (unsigned, unsigned *, unsigned *, unsigned *)) \
	DDK(int, CfyE_vPort_Add, (unsigned, void **, cfye_vport_t *, unsigned)) \
	DDK(int, CfyE_vPort_Remove, (unsigned, void *)) \
	DDK(int, CfyE_vPortIndex_Get, (void *, unsigned *)) \
	DDK(int, CfyE_Rule_Add, (unsigned, void *, void **, cfye_rule_t *)) \
	DDK(int, CfyE_Rule_Update, (unsigned, void *, cfye_rule_t *)) \
	DDK(int, CfyE_Rule_Enable, (unsigned, void *, bool)) \
	DDK(int, CfyE_Rule_Remove, (unsigned, void *)) \
	DDK(int, CfyE_Rule_Disable, (unsigned, void *, bool)) \
	DDK(int, CfyE_RuleIndex_Get, (void *, unsigned *)) \
	DDK(int, CfyE_Statistics_TCAM_Get, (unsigned, unsigned, cnt_t *, bool)) \
	DDK(int, SABuilder_InitParams, (sab_params_t *, uint8_t, uint32_t, uint32_t)) \
	DDK(int, SABuilder_GetSize, (sab_params_t *, unsigned *)) \
	DDK(int, SABuilder_BuildSA, (sab_params_t *, uint32_t *, void *)) \
	DDK(int, get_channel_id, (void *, int, unsigned *)) \
	DDK(int, enable_low_latency_bypass, (void *, int, int, int))
DDK_FUNCS
#undef DDK

/* libcredoCmd.so internals (local symbols, found through its .symtab) */
static uint8_t *v_devices;	/* 16 x 0x1b8 slice entries from +8 */
static void **v_g_slice;	/* slice the EIP register accessors use */
static struct { char name[512]; int (*fn)(char *); } *v_credo_cmds;
static int ready;		/* 1 = everything resolved */
static int registered;
static char init_err[200];

/* ---- state ---- */

enum { RX = 0, TX = 1 };
static const char *dirname_[2] = { "rx", "tx" };

enum cipher { C_128 = 0, C_256, C_XPN128, C_XPN256 };
static const char *cipher_name[] = { "gcm-aes-128", "gcm-aes-256", "gcm-aes-xpn-128", "gcm-aes-xpn-256" };

struct sa {
	bool used;
	bool hw;		/* installed in the engine */
	bool active;		/* tx: the SA the SC transmits with; rx: fSAInUse */
	uint8_t an;
	uint8_t sci[8];
	int cipher;
	void *h;		/* SecY SA handle */
	time_t created;
	/* tx only, between "add enable=0" and "enable": */
	bool pending;
	uint8_t key[32], hkey[16], salt[12], ssci[4];
	uint32_t seq_lo, seq_hi;
	unsigned long long acc[10];	/* the engine's counters are clear-on-read */
};

struct dirst {
	void *vp_data, *vp_eapol;
	unsigned vpi_data, vpi_eapol;
	void *rule_data, *rule_eapol;
	cfye_rule_t r_data, r_eapol;
	void *sa_eapol;
	bool drop;		/* current fDrop of the data rule */
	unsigned long long acc_secy[9], acc_ifc[9], acc_hits_data, acc_hits_eapol;
};

struct port {
	bool ours;		/* we initialised this slice's engine (survives down) */
	bool up;
	unsigned ch;
	struct dirst d[2];
	/* policy */
	bool controlled, open, protect, encrypt, include_sci, es, scb, replay;
	uint8_t conf_offset;
	uint32_t window;
	uint32_t validate;
	struct sa tx[4];	/* indexed by AN */
	struct sa rx[MAXRXSC][4];
};
static struct port ports[NSLICE + 1];

/* ---- response file ---- */

static FILE *resp;
static char resp_tmp[128], resp_path[128];

static void out(const char *fmt, ...) __attribute__((format(printf, 1, 2)));
static void out(const char *fmt, ...)
{
	va_list ap;
	if (!resp)
		return;
	va_start(ap, fmt);
	vfprintf(resp, fmt, ap);
	va_end(ap);
	fputc('\n', resp);
}

static void wipe(void *p, size_t n)
{
	volatile uint8_t *v = p;
	while (n--)
		*v++ = 0;
}

/* ---- argument parsing: k=v tokens, conservative charset ---- */

#define MAXARGS 24
struct args { int n; char *k[MAXARGS]; char *v[MAXARGS]; };

static int parse(char *line, struct args *a)
{
	char *save = NULL, *tok;
	a->n = 0;
	tok = strtok_r(line, " \t\r\n", &save);	/* command name */
	while ((tok = strtok_r(NULL, " \t\r\n", &save))) {
		char *eq = strchr(tok, '=');
		if (!eq || a->n == MAXARGS)
			return -1;
		for (char *c = tok; *c; c++)
			if (!(*c == '=' || *c == '_' || *c == '-' || *c == '.' || *c == ':' ||
			      (*c >= '0' && *c <= '9') || (*c >= 'a' && *c <= 'z') ||
			      (*c >= 'A' && *c <= 'Z')))
				return -1;
		*eq = 0;
		a->k[a->n] = tok;
		a->v[a->n] = eq + 1;
		a->n++;
	}
	return 0;
}

static const char *arg(struct args *a, const char *k)
{
	for (int i = 0; i < a->n; i++)
		if (!strcmp(a->k[i], k))
			return a->v[i];
	return NULL;
}

static int hexval(char c)
{
	if (c >= '0' && c <= '9')
		return c - '0';
	if (c >= 'a' && c <= 'f')
		return c - 'a' + 10;
	if (c >= 'A' && c <= 'F')
		return c - 'A' + 10;
	return -1;
}

/* decimal or 0x-hex, no sign, no overflow (own parser: glibc 2.38 headers
 * would turn strtoull into __isoc23_strtoull) */
static int parse_u64(const char *s, uint64_t *v)
{
	uint64_t x = 0;
	int base = 10;
	if (!s || !*s)
		return -1;
	if (s[0] == '0' && (s[1] == 'x' || s[1] == 'X')) {
		base = 16;
		s += 2;
		if (!*s)
			return -1;
	}
	for (; *s; s++) {
		int d = hexval(*s);
		if (d < 0 || d >= base || x > (UINT64_MAX - d) / base)
			return -1;
		x = x * base + d;
	}
	*v = x;
	return 0;
}

static int arg_u64(struct args *a, const char *k, uint64_t *v, uint64_t max)
{
	const char *s = arg(a, k);
	uint64_t x;
	if (!s)
		return 0;
	if (parse_u64(s, &x) || x > max)
		return -1;
	*v = x;
	return 1;
}

static int arg_bool(struct args *a, const char *k, bool *v)
{
	uint64_t x;
	int r = arg_u64(a, k, &x, 1);
	if (r == 1)
		*v = x;
	return r;
}

static int hexbytes(const char *s, uint8_t *b, size_t n)
{
	if (!s || strlen(s) != 2 * n)
		return -1;
	for (size_t i = 0; i < n; i++) {
		int h = hexval(s[2 * i]), l = hexval(s[2 * i + 1]);
		if (h < 0 || l < 0)
			return -1;
		b[i] = h << 4 | l;
	}
	return 0;
}

static void hexstr(const uint8_t *b, size_t n, char *o)
{
	for (size_t i = 0; i < n; i++)
		sprintf(o + 2 * i, "%02x", b[i]);
}

/* ---- stat without the GLIBC_2.33 wrappers ---- */

static int sys_fstat(int fd, struct stat *st)
{
	return syscall(SYS_fstat, fd, st);
}

static int sys_lstat(const char *path, struct stat *st)
{
	return syscall(SYS_newfstatat, AT_FDCWD, path, st, AT_SYMLINK_NOFOLLOW);
}

/* ---- symbol resolution ---- */

static int elf_local_sym(const char *path, const char *want, uint64_t *val, uint64_t *size)
{
	int fd = open(path, O_RDONLY | O_CLOEXEC);
	struct stat st;
	int rc = -1;
	if (fd < 0)
		return -1;
	if (sys_fstat(fd, &st) || st.st_size < (off_t)sizeof(Elf64_Ehdr)) {
		close(fd);
		return -1;
	}
	uint8_t *m = malloc(st.st_size);
	if (!m || pread(fd, m, st.st_size, 0) != st.st_size)
		goto done;
	Elf64_Ehdr *eh = (Elf64_Ehdr *)m;
	if (memcmp(eh->e_ident, ELFMAG, SELFMAG) || eh->e_ident[EI_CLASS] != ELFCLASS64 ||
	    eh->e_shoff + (uint64_t)eh->e_shnum * sizeof(Elf64_Shdr) > (uint64_t)st.st_size)
		goto done;
	Elf64_Shdr *sh = (Elf64_Shdr *)(m + eh->e_shoff);
	for (int i = 0; i < eh->e_shnum; i++) {
		if (sh[i].sh_type != SHT_SYMTAB || sh[i].sh_link >= eh->e_shnum)
			continue;
		Elf64_Shdr *str = &sh[sh[i].sh_link];
		if (sh[i].sh_offset + sh[i].sh_size > (uint64_t)st.st_size ||
		    str->sh_offset + str->sh_size > (uint64_t)st.st_size)
			goto done;
		Elf64_Sym *sym = (Elf64_Sym *)(m + sh[i].sh_offset);
		size_t n = sh[i].sh_size / sizeof(Elf64_Sym);
		for (size_t j = 0; j < n; j++) {
			if (sym[j].st_name >= str->sh_size)
				continue;
			if (!strcmp((char *)m + str->sh_offset + sym[j].st_name, want)) {
				*val = sym[j].st_value;
				*size = sym[j].st_size;
				rc = 0;
				goto done;
			}
		}
	}
done:
	free(m);
	close(fd);
	return rc;
}

static char credo_path[256];
static uintptr_t credo_base;

static int find_credo(struct dl_phdr_info *info, size_t size, void *data)
{
	(void)size;
	(void)data;
	const char *n = info->dlpi_name, *b;
	if (!n || !*n)
		return 0;
	b = strrchr(n, '/');
	b = b ? b + 1 : n;
	if (strcmp(b, "libcredoCmd.so"))
		return 0;
	snprintf(credo_path, sizeof(credo_path), "%s", n);
	credo_base = info->dlpi_addr;
	return 1;
}

static int resolve(void)
{
	uint64_t v, sz;
#define DDK(ret, name, args) \
	if (!(p_##name = name)) { \
		snprintf(init_err, sizeof(init_err), "missing symbol %s", #name); \
		return -1; \
	}
	DDK_FUNCS
#undef DDK
	if (!dl_iterate_phdr(find_credo, NULL) || !credo_base) {
		snprintf(init_err, sizeof(init_err), "libcredoCmd.so not loaded");
		return -1;
	}
	if (elf_local_sym(credo_path, "devices", &v, &sz) || sz < 8 + (NSLICE / 2 - 1) * 0x378 + 2 * 0x1b8) {
		snprintf(init_err, sizeof(init_err), "libcredoCmd: no usable 'devices' symbol");
		return -1;
	}
	v_devices = (uint8_t *)credo_base + v;
	if (elf_local_sym(credo_path, "g_slice", &v, &sz) || sz != 8) {
		snprintf(init_err, sizeof(init_err), "libcredoCmd: no usable 'g_slice' symbol");
		return -1;
	}
	v_g_slice = (void **)((uint8_t *)credo_base + v);
	v_credo_cmds = (void *)credo_cmds;
	if (!v_credo_cmds) {
		snprintf(init_err, sizeof(init_err), "missing symbol credo_cmds");
		return -1;
	}
	return 0;
}

/* devices: 8 pairs of slice entries, pair stride 0x378, second entry of a
 * pair at +0x1b8; slice id at +0, slice pointer at +0x10 (credo_macsec_*
 * in libcredoCmd; verified in phy_server memory 2026-11-16) */
static uint8_t *slice_entry(int k)
{
	return v_devices + 8 + (k / 2) * 0x378 + (k % 2) * 0x1b8;
}

/* slice pointer for a vendor slice id (1..16), and make it the one the
 * EIP register accessors (eip_register_read/write) talk to */
static void *use_slice(int s)
{
	for (int k = 0; k < NSLICE; k++) {
		uint8_t *e = slice_entry(k);
		if (*(uint32_t *)e == (uint32_t)s) {
			void *p = *(void **)(e + 0x10);
			if (p)
				*v_g_slice = p;
			return p;
		}
	}
	return NULL;
}

static int (*vendor_cmd(const char *name))(char *)
{
	size_t n = strlen(name);
	for (int i = 0; i < 0x41; i++)
		if (!strncmp(v_credo_cmds[i].name, name, n) &&
		    (v_credo_cmds[i].name[n] == ' ' || v_credo_cmds[i].name[n] == 0))
			return v_credo_cmds[i].fn;
	return NULL;
}

static unsigned dev_of(int s, int dir)
{
	return 2 * (s - 1) + (dir == TX ? 1 : 0);
}

static bool dev_ready(unsigned dev)
{
	int role;
	return p_SecY_Device_Role_Get(dev, &role) == 0;
}

/* ---- SA building ---- */

static bool is_xpn(int c) { return c == C_XPN128 || c == C_XPN256; }
static unsigned keylen(int c) { return (c == C_256 || c == C_XPN256) ? 32 : 16; }

static int parse_cipher(const char *s)
{
	for (int i = 0; i < 4; i++)
		if (s && !strcmp(s, cipher_name[i]))
			return i;
	return -1;
}

/* key file: lines "sak=HEX", "hkey=HEX", optional "salt=HEX" (XPN) */
static int read_keyfile(const char *token, int cipher, uint8_t *key, uint8_t *hkey, uint8_t *salt)
{
	char path[160], buf[512];
	struct stat st;
	int fd, rc = -1;
	ssize_t n;
	bool have_k = false, have_h = false, have_s = false;
	size_t tl = strlen(token);

	if (tl < 8 || tl > 64)
		return -1;
	for (size_t i = 0; i < tl; i++)
		if (!((token[i] >= '0' && token[i] <= '9') || (token[i] >= 'a' && token[i] <= 'z') ||
		      (token[i] >= 'A' && token[i] <= 'Z') || token[i] == '-' || token[i] == '_'))
			return -1;
	snprintf(path, sizeof(path), KEYDIR "/%s", token);
	fd = open(path, O_RDONLY | O_NOFOLLOW | O_CLOEXEC);
	if (fd < 0)
		return -1;
	unlink(path);
	if (sys_fstat(fd, &st) || !S_ISREG(st.st_mode) || st.st_uid != 0 || (st.st_mode & 077) ||
	    st.st_size >= (off_t)sizeof(buf)) {
		close(fd);
		return -1;
	}
	n = read(fd, buf, sizeof(buf) - 1);
	close(fd);
	if (n <= 0)
		goto out;
	buf[n] = 0;
	for (char *save = NULL, *l = strtok_r(buf, "\n", &save); l; l = strtok_r(NULL, "\n", &save)) {
		if (!strncmp(l, "sak=", 4))
			have_k = !hexbytes(l + 4, key, keylen(cipher));
		else if (!strncmp(l, "hkey=", 5))
			have_h = !hexbytes(l + 5, hkey, 16);
		else if (!strncmp(l, "salt=", 5))
			have_s = !hexbytes(l + 5, salt, 12);
	}
	if (have_k && have_h && (have_s || !is_xpn(cipher)))
		rc = 0;
out:
	wipe(buf, sizeof(buf));
	return rc;
}

static void fill_flow(struct port *p, int dir, struct sa *sa, secy_sa_t *f)
{
	memset(f, 0, sizeof(*f));
	if (dir == TX) {
		f->ActionType = ACT_EGRESS;
		f->DropType = DROP_INTERNAL;
		f->DestPort = PORT_COMMON;
		f->Params.Egress.fSAInUse = true;
		f->Params.Egress.ConfidentialityOffset = p->encrypt ? p->conf_offset : 0;
		f->Params.Egress.fProtectFrames = p->protect;
		f->Params.Egress.fIncludeSCI = p->include_sci;
		f->Params.Egress.fUseES = p->es;
		f->Params.Egress.fUseSCB = p->scb;
		f->Params.Egress.fConfProtect = p->encrypt;
		f->Params.Egress.fAllowDataPkts = true;
		f->Params.Egress.PreSecTagAuthStart = 0;
		f->Params.Egress.PreSecTagAuthLength = 12;	/* DA + SA in the ICV, as 802.1AE */
	} else {
		f->ActionType = ACT_INGRESS;
		f->DropType = DROP_INTERNAL;
		f->DestPort = PORT_CONTROLLED;
		f->Params.Ingress.fSAInUse = sa ? sa->active : true;
		f->Params.Ingress.ConfidentialityOffset = p->conf_offset;
		f->Params.Ingress.fReplayProtect = p->replay;
		f->Params.Ingress.ValidateFramesTagged = p->validate;
		f->Params.Ingress.SCI_p = sa ? sa->sci : NULL;
		f->Params.Ingress.AN = sa ? sa->an : 0;
		f->Params.Ingress.fAllowTagged = true;
		f->Params.Ingress.fAllowUntagged = p->validate != 2;
		f->Params.Ingress.fValidateUntagged = false;
		f->Params.Ingress.PreSecTagAuthStart = 0;
		f->Params.Ingress.PreSecTagAuthLength = 12;
	}
}

/* builds the transform record into f (caller frees f->TransformRecord_p) */
static int build_tr(struct port *p, int dir, struct sa *sa, const uint8_t *key, const uint8_t *hkey,
		    const uint8_t *salt, const uint8_t *ssci, uint32_t seq_lo, uint32_t seq_hi,
		    secy_sa_t *f)
{
	sab_params_t sp;
	unsigned words = 0;
	uint8_t sci[8], ssci_c[4], salt_c[12];

	fill_flow(p, dir, sa, f);
	if (p_SABuilder_InitParams(&sp, sa->an, dir == TX ? 0 : 1, 0))
		return -1;
	memcpy(sci, sa->sci, 8);
	memcpy(ssci_c, ssci, 4);
	memcpy(salt_c, salt, 12);
	sp.flags = is_xpn(sa->cipher) ? SAB_FLAG_LONGSEQ : 0;
	sp.Key_p = (uint8_t *)key;
	sp.KeyByteCount = keylen(sa->cipher);
	sp.HKey_p = (uint8_t *)hkey;
	sp.SCI_p = sci;
	sp.SeqNumLo = seq_lo;
	sp.SeqNumHi = is_xpn(sa->cipher) ? seq_hi : 0;
	sp.WindowSize = dir == RX ? p->window : 0;
	if (is_xpn(sa->cipher)) {
		sp.Salt_p = salt_c;
		sp.SSCI_p = ssci_c;
	}
	if (p_SABuilder_GetSize(&sp, &words) || words == 0 || words > 64)
		return -1;
	f->TransformRecord_p = calloc(words, 4);
	if (!f->TransformRecord_p)
		return -1;
	f->SA_WordCount = words;
	if (p_SABuilder_BuildSA(&sp, f->TransformRecord_p, NULL)) {
		free(f->TransformRecord_p);
		f->TransformRecord_p = NULL;
		return -1;
	}
	wipe(&sp, sizeof(sp));
	return 0;
}

static void free_tr(secy_sa_t *f)
{
	if (f->TransformRecord_p) {
		wipe(f->TransformRecord_p, f->SA_WordCount * 4);
		free(f->TransformRecord_p);
		f->TransformRecord_p = NULL;
	}
}

/* ---- gating (fail closed) ---- */

static int count_rx(struct port *p, bool active_only)
{
	int n = 0;
	for (int i = 0; i < MAXRXSC; i++)
		for (int a = 0; a < 4; a++)
			if (p->rx[i][a].used && p->rx[i][a].hw && (!active_only || p->rx[i][a].active))
				n++;
	return n;
}

static struct sa *tx_active(struct port *p)
{
	for (int a = 0; a < 4; a++)
		if (p->tx[a].used && p->tx[a].hw && p->tx[a].active)
			return &p->tx[a];
	return NULL;
}

static int set_drop(int s, int dir, bool drop)
{
	struct port *p = &ports[s];
	struct dirst *d = &p->d[dir];
	if (!d->rule_data || d->drop == drop)
		return 0;
	d->r_data.fDrop = drop;
	if (p_CfyE_Rule_Update(dev_of(s, dir), d->rule_data, &d->r_data)) {
		d->r_data.fDrop = d->drop;
		return -1;
	}
	d->drop = drop;
	return 0;
}

/* drop data unless (controlled port enabled) and (an SA can carry it, or
 * policy open lets the port run unprotected) */
static int apply_gates(int s)
{
	struct port *p = &ports[s];
	bool tx_ok = p->controlled && (tx_active(p) || p->open);
	bool rx_ok = p->controlled && (count_rx(p, false) > 0 || p->open);
	int rc = 0;
	rc |= set_drop(s, TX, !tx_ok);
	rc |= set_drop(s, RX, !rx_ok);
	return rc;
}

/* ---- commands ---- */

#define ERR(...) do { out("err " __VA_ARGS__); return 1; } while (0)

static int get_slice(struct args *a, int *s)
{
	uint64_t v;
	if (arg_u64(a, "slice", &v, NSLICE) != 1 || v < 1)
		return -1;
	*s = v;
	if (!use_slice(*s))
		return -1;
	return 0;
}

static int c_hello(struct args *a)
{
	(void)a;
	out("ok proto=1 shim=" SHIM_VERSION);
	for (int s = 1; s <= NSLICE; s++) {
		if (!use_slice(s))
			continue;
		for (int dir = 0; dir < 2; dir++) {
			unsigned dev = dev_of(s, dir), ch = 0, vp = 0, sas = 0, scs = 0, cch = 0, cvp = 0, rules = 0;
			if (!dev_ready(dev))
				continue;
			p_SecY_Device_Limits(dev, &ch, &vp, &sas, &scs);
			p_CfyE_Device_Limits(dev, &cch, &cvp, &rules);
			out("limits slice=%d dir=%s dev=%u secy_channels=%u secy_vports=%u sas=%u scs=%u cfye_channels=%u cfye_vports=%u rules=%u",
			    s, dirname_[dir], dev, ch, vp, sas, scs, cch, cvp, rules);
		}
	}
	return 0;
}

static int mk_vport(unsigned dev, uint8_t ext, void **h, unsigned *idx)
{
	cfye_vport_t v = { .SecTagOffset = 12, .PktExtension = ext };
	if (p_CfyE_vPort_Add(dev, h, &v, 0))
		return -1;
	if (p_CfyE_vPortIndex_Get(*h, idx)) {
		p_CfyE_vPort_Remove(dev, *h);
		*h = NULL;
		return -1;
	}
	return 0;
}

static int mk_rule(unsigned dev, unsigned ch, void *vp, bool eapol, bool drop, cfye_rule_t *r, void **h)
{
	memset(r, 0, sizeof(*r));
	r->ChannelID = ch;
	r->MChannelID = 0xff;
	r->vPortHandle = vp;
	if (eapol) {
		/* TCAM data is the frame bytes packed little-endian: EtherType
		 * 88 8e -> 0x8e88 (same as Credo's MACsec_Rule_Create) */
		r->Data[2] = 0x8e88;
		r->DataMask[2] = 0xffff;
		r->Priority = 7;
		r->fControlPacket = true;
	} else {
		r->Priority = 0;
		r->fDrop = drop;
	}
	if (p_CfyE_Rule_Add(dev, vp, h, r))
		return -1;
	if (p_CfyE_Rule_Enable(dev, *h, true)) {
		p_CfyE_Rule_Disable(dev, *h, true);
		p_CfyE_Rule_Remove(dev, *h);
		*h = NULL;
		return -1;
	}
	return 0;
}

static void teardown_dir(int s, int dir)
{
	struct port *p = &ports[s];
	struct dirst *d = &p->d[dir];
	unsigned dev = dev_of(s, dir);
	if (d->rule_data) {
		d->r_data.fDrop = true;
		p_CfyE_Rule_Update(dev, d->rule_data, &d->r_data);
		d->drop = true;
	}
	if (dir == TX) {
		for (int an = 0; an < 4; an++) {
			struct sa *sa = &p->tx[an];
			if (sa->used && sa->hw)
				p_SecY_SA_Remove(dev, sa->h);
			wipe(sa, sizeof(*sa));
		}
	} else {
		for (int i = 0; i < MAXRXSC; i++)
			for (int an = 0; an < 4; an++) {
				struct sa *sa = &p->rx[i][an];
				if (sa->used && sa->hw)
					p_SecY_SA_Remove(dev, sa->h);
				wipe(sa, sizeof(*sa));
			}
	}
	/* CfyE_Rule_Remove only resets the TCAM entry's policy to the default
	 * (vPort 0) and leaves the entry enabled: a removed rule keeps
	 * matching and sends traffic to vPort 0, in clear text if that has a
	 * bypass flow (seen on hardware 2026-11-16). Disable first. */
	if (d->rule_data) {
		p_CfyE_Rule_Disable(dev, d->rule_data, true);
		p_CfyE_Rule_Remove(dev, d->rule_data);
	}
	if (d->rule_eapol) {
		p_CfyE_Rule_Disable(dev, d->rule_eapol, true);
		p_CfyE_Rule_Remove(dev, d->rule_eapol);
	}
	if (d->sa_eapol)
		p_SecY_SA_Remove(dev, d->sa_eapol);
	if (d->vp_data)
		p_CfyE_vPort_Remove(dev, d->vp_data);
	if (d->vp_eapol)
		p_CfyE_vPort_Remove(dev, d->vp_eapol);
	memset(d, 0, sizeof(*d));
}

/* cmx_port_up slice=S [port=0] [eapol=1]
 * engine on for the slice: datapath init if needed, EAPOL bypass rule +
 * data rule (dropping) per direction, then low-latency bypass off */
static int c_port_up(struct args *a)
{
	int s;
	uint64_t port = 0;
	bool eapol = true, force = false;
	if (get_slice(a, &s) || arg_u64(a, "port", &port, 0) < 0 || arg_bool(a, "eapol", &eapol) < 0 ||
	    arg_bool(a, "force", &force) < 0)
		ERR("bad arguments");
	struct port *p = &ports[s];
	void *slice = use_slice(s);
	if (p->up)
		ERR("slice %d already up", s);
	/* initialised but not by us = the vendor loader's own SAs and match-all
	 * rules are there and would shadow ours (same priority, lower index) */
	if ((dev_ready(dev_of(s, RX)) || dev_ready(dev_of(s, TX))) && !force && !p->ours)
		ERR("slice %d engine already initialised by someone else (vendor static config?); restart cs8210-switch or force=1", s);
	if (!dev_ready(dev_of(s, RX)) || !dev_ready(dev_of(s, TX))) {
		int (*init)(char *) = vendor_cmd("macsec_datapath_init");
		char line[64];
		if (!init)
			ERR("vendor macsec_datapath_init not found");
		snprintf(line, sizeof(line), "macsec_datapath_init slice_id=%d", s);
		init(line);
		use_slice(s);
		if (!dev_ready(dev_of(s, RX)) || !dev_ready(dev_of(s, TX)))
			ERR("datapath init failed for slice %d", s);
	}
	p->ours = true;
	if (p_get_channel_id(slice, (int)port, &p->ch))
		ERR("get_channel_id failed");
	memset(p->tx, 0, sizeof(p->tx));
	memset(p->rx, 0, sizeof(p->rx));
	p->controlled = false;
	p->open = false;
	p->protect = p->encrypt = p->include_sci = p->replay = true;
	p->es = p->scb = false;
	p->conf_offset = 0;
	p->window = 0;
	p->validate = 2;
	for (int dir = 0; dir < 2; dir++) {
		struct dirst *d = &p->d[dir];
		unsigned dev = dev_of(s, dir);
		memset(d, 0, sizeof(*d));
		if (eapol) {
			secy_sa_t f;
			memset(&f, 0, sizeof(f));
			f.ActionType = ACT_BYPASS;
			f.DestPort = PORT_UNCONTROLLED;
			f.Params.fSAInUse = true;
			if (mk_vport(dev, 0, &d->vp_eapol, &d->vpi_eapol) ||
			    p_SecY_SA_Add(dev, d->vpi_eapol, &d->sa_eapol, &f) ||
			    mk_rule(dev, p->ch, d->vp_eapol, true, false, &d->r_eapol, &d->rule_eapol)) {
				teardown_dir(s, dir);
				if (dir == TX)
					teardown_dir(s, RX);
				ERR("eapol bypass setup failed (dir=%s)", dirname_[dir]);
			}
		}
		/* PktExtension 3 = room for SecTAG with SCI + ICV (vendor value) */
		if (mk_vport(dev, 3, &d->vp_data, &d->vpi_data) ||
		    mk_rule(dev, p->ch, d->vp_data, false, true, &d->r_data, &d->rule_data)) {
			teardown_dir(s, dir);
			if (dir == TX)
				teardown_dir(s, RX);
			ERR("data rule setup failed (dir=%s)", dirname_[dir]);
		}
		d->drop = true;
	}
	use_slice(s);
	if (p_enable_low_latency_bypass(slice, (int)port, 0, 0) ||
	    p_enable_low_latency_bypass(slice, (int)port, 1, 0)) {
		p_enable_low_latency_bypass(slice, (int)port, 0, 1);
		p_enable_low_latency_bypass(slice, (int)port, 1, 1);
		teardown_dir(s, TX);
		teardown_dir(s, RX);
		ERR("enable_low_latency_bypass failed");
	}
	p->up = true;
	out("ok slice=%d channel=%u rx_vport=%u tx_vport=%u eapol=%d", s, p->ch,
	    p->d[RX].vpi_data, p->d[TX].vpi_data, eapol);
	return 0;
}

/* cmx_port_down slice=S [port=0]: back to the vendor bypass (clear text!) */
static int c_port_down(struct args *a)
{
	int s;
	uint64_t port = 0;
	if (get_slice(a, &s) || arg_u64(a, "port", &port, 0) < 0)
		ERR("bad arguments");
	struct port *p = &ports[s];
	void *slice = use_slice(s);
	if (!p->up)
		ERR("slice %d not up", s);
	set_drop(s, TX, true);
	set_drop(s, RX, true);
	p_enable_low_latency_bypass(slice, (int)port, 0, 1);
	p_enable_low_latency_bypass(slice, (int)port, 1, 1);
	use_slice(s);
	teardown_dir(s, TX);
	teardown_dir(s, RX);
	p->up = false;
	out("ok slice=%d bypass", s);
	return 0;
}

/* cmx_port_reset slice=S: close the controlled port, delete every SA, keep
 * the engine and rules (no clear-text window, unlike port_down/up) */
static int c_port_reset(struct args *a)
{
	int s;
	if (get_slice(a, &s))
		ERR("bad arguments");
	struct port *p = &ports[s];
	if (!p->up)
		ERR("slice %d not up", s);
	p->controlled = false;
	if (set_drop(s, TX, true) || set_drop(s, RX, true))
		ERR("could not block data");
	for (int an = 0; an < 4; an++) {
		struct sa *sa = &p->tx[an];
		if (sa->used && sa->hw)
			p_SecY_SA_Remove(dev_of(s, TX), sa->h);
		wipe(sa, sizeof(*sa));
	}
	for (int i = 0; i < MAXRXSC; i++)
		for (int an = 0; an < 4; an++) {
			struct sa *sa = &p->rx[i][an];
			if (sa->used && sa->hw)
				p_SecY_SA_Remove(dev_of(s, RX), sa->h);
			wipe(sa, sizeof(*sa));
		}
	out("ok slice=%d reset", s);
	return 0;
}

static int reflow(int s)
{
	/* the flow (SAM_FLOW_CTRL) is per vPort: rewrite it through any SA */
	struct port *p = &ports[s];
	secy_sa_t f;
	int rc = 0;
	struct sa *t = tx_active(p);
	if (t) {
		fill_flow(p, TX, t, &f);
		rc |= p_SecY_SA_Update(dev_of(s, TX), t->h, &f);
	}
	for (int i = 0; i < MAXRXSC; i++)
		for (int an = 0; an < 4; an++) {
			struct sa *sa = &p->rx[i][an];
			if (!sa->used || !sa->hw)
				continue;
			fill_flow(p, RX, sa, &f);
			rc |= p_SecY_SA_Update(dev_of(s, RX), sa->h, &f);
			rc |= p_SecY_SA_WindowSize_Update(dev_of(s, RX), sa->h, p->window);
		}
	return rc;
}

/* cmx_policy slice=S [controlled= open= protect= encrypt= include_sci= es=
 *   scb= replay= window= validate=0|1|2 conf_offset=] */
static int c_policy(struct args *a)
{
	int s;
	uint64_t v;
	if (get_slice(a, &s))
		ERR("bad slice");
	struct port *p = &ports[s];
	if (!p->up)
		ERR("slice %d not up", s);
	struct port n;
	memcpy(&n, p, offsetof(struct port, tx));
	if (arg_bool(a, "controlled", &n.controlled) < 0 || arg_bool(a, "open", &n.open) < 0 ||
	    arg_bool(a, "protect", &n.protect) < 0 || arg_bool(a, "encrypt", &n.encrypt) < 0 ||
	    arg_bool(a, "include_sci", &n.include_sci) < 0 || arg_bool(a, "es", &n.es) < 0 ||
	    arg_bool(a, "scb", &n.scb) < 0 || arg_bool(a, "replay", &n.replay) < 0)
		ERR("bad flag");
	if (arg_u64(a, "window", &v, 0xffffffffu) == 1)
		n.window = v;
	else if (arg(a, "window"))
		ERR("bad window");
	if (arg_u64(a, "validate", &v, 2) == 1)
		n.validate = v;
	else if (arg(a, "validate"))
		ERR("bad validate");
	if (arg_u64(a, "conf_offset", &v, 63) == 1)
		n.conf_offset = v;
	else if (arg(a, "conf_offset"))
		ERR("bad conf_offset");
	bool flow = n.protect != p->protect || n.encrypt != p->encrypt || n.include_sci != p->include_sci ||
		    n.es != p->es || n.scb != p->scb || n.replay != p->replay || n.window != p->window ||
		    n.validate != p->validate || n.conf_offset != p->conf_offset;
	p->controlled = n.controlled;
	p->open = n.open;
	p->protect = n.protect;
	p->encrypt = n.encrypt;
	p->include_sci = n.include_sci;
	p->es = n.es;
	p->scb = n.scb;
	p->replay = n.replay;
	p->window = n.window;
	p->validate = n.validate;
	p->conf_offset = n.conf_offset;
	use_slice(s);
	/* close the gate before changing flows, reopen after */
	if (flow) {
		set_drop(s, TX, true);
		set_drop(s, RX, true);
		if (reflow(s))
			ERR("flow update failed (data blocked)");
	}
	if (apply_gates(s))
		ERR("gate update failed");
	out("ok slice=%d", s);
	return 0;
}

static struct sa *find_rx(struct port *p, const uint8_t *sci, int an, bool create)
{
	int free_sc = -1;
	for (int i = 0; i < MAXRXSC; i++) {
		bool any = false;
		for (int a = 0; a < 4; a++)
			if (p->rx[i][a].used) {
				any = true;
				if (!memcmp(p->rx[i][a].sci, sci, 8))
					return p->rx[i][an].used || create ? &p->rx[i][an] : NULL;
			}
		if (!any && free_sc < 0)
			free_sc = i;
	}
	if (create && free_sc >= 0)
		return &p->rx[free_sc][an];
	return NULL;
}

/* cmx_sa_add slice=S dir=tx|rx an=0..3 sci=HEX16 cipher=... key=TOKEN
 *   pn=NEXT_PN [pnhi=HI] [ssci=HEX8] [enable=0|1]
 * tx: enable=1 installs and switches to it (hitless when another SA is
 * active: SecY_SA_Chain + SecY_SA_Switch); enable=0 keeps it pending
 * (in memory) until cmx_sa_enable. rx: installed at once. */
static int c_sa_add(struct args *a)
{
	int s, dir, cipher;
	uint64_t an, pn, pnhi = 0;
	bool enable = true;
	uint8_t sci[8], ssci[4] = { 0, 0, 0, 1 }, key[32], hkey[16], salt[12];
	const char *d = arg(a, "dir");
	if (get_slice(a, &s) || !d || (strcmp(d, "tx") && strcmp(d, "rx")) ||
	    arg_u64(a, "an", &an, 3) != 1 || hexbytes(arg(a, "sci"), sci, 8) ||
	    (cipher = parse_cipher(arg(a, "cipher"))) < 0 || !arg(a, "key") ||
	    arg_u64(a, "pn", &pn, 0xffffffffu) != 1 || arg_u64(a, "pnhi", &pnhi, 0xffffffffu) < 0 ||
	    arg_bool(a, "enable", &enable) < 0 || (arg(a, "ssci") && hexbytes(arg(a, "ssci"), ssci, 4)))
		ERR("bad arguments");
	dir = strcmp(d, "tx") ? RX : TX;
	struct port *p = &ports[s];
	if (!p->up)
		ERR("slice %d not up", s);
	if (pn == 0 && pnhi == 0 && dir == TX)
		ERR("tx pn must be >= 1");
	if (!is_xpn(cipher) && pnhi)
		ERR("pnhi needs an XPN cipher");
	memset(salt, 0, sizeof(salt));
	if (read_keyfile(arg(a, "key"), cipher, key, hkey, salt))
		ERR("key file missing or invalid");
	struct sa *sa = dir == TX ? &p->tx[an] : find_rx(p, sci, an, true);
	int rc = 1;
	secy_sa_t f;
	memset(&f, 0, sizeof(f));
	if (!sa) {
		out("err no free receive SC (max %d peers)", MAXRXSC);
		goto out;
	}
	if (sa->used) {
		out("err %s SA an=%d already exists", dirname_[dir], (int)an);
		goto out;
	}
	memset(sa, 0, sizeof(*sa));
	sa->used = true;
	sa->an = an;
	memcpy(sa->sci, sci, 8);
	sa->cipher = cipher;
	sa->created = time(NULL);
	/* egress: the record holds the last PN sent (0 -> first frame PN 1);
	 * ingress: the next PN expected (frames below it minus the window are
	 * late). 64-bit PN = hi:lo for XPN. */
	uint64_t full = (pnhi << 32) | pn;
	uint64_t seq = dir == TX ? full - 1 : full;
	use_slice(s);
	unsigned dev = dev_of(s, dir);
	if (dir == TX && !enable) {
		sa->pending = true;
		memcpy(sa->key, key, sizeof(key));
		memcpy(sa->hkey, hkey, sizeof(hkey));
		memcpy(sa->salt, salt, sizeof(salt));
		memcpy(sa->ssci, ssci, sizeof(ssci));
		sa->seq_lo = (uint32_t)seq;
		sa->seq_hi = (uint32_t)(seq >> 32);
		out("ok slice=%d dir=tx an=%d state=pending", s, (int)an);
		rc = 0;
		goto out;
	}
	if (build_tr(p, dir, sa, key, hkey, salt, ssci, (uint32_t)seq, (uint32_t)(seq >> 32), &f)) {
		memset(sa, 0, sizeof(*sa));
		out("err SA build failed");
		goto out;
	}
	if (dir == TX) {
		struct sa *old = tx_active(p);
		int r;
		if (!old) {
			r = p_SecY_SA_Add(dev, p->d[TX].vpi_data, &sa->h, &f);
		} else {
			r = p_SecY_SA_Chain(dev, old->h, &sa->h, &f);
			if (!r && p_SecY_SA_Switch(dev, old->h, sa->h)) {
				/* chained but not switched: the SC map is in an unknown
				 * state, so stop transmitting data */
				set_drop(s, TX, true);
				out("err SecY_SA_Switch failed, tx blocked");
				sa->hw = true;
				sa->active = false;
				goto out;
			}
		}
		if (r) {
			memset(sa, 0, sizeof(*sa));
			out("err SecY_SA_%s failed (%d)", old ? "Chain" : "Add", r);
			goto out;
		}
		if (old)
			old->active = false;
		sa->hw = sa->active = true;
	} else {
		sa->active = enable;
		f.Params.Ingress.fSAInUse = true;
		int r = p_SecY_SA_Add(dev, p->d[RX].vpi_data, &sa->h, &f);
		if (r) {
			memset(sa, 0, sizeof(*sa));
			out("err SecY_SA_Add failed (%d)", r);
			goto out;
		}
		sa->hw = true;
		p_SecY_SA_WindowSize_Update(dev, sa->h, p->window);
		if (!enable) {
			secy_sa_t g;
			fill_flow(p, RX, sa, &g);
			p_SecY_SA_Update(dev, sa->h, &g);
		}
	}
	apply_gates(s);
	unsigned sai = 0, sci_i = 0;
	p_SecY_SAIndex_Get(sa->h, &sai, &sci_i);
	out("ok slice=%d dir=%s an=%d sa_index=%u sc_index=%u state=%s", s, dirname_[dir], (int)an, sai, sci_i,
	    sa->active ? "active" : "installed");
	rc = 0;
out:
	free_tr(&f);
	wipe(key, sizeof(key));
	wipe(hkey, sizeof(hkey));
	wipe(salt, sizeof(salt));
	return rc;
}

static struct sa *pick_sa(struct args *a, int *s, int *dir)
{
	uint64_t an;
	uint8_t sci[8];
	const char *d = arg(a, "dir");
	if (get_slice(a, s) || !d || (strcmp(d, "tx") && strcmp(d, "rx")) || arg_u64(a, "an", &an, 3) != 1)
		return NULL;
	*dir = strcmp(d, "tx") ? RX : TX;
	struct port *p = &ports[*s];
	if (!p->up)
		return NULL;
	if (*dir == TX)
		return p->tx[an].used ? &p->tx[an] : NULL;
	if (hexbytes(arg(a, "sci"), sci, 8))
		return NULL;
	return find_rx(p, sci, an, false);
}

/* cmx_sa_enable slice dir an [sci] on=0|1 */
static int c_sa_enable(struct args *a)
{
	int s, dir;
	bool on = true;
	struct sa *sa = pick_sa(a, &s, &dir);
	if (!sa || arg_bool(a, "on", &on) < 0)
		ERR("no such SA / bad arguments");
	struct port *p = &ports[s];
	unsigned dev = dev_of(s, dir);
	use_slice(s);
	if (dir == RX) {
		secy_sa_t f;
		sa->active = on;
		fill_flow(p, RX, sa, &f);
		if (p_SecY_SA_Update(dev, sa->h, &f))
			ERR("SecY_SA_Update failed");
	} else if (on) {
		if (sa->active)
			goto done;
		if (!sa->pending)
			ERR("tx SA an=%d is not pending (retired SAs cannot come back)", sa->an);
		secy_sa_t f;
		memset(&f, 0, sizeof(f));
		int r = build_tr(p, TX, sa, sa->key, sa->hkey, sa->salt, sa->ssci, sa->seq_lo, sa->seq_hi, &f);
		wipe(sa->key, sizeof(sa->key));
		wipe(sa->hkey, sizeof(sa->hkey));
		wipe(sa->salt, sizeof(sa->salt));
		sa->pending = false;
		if (r) {
			sa->used = false;
			ERR("SA build failed");
		}
		struct sa *old = tx_active(p);
		if (!old)
			r = p_SecY_SA_Add(dev, p->d[TX].vpi_data, &sa->h, &f);
		else {
			r = p_SecY_SA_Chain(dev, old->h, &sa->h, &f);
			if (!r && p_SecY_SA_Switch(dev, old->h, sa->h)) {
				free_tr(&f);
				sa->hw = true;
				set_drop(s, TX, true);
				ERR("SecY_SA_Switch failed, tx blocked");
			}
		}
		free_tr(&f);
		if (r) {
			sa->used = false;
			ERR("SecY_SA_%s failed (%d)", old ? "Chain" : "Add", r);
		}
		if (old)
			old->active = false;
		sa->hw = sa->active = true;
	} else if (sa->active) {
		/* the engine cannot un-use the SC's only SA: stop data instead */
		sa->active = false;
	}
done:
	apply_gates(s);
	out("ok slice=%d dir=%s an=%d on=%d", s, dirname_[dir], sa->an, on);
	return 0;
}

/* cmx_sa_del slice dir an [sci] */
static int c_sa_del(struct args *a)
{
	int s, dir;
	struct sa *sa = pick_sa(a, &s, &dir);
	if (!sa)
		ERR("no such SA");
	struct port *p = &ports[s];
	unsigned dev = dev_of(s, dir);
	use_slice(s);
	int an = sa->an;
	if (sa->hw) {
		bool last = dir == TX ? (sa->active) : (count_rx(p, false) == 1);
		if (last && !p->open) {
			/* SAMFlow_Clear would leave a BYPASS flow: drop first */
			if (set_drop(s, dir, true))
				ERR("could not block data before removing the last SA");
		}
		if (p_SecY_SA_Remove(dev, sa->h))
			ERR("SecY_SA_Remove failed");
	}
	wipe(sa, sizeof(*sa));
	apply_gates(s);
	out("ok slice=%d dir=%s an=%d deleted", s, dirname_[dir], an);
	return 0;
}

/* cmx_sa_pn slice dir an [sci] [set=PN sethi=HI] -> next PN */
static int c_sa_pn(struct args *a)
{
	int s, dir;
	uint64_t set, sethi = 0;
	struct sa *sa = pick_sa(a, &s, &dir);
	if (!sa)
		ERR("no such SA");
	int r = arg_u64(a, "set", &set, 0xffffffffu);
	if (r < 0 || arg_u64(a, "sethi", &sethi, 0xffffffffu) < 0)
		ERR("bad pn");
	if (!sa->hw) {
		/* pending tx SA: its PN is what it will start with */
		uint64_t q = ((uint64_t)sa->seq_hi << 32) | sa->seq_lo;
		if (r == 1) {
			uint64_t full = (sethi << 32) | set;
			if (full == 0)
				ERR("tx pn must be >= 1");
			q = full - 1;
			sa->seq_lo = (uint32_t)q;
			sa->seq_hi = (uint32_t)(q >> 32);
		}
		out("ok slice=%d dir=%s an=%d seq=%llu next_pn=%llu xpn=%d pending=1", s, dirname_[dir], sa->an,
		    (unsigned long long)q, (unsigned long long)(q + 1), is_xpn(sa->cipher));
		return 0;
	}
	unsigned dev = dev_of(s, dir);
	use_slice(s);
	if (r == 1) {
		uint64_t full = (sethi << 32) | set;
		uint64_t seq = dir == TX ? full - 1 : full;
		bool written = false;
		if (dir == TX && full == 0)
			ERR("tx pn must be >= 1");
		if (p_SecY_SA_NextPN_Update(dev, sa->h, (uint32_t)seq, (uint32_t)(seq >> 32), &written))
			ERR("SecY_SA_NextPN_Update failed");
		/* the EIP-66 next-PN update reports whether it wrote; on a live
		 * egress SA it does not (hardware 2026-11-16): the egress PN is
		 * set when the SA is installed */
		if (!written)
			ERR("engine did not take the new PN (written=0)");
	}
	uint32_t lo = 0, hi = 0;
	bool ext = false;
	if (p_SecY_SA_NextPN_Get(dev, sa->h, &lo, &hi, &ext))
		ERR("SecY_SA_NextPN_Get failed");
	uint64_t seq = ((uint64_t)hi << 32) | lo;
	out("ok slice=%d dir=%s an=%d seq=%llu next_pn=%llu xpn=%d", s, dirname_[dir], sa->an,
	    (unsigned long long)seq, (unsigned long long)(dir == TX ? seq + 1 : seq), ext);
	return 0;
}

static unsigned long long c64(cnt_t c) { return ((unsigned long long)c.hi << 32) | c.lo; }

/* every read clears the hardware counters (all classes, seen on hardware
 * 2026-11-16): add each read to a running total and report the totals */
static void accum(unsigned long long *acc, const cnt_t *st, int n)
{
	for (int i = 0; i < n; i++)
		acc[i] += c64(st[i]);
}

static void status_sa(int s, int dir, struct sa *sa)
{
	unsigned dev = dev_of(s, dir), sai = 0, sci_i = 0;
	char sci[17];
	hexstr(sa->sci, 8, sci);
	if (!sa->hw) {
		out("sa slice=%d dir=%s an=%d sci=%s cipher=%s state=pending", s, dirname_[dir], sa->an, sci,
		    cipher_name[sa->cipher]);
		return;
	}
	uint32_t lo = 0, hi = 0;
	bool ext = false;
	p_SecY_SAIndex_Get(sa->h, &sai, &sci_i);
	p_SecY_SA_NextPN_Get(dev, sa->h, &lo, &hi, &ext);
	uint64_t seq = ((uint64_t)hi << 32) | lo;
	char head[256];
	snprintf(head, sizeof(head), "sa slice=%d dir=%s an=%d sci=%s cipher=%s state=%s sa_index=%u sc_index=%u age=%lld seq=%llu next_pn=%llu",
		 s, dirname_[dir], sa->an, sci, cipher_name[sa->cipher],
		 dir == TX ? (sa->active ? "active" : "retired") : (sa->active ? "in_use" : "disabled"), sai, sci_i,
		 (long long)(time(NULL) - sa->created), (unsigned long long)seq,
		 (unsigned long long)(dir == TX ? seq + 1 : seq));
	cnt_t st[16];
	unsigned long long *t = sa->acc;
	memset(st, 0, sizeof(st));
	if (dir == TX) {
		if (!p_SecY_SA_Statistics_E_Get(dev, sa->h, st, true))
			accum(t, st, 4);
		out("%s OutPktsEncryptedProtected=%llu OutPktsTooLong=%llu OutPktsSANotInUse=%llu OutOctetsEncryptedProtected=%llu",
		    head, t[0], t[1], t[2], t[3]);
	} else {
		if (!p_SecY_SA_Statistics_I_Get(dev, sa->h, st, true))
			accum(t, st, 10);
		out("%s InPktsUnchecked=%llu InPktsDelayed=%llu InPktsLate=%llu InPktsOK=%llu InPktsInvalid=%llu InPktsNotValid=%llu InPktsNotUsingSA=%llu InPktsUnusedSA=%llu InOctetsDecrypted=%llu InOctetsValidated=%llu",
		    head, t[0], t[1], t[2], t[3], t[4], t[5], t[6], t[7], t[8], t[9]);
	}
}

static void status_rule(int s, int dir, const char *kind, void *h, bool drop, unsigned long long *acc)
{
	unsigned idx = 0;
	cnt_t c = { 0, 0 };
	if (!h)
		return;
	p_CfyE_RuleIndex_Get(h, &idx);
	if (!p_CfyE_Statistics_TCAM_Get(dev_of(s, dir), idx, &c, true))
		*acc += c64(c);
	out("rule slice=%d dir=%s kind=%s index=%u drop=%d hits=%llu", s, dirname_[dir], kind, idx, drop, *acc);
}

/* cmx_status [slice=S] */
static int c_status(struct args *a)
{
	int from = 1, to = NSLICE, s;
	if (arg(a, "slice")) {
		if (get_slice(a, &s))
			ERR("bad slice");
		from = to = s;
	}
	out("ok shim=" SHIM_VERSION);
	for (s = from; s <= to; s++) {
		struct port *p = &ports[s];
		if (!use_slice(s))
			continue;
		bool init = dev_ready(dev_of(s, RX)) && dev_ready(dev_of(s, TX));
		if (!p->up) {
			out("port slice=%d mode=%s", s, init ? "engine-initialised-unmanaged" : "bypass");
			continue;
		}
		out("port slice=%d mode=macsec channel=%u controlled=%d open=%d protect=%d encrypt=%d include_sci=%d es=%d scb=%d conf_offset=%u replay=%d window=%u validate=%s tx_drop=%d rx_drop=%d",
		    s, p->ch, p->controlled, p->open, p->protect, p->encrypt, p->include_sci, p->es, p->scb,
		    p->conf_offset, p->replay, p->window,
		    p->validate == 2 ? "strict" : p->validate == 1 ? "check" : "disabled", p->d[TX].drop,
		    p->d[RX].drop);
		for (int dir = 0; dir < 2; dir++) {
			struct dirst *d = &p->d[dir];
			unsigned dev = dev_of(s, dir);
			cnt_t st[16];
			unsigned long long *a1 = d->acc_secy, *a2 = d->acc_ifc;
			status_rule(s, dir, "eapol", d->rule_eapol, false, &d->acc_hits_eapol);
			status_rule(s, dir, "data", d->rule_data, d->drop, &d->acc_hits_data);
			memset(st, 0, sizeof(st));
			if (dir == TX) {
				if (!p_SecY_SecY_Statistics_E_Get(dev, d->vpi_data, st, true))
					accum(a1, st, 3);
				out("secy slice=%d dir=tx OutPktsTransformError=%llu OutPktsControl=%llu OutPktsUntagged=%llu",
				    s, a1[0], a1[1], a1[2]);
				memset(st, 0, sizeof(st));
				if (!p_SecY_Ifc_Statistics_E_Get(dev, d->vpi_data, st, true))
					accum(a2, st, 9);
				out("ifc slice=%d dir=tx OutPktsUnicastUncontrolled=%llu OutPktsMulticastUncontrolled=%llu OutPktsBroadcastUncontrolled=%llu OutPktsUnicastControlled=%llu OutPktsMulticastControlled=%llu OutPktsBroadcastControlled=%llu OutOctetsUncontrolled=%llu OutOctetsControlled=%llu OutOctetsCommon=%llu",
				    s, a2[0], a2[1], a2[2], a2[3], a2[4], a2[5], a2[6], a2[7], a2[8]);
				for (int an = 0; an < 4; an++)
					if (p->tx[an].used)
						status_sa(s, TX, &p->tx[an]);
			} else {
				if (!p_SecY_SecY_Statistics_I_Get(dev, d->vpi_data, st, true))
					accum(a1, st, 8);
				out("secy slice=%d dir=rx InPktsTransformError=%llu InPktsControl=%llu InPktsUntagged=%llu InPktsNoTag=%llu InPktsBadTag=%llu InPktsNoSCI=%llu InPktsUnknownSCI=%llu InPktsTaggedCtrl=%llu",
				    s, a1[0], a1[1], a1[2], a1[3], a1[4], a1[5], a1[6], a1[7]);
				memset(st, 0, sizeof(st));
				if (!p_SecY_Ifc_Statistics_I_Get(dev, d->vpi_data, st, true))
					accum(a2, st, 8);
				out("ifc slice=%d dir=rx InPktsUnicastUncontrolled=%llu InPktsMulticastUncontrolled=%llu InPktsBroadcastUncontrolled=%llu InPktsUnicastControlled=%llu InPktsMulticastControlled=%llu InPktsBroadcastControlled=%llu InOctetsUncontrolled=%llu InOctetsControlled=%llu",
				    s, a2[0], a2[1], a2[2], a2[3], a2[4], a2[5], a2[6], a2[7]);
				for (int i = 0; i < MAXRXSC; i++)
					for (int an = 0; an < 4; an++)
						if (p->rx[i][an].used)
							status_sa(s, RX, &p->rx[i][an]);
			}
		}
	}
	return 0;
}

/* ---- dispatch ---- */

typedef int (*cmdfn)(struct args *);

static int run(char *line, cmdfn fn)
{
	struct args a;
	struct stat st;
	uint64_t req = 0;
	int rc;
	char *copy = strdup(line);

	if (!copy)
		return -1;
	if (parse(copy, &a) || arg_u64(&a, "req", &req, 0xffffffffffffull) != 1) {
		free(copy);
		return -1;	/* no request id: nowhere to answer */
	}
	if (sys_lstat(BASEDIR, &st) || !S_ISDIR(st.st_mode) || st.st_uid != 0 || (st.st_mode & 077)) {
		free(copy);
		return -1;
	}
	snprintf(resp_tmp, sizeof(resp_tmp), BASEDIR "/r-%llu.tmp", (unsigned long long)req);
	snprintf(resp_path, sizeof(resp_path), BASEDIR "/r-%llu", (unsigned long long)req);
	int fd = open(resp_tmp, O_WRONLY | O_CREAT | O_TRUNC | O_NOFOLLOW | O_CLOEXEC, 0600);
	if (fd < 0 || !(resp = fdopen(fd, "w"))) {
		if (fd >= 0)
			close(fd);
		free(copy);
		return -1;
	}
	if (!ready)
		rc = (out("err shim not ready: %s", init_err), 1);
	else
		rc = fn(&a);
	out("end rc=%d", rc);
	fclose(resp);
	resp = NULL;
	rename(resp_tmp, resp_path);
	wipe(copy, strlen(line));
	free(copy);
	return rc;
}

#define CMD(name) static int x_##name(char *l) { return run(l, c_##name); }
CMD(hello) CMD(port_up) CMD(port_down) CMD(port_reset) CMD(policy) CMD(sa_add) CMD(sa_enable) CMD(sa_del) CMD(sa_pn) CMD(status)
#undef CMD

static struct { char name[512]; int (*fn)(char *); } table[] = {
	{ "cmx_hello", x_hello },
	{ "cmx_port_up", x_port_up },
	{ "cmx_port_down", x_port_down },
	{ "cmx_port_reset", x_port_reset },
	{ "cmx_policy", x_policy },
	{ "cmx_sa_add", x_sa_add },
	{ "cmx_sa_enable", x_sa_enable },
	{ "cmx_sa_del", x_sa_del },
	{ "cmx_sa_pn", x_sa_pn },
	{ "cmx_status", x_status },
};

/* phy_server's main(): init the command list, register_credo_cmds() (vendor
 * commands, Driver164_Init), optional config file, then daemon(1, 1) once,
 * then the FIFO loop. Hooking daemon() adds our commands at exactly that
 * point. daemon() itself is reimplemented (glibc's: one fork, setsid). */
int daemon(int nochdir, int noclose)
{
	if (!registered && register_cmds && credo_cmds) {
		registered = 1;
		ready = !resolve();
		register_cmds(table, sizeof(table) / sizeof(table[0]));
		fprintf(stderr, "cs8210-macsec-shim %s: %s\n", SHIM_VERSION, ready ? "ready" : init_err);
	}
	pid_t pid = fork();
	if (pid < 0)
		return -1;
	if (pid > 0)
		_exit(0);
	if (setsid() < 0)
		return -1;
	if (!nochdir && chdir("/"))
		return -1;
	if (!noclose) {
		int fd = open("/dev/null", O_RDWR);
		if (fd < 0)
			return -1;
		dup2(fd, 0);
		dup2(fd, 1);
		dup2(fd, 2);
		if (fd > 2)
			close(fd);
	}
	return 0;
}
