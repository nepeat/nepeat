import { webcrypto } from 'node:crypto';
import { beforeAll, describe, expect, it } from 'vitest';
import { hexToBytes, verifyDiscordRequest } from '../src/discord/verify';

const subtle = webcrypto.subtle;
type NodeCryptoKey = webcrypto.CryptoKey;

function toHex(buf: ArrayBuffer): string {
  return [...new Uint8Array(buf)].map((b) => b.toString(16).padStart(2, '0')).join('');
}

async function keypair(): Promise<{ publicKeyHex: string; privateKey: NodeCryptoKey }> {
  const pair = (await subtle.generateKey({ name: 'Ed25519' }, true, [
    'sign',
    'verify',
  ])) as webcrypto.CryptoKeyPair;
  return {
    privateKey: pair.privateKey,
    publicKeyHex: toHex(await subtle.exportKey('raw', pair.publicKey)),
  };
}

describe('verifyDiscordRequest', () => {
  let publicKeyHex: string;
  let privateKey: NodeCryptoKey;

  beforeAll(async () => {
    ({ publicKeyHex, privateKey } = await keypair());
  });

  async function sign(timestamp: string, body: string): Promise<string> {
    const data = new TextEncoder().encode(timestamp + body);
    return toHex(await subtle.sign({ name: 'Ed25519' }, privateKey, data));
  }

  it('accepts a correctly signed request', async () => {
    const body = JSON.stringify({ type: 1 });
    const ts = '1700000000';
    await expect(verifyDiscordRequest(body, await sign(ts, body), ts, publicKeyHex)).resolves.toBe(
      true,
    );
  });

  it('rejects a tampered body', async () => {
    const ts = '1700000000';
    const sig = await sign(ts, '{"type":1}');
    await expect(verifyDiscordRequest('{"type":2}', sig, ts, publicKeyHex)).resolves.toBe(false);
  });

  it('rejects a replayed signature under a different timestamp', async () => {
    const body = '{"type":1}';
    const sig = await sign('1700000000', body);
    await expect(verifyDiscordRequest(body, sig, '1700000001', publicKeyHex)).resolves.toBe(false);
  });

  it('rejects a wrong public key', async () => {
    const body = '{"type":1}';
    const ts = '1700000000';
    const sig = await sign(ts, body);
    const other = await keypair();
    await expect(verifyDiscordRequest(body, sig, ts, other.publicKeyHex)).resolves.toBe(false);
  });

  it('rejects missing / malformed headers without throwing', async () => {
    await expect(verifyDiscordRequest('{}', null, '1', publicKeyHex)).resolves.toBe(false);
    await expect(verifyDiscordRequest('{}', 'ff', null, publicKeyHex)).resolves.toBe(false);
    await expect(verifyDiscordRequest('{}', 'zzzz', '1', publicKeyHex)).resolves.toBe(false);
    await expect(verifyDiscordRequest('{}', 'ab', '1', publicKeyHex)).resolves.toBe(false);
    await expect(verifyDiscordRequest('{}', 'ab'.repeat(64), '1', '')).resolves.toBe(false);
    await expect(verifyDiscordRequest('{}', 'ab'.repeat(64), '1', 'nothex')).resolves.toBe(false);
  });
});

describe('hexToBytes', () => {
  it('decodes and rejects', () => {
    expect([...hexToBytes('00ff10')]).toEqual([0, 255, 16]);
    expect(() => hexToBytes('abc')).toThrow();
    expect(() => hexToBytes('zz')).toThrow();
  });
});
