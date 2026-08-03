/** Ed25519 request verification for Discord interaction webhooks. */

export function hexToBytes(hex: string): Uint8Array {
  const clean = hex.trim();
  if (clean.length % 2 !== 0 || /[^0-9a-fA-F]/.test(clean)) {
    throw new Error('invalid hex');
  }
  const out = new Uint8Array(clean.length / 2);
  for (let i = 0; i < out.length; i++) {
    out[i] = Number.parseInt(clean.slice(i * 2, i * 2 + 2), 16);
  }
  return out;
}

// workerd and Node 22 both expose "Ed25519"; older workerd only "NODE-ED25519".
const ALGORITHMS = ['Ed25519', 'NODE-ED25519'] as const;

async function importKey(publicKey: string): Promise<CryptoKey> {
  const raw = hexToBytes(publicKey);
  let lastErr: unknown;
  for (const name of ALGORITHMS) {
    try {
      return await crypto.subtle.importKey(
        'raw',
        raw as unknown as BufferSource,
        { name, namedCurve: 'NODE-ED25519' } as unknown as Parameters<
          typeof crypto.subtle.importKey
        >[2],
        false,
        ['verify'],
      );
    } catch (err) {
      lastErr = err;
    }
  }
  throw lastErr instanceof Error ? lastErr : new Error('ed25519 unsupported');
}

/**
 * Verify `X-Signature-Ed25519` over `timestamp + body`.
 * Returns false (never throws) for any malformed input.
 */
export async function verifyDiscordRequest(
  body: string,
  signature: string | null,
  timestamp: string | null,
  publicKey: string,
): Promise<boolean> {
  if (!signature || !timestamp || !publicKey) return false;
  let sig: Uint8Array;
  try {
    sig = hexToBytes(signature);
  } catch {
    return false;
  }
  if (sig.length !== 64) return false;

  let key: CryptoKey;
  try {
    key = await importKey(publicKey);
  } catch {
    return false;
  }

  const data = new TextEncoder().encode(timestamp + body);
  try {
    return await crypto.subtle.verify(
      key.algorithm.name,
      key,
      sig as unknown as BufferSource,
      data as unknown as BufferSource,
    );
  } catch {
    return false;
  }
}
