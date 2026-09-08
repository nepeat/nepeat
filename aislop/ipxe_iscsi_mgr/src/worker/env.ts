export interface Env {
  DB: D1Database;
  SECRET_CACHE: KVNamespace;
  /** Bearer token for the admin API. `wrangler secret put ADMIN_TOKEN`. */
  ADMIN_TOKEN: string;
  /** Naming-authority prefix for generated initiator IQNs. */
  INITIATOR_IQN_PREFIX: string;
  /** Public base URL of this worker, used in generated scripts. */
  MGR_URL: string;
  /** Seconds a general-pool lease is held. */
  LEASE_TTL_SECONDS: string;
  /** Netboot server base URL, e.g. http://10.36.75.7. Enables the reload,
   *  check-in and memtest entries in the boot-failure menu. Optional. */
  BOOT_SERVER?: string;
  /** Firmware images, served unauthenticated from /fw/*. */
  FIRMWARE: R2Bucket;
}

export function now(): number {
  return Math.floor(Date.now() / 1000);
}

export function id(prefix: string): string {
  return `${prefix}_${crypto.randomUUID().replace(/-/g, '').slice(0, 16)}`;
}

export async function sha256Hex(input: string): Promise<string> {
  const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(input));
  return [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, '0')).join('');
}

/** Length-independent equality for hex digests of equal length. */
export function timingSafeEqual(a: string, b: string): boolean {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) diff |= a.charCodeAt(i) ^ b.charCodeAt(i);
  return diff === 0;
}

/** aa-bb-cc-dd-ee-ff and AA:BB:... both normalise to aa:bb:cc:dd:ee:ff. */
export function normalizeMac(mac: string | null | undefined): string | null {
  if (!mac) return null;
  const hex = mac.replace(/[^0-9a-fA-F]/g, '').toLowerCase();
  if (hex.length !== 12) return null;
  return (hex.match(/.{2}/g) ?? []).join(':');
}

export function normalizeUuid(uuid: string | null | undefined): string | null {
  if (!uuid) return null;
  const v = uuid.trim().toLowerCase();
  if (!/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(v)) return null;
  // SMBIOS on some firmware reports an all-zero or all-ff UUID for every unit.
  if (/^[0f-]+$/.test(v)) return null;
  return v;
}

/** iPXE renders raw SMBIOS byte fields as little-endian colon-hex: "00:00:40:00". */
function colonHexLe(value: string | null | undefined): number | null {
  if (!value) return null;
  const bytes = value.split(':').map((b) => parseInt(b, 16));
  if (!bytes.length || bytes.some((b) => Number.isNaN(b))) return null;
  return bytes.reduce((acc, b, i) => acc + b * 2 ** (8 * i), 0);
}

/**
 * Installed RAM, best effort. `${memsize}` is authoritative but BIOS-only --
 * it is empty under UEFI, which is most machines now. The SMBIOS fallbacks are
 * approximations and are labelled as such so nobody mistakes them for a real
 * total: type 17 reports one DIMM, type 16 reports what the board could hold.
 */
export function decodeMem(facts: {
  mem: number | null;
  mem16?: string | null;
  mem17?: string | null;
}): { mb: number | null; source: string | null } {
  if (facts.mem && facts.mem > 0) return { mb: facts.mem, source: 'memsize' };
  const dimm0 = colonHexLe(facts.mem17);
  if (dimm0 && dimm0 > 0 && dimm0 !== 0xffff) return { mb: dimm0, source: 'smbios17-dimm0' };
  const maxKb = colonHexLe(facts.mem16);
  if (maxKb && maxKb > 0) return { mb: Math.floor(maxKb / 1024), source: 'smbios16-max' };
  return { mb: null, source: null };
}

/** Trim and drop the empty/placeholder strings SMBIOS is famous for. */
export function cleanSmbios(value: string | null | undefined): string | null {
  if (!value) return null;
  const v = value.trim();
  if (!v) return null;
  const junk = /^(to be filled by o\.?e\.?m\.?|system serial number|default string|not specified|none|n\/a|unknown|0+)$/i;
  return junk.test(v) ? null : v;
}
