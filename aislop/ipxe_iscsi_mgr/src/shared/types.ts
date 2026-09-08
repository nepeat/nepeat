import { z } from 'zod';

/**
 * RFC 3720 IQN. Deliberately loose on the suffix -- LIO accepts a lot -- but
 * strict on the `iqn.YYYY-MM.reversed.domain` prefix, which is where typos
 * actually happen.
 */
export const iqnSchema = z
  .string()
  .regex(
    /^iqn\.\d{4}-\d{2}\.[a-z0-9.-]+(?::.+)?$/,
    'must look like iqn.YYYY-MM.reversed.domain[:unique-id]',
  )
  .max(223);

export const archSchema = z.enum(['x86_64', 'i386', 'arm64']);
export type Arch = z.infer<typeof archSchema>;

export const hostStateSchema = z.enum(['pending', 'approved', 'disabled']);
export type HostState = z.infer<typeof hostStateSchema>;

/** How a volume may be handed out. See PLAN.md section 4. */
export const assignModeSchema = z.enum(['pinned', 'general-exclusive', 'shared-ro']);
export type AssignMode = z.infer<typeof assignModeSchema>;

export const volumeSchema = z.object({
  id: z.string(),
  name: z.string().min(1),
  provider: z.literal('zfs').default('zfs'),
  zvolPath: z.string().startsWith('/dev/zvol/'),
  portalHost: z.string().min(1),
  portalPort: z.number().int().min(1).max(65535).default(3260),
  lun: z.number().int().min(0).default(0),
  targetIqn: iqnSchema,
  sizeBytes: z.number().int().nonnegative().nullable().default(null),
  state: z.enum(['active', 'offline']).default('active'),
});
export type Volume = z.infer<typeof volumeSchema>;

export const hostSchema = z.object({
  id: z.string(),
  name: z.string().min(1),
  state: hostStateSchema,
  smbiosUuid: z.string().nullable().default(null),
  serial: z.string().nullable().default(null),
  assetTag: z.string().nullable().default(null),
  manufacturer: z.string().nullable().default(null),
  product: z.string().nullable().default(null),
  cpuModel: z.string().nullable().default(null),
  memMb: z.number().int().nonnegative().nullable().default(null),
  arch: archSchema.default('x86_64'),
  platform: z.enum(['efi', 'pcbios']).default('efi'),
  initiatorIqn: iqnSchema,
  bootProfileId: z.string().default('generic-efi'),
});
export type Host = z.infer<typeof hostSchema>;

export const bootProfileSchema = z.object({
  id: z.string(),
  name: z.string().min(1),
  arch: archSchema,
  /** EFI bootloader paths, tried in order. Backslash-separated, ESP-relative. */
  efiCandidates: z.array(z.string().startsWith('\\')).min(1),
  extraIpxe: z.string().nullable().default(null),
});
export type BootProfile = z.infer<typeof bootProfileSchema>;

/**
 * Facts a booting machine reports about itself. Everything is optional --
 * a machine with no SMBIOS still deserves a boot script.
 */
export const bootFactsSchema = z.object({
  mac: z.string().nullable().default(null),
  uuid: z.string().nullable().default(null),
  serial: z.string().nullable().default(null),
  asset: z.string().nullable().default(null),
  manufacturer: z.string().nullable().default(null),
  product: z.string().nullable().default(null),
  cpu: z.string().nullable().default(null),
  cpuvendor: z.string().nullable().default(null),
  /** `${memsize}` in MB. Empty under UEFI -- see mem16/mem17. */
  mem: z
    .string()
    .nullable()
    .default(null)
    .transform((v) => (v && /^\d+$/.test(v) ? Number(v) : null)),
  /** SMBIOS type 16 Maximum Capacity, colon-hex little-endian DWORD of KB. */
  mem16: z.string().nullable().default(null),
  /** SMBIOS type 17 DIMM 0 Size, colon-hex little-endian WORD of MB. */
  mem17: z.string().nullable().default(null),
  arch: archSchema.nullable().default(null),
  platform: z.enum(['efi', 'pcbios']).nullable().default(null),
});
export type BootFacts = z.infer<typeof bootFactsSchema>;
