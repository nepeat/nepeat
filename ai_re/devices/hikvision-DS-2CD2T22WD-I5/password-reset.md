# SADP password reset

Initial recovery used CVE-2018-6414 rather than either vendor reset-code mode.
The successful path and exact firmware addresses are documented in
[web-overflow.md](web-overflow.md). A separate V5.5.82 factory-reset validation
is recorded below.

## Latest-firmware local reset

The live V5.5.82 filesystem creates `/bin/resetPasswd` and `/bin/resetParam` as
links to `/bin/da_info`. Static analysis maps them to local application command
IDs `0x13` and `0x14`, respectively, and both require zero arguments.

`resetPasswd` completed without an error but did not change this camera. The
existing administrator credential still returned HTTP 200, factory and empty
passwords returned 401, and byte-for-byte dumps of both config banks were
unchanged. It should not be treated as an IPC recovery method on this build.

`resetParam` was then tested on the same running V5.5.82 image. It changed the
active `cfg_pri` bank and stopped web service. After a manual reboot, the camera
appeared at `192.168.1.64`; old, factory, and empty credentials all received
HTTP 403 because the device was inactive. The camera was successfully
reactivated with [activate_camera.py](activate_camera.py), returned to DHCP at
`10.60.2.77`, and still reported `V5.5.82 build 190909`.

This proves the full-reset operation in the latest firmware without changing
the kernel, application, recovery, or bootloader partitions.

## Locked-owner UART route

The read-only flash dump from this camera contains its actual Ambarella R6
bootloader. It reports `U-Boot 3.1.6`, prompts `Hit Ctrl+u to stop autoboot:`,
and enters an `HKVS #` console. Its built-in help includes `erase`, `reset`,
`cfg_pri`, and `cfg_sec`; no bootloader password or console-authentication path
appears in the binary. Linux is also launched with `console=ttyS0`.

For another locked unit, the configuration-only bootloader procedure is:

```text
1. Connect to the internal 3.3 V TTL UART with adapter VCC disconnected.
2. Power on and send Ctrl+u during the autoboot countdown.
3. At HKVS #, inspect `help erase` and `show ptb`.
4. Erase `cfg_pri` and `cfg_sec`, then run `reset`.
5. Discover the inactive camera at 192.168.1.64 and activate it with a new password.
```

Both config banks must be cleared so the redundant copy cannot restore the old
account database. Do not erase `bst`, `bld`, `ptb`, `env`, `krn_*`, `app_*`, or
`rcvy`. The reset result is live-verified on V5.5.82, and the entry point and
commands are verified against this unit's bootloader dump. The board UART pad
order has not been electrically mapped on this particular unit; follow
[uart.md](uart.md) before connecting an adapter.

## Example exported challenge

An earlier `getcode` response returned this DeviceKey:

```text
AwAAADY3MDQ5NTc2NFp/rBI=
```

Decoded bytes:

```text
03 00 00 00 36 37 30 34 39 35 37 36 34 5a 7f ac 12
```

That DeviceKey expired when the camera rebooted. Export a new one for any signed response attempt; reset artifacts are tied to the current boot and reset session.

## Signed DeviceKey mode

The SADP client requests the challenge with:

```xml
<?xml version="1.0" encoding="utf-8"?><Probe><Uuid>...</Uuid><MAC>a4-14-37-d1-c1-4f</MAC><Types>getcode</Types></Probe>
```

SADP writes the response's base64 `Code` text directly into the exported DeviceKey file. It does not calculate or wrap the challenge locally.

The verifier at virtual address `0x14d908` in the matching R6 `davinci` executable performs this operation:

```text
challenge = base64_decode(DeviceKey)
signature = base64_decode(SecurityCode)
recovered = RSA_public_decrypt(signature, embedded_public_key, RSA_PKCS1_PADDING)
accept if recovered == challenge
```

The corresponding server calculation is:

```text
SecurityCode = base64_encode(RSA_private_encrypt(challenge, private_key, RSA_PKCS1_PADDING))
```

This is PKCS#1 v1.5 type-1 padding over the raw challenge. There is no date or serial-number calculation in this verifier.

## Encrypted-string mode

The second mode is split across three SADP operations:

1. `getencryptstring` creates a random 32-character lowercase hexadecimal secret `A`. It stores `MD5(A).hexdigest()[:8]` as the expected reset code. The response contains the label serial followed by `A`, encrypted with an embedded Hikvision RSA public key.
2. `exchangecode` creates a distinct random 32-character hexadecimal secret `B` and encrypts it with a client-supplied RSA public key. The client can recover `B` with its own private key.
3. `reset` accepts the eight-character code derived from `A`. It decrypts the proposed password with AES-128-ECB using `B[:16]`; the plaintext is `B[:16]` followed by the password and zero padding.

The reset-code calculation itself is therefore simple, but the camera never discloses `A` in plaintext. The corresponding private key is held by Hikvision's reset service. `A` is generated from timestamps plus 1 KiB read from `/dev/urandom`, so it cannot be reconstructed from `B`, the packet timestamp, serial number, or verification code.

The public SADP keygen described in older blog posts calculates a date-and-serial code for pre-5.3 firmware. It does not implement either verifier used by this V5.5.0 build.

### Exact random-code derivation

Ghidra decompilation of `davinci` confirms the raw-code path at `0x14d908` and generator at `0x14d7e4`. The camera constructs `A` as:

```text
seed = sum_32bit(256 words read from /dev/urandom) + tv_sec + tv_usec
r = libc_rand_after_srand(seed)
A = MD5(hex(tv_sec + tv_usec) || hex(r) || "12345678").hexdigest()
code = MD5(A).hexdigest()[:8]
```

The 1 KiB of kernel randomness is collapsed to a 32-bit seed, but the RSA-encrypted `A` does not provide an offline guess test. PKCS#1 v1.5 encryption uses randomized padding, so encrypting a candidate and comparing ciphertexts does not work. The live reset endpoint only reveals success or failure and limits each generated secret to five failed attempts.

The reset parser treats a `Code` of exactly eight characters as the raw-code mode. Other lengths are base64-decoded and sent to the signed DeviceKey verifier. This prevents a zero-length `memcmp` or short-prefix comparison from reaching the raw-code verifier.

## Embedded public keys

The signed DeviceKey verifier contains two runtime-selected 1024-bit RSA public keys. Both use exponent 65537 and are PKCS#1 `RSAPublicKey` DER encoded below.

```text
MIGJAoGBAOgj9E3AFyFNBTY11p97Lmdd5WLnRISiFRd31KSp3DQT+38lWkY9DLGPlrQR38uvqo/zlzxlAEqxlRH8GQxfOytQJ/7FrTf7XqFBh+v7wJp9YqBSOGb2XoQPWode/IWqgmMs+2L8LUqvHPtHE8WUpO9HPE0wSXHfqSz3sbAE/p1DAgMBAAE=
```

```text
MIGJAoGBAOZwSLecBmsjYjEixnXdeYfDeZJ39mDk6CH/cduiKSYz9KHAT6uqvWsYA5kT6JtWfitnl6fnPSd4/K9DYsVEMxs8esFElmV+HqVo8owInBkHAol++kbH4SPw4L+RxkOgZ5zQuVlrZ1l6Lr08+Uli6clxxG2f7WxH8bEtyURJqPLzAgMBAAE=
```

The encrypted-string path contains two additional 1024-bit public keys. All four moduli are pairwise coprime and FactorDB reports each as an unfactored composite. The matching private keys are absent from the firmware and SADP client. A response therefore cannot be generated locally from these artifacts alone.

Use `reset_code.py` to check a returned SecurityCode against both keys:

```sh
./reset_code.py 'DeviceKey' 'SecurityCode'
```

## Firmware evidence

The matching `V5.5.0 build 170725` firmware contains an encrypted inner `davinci` executable. `hikio.ko` supplies a static 14-byte application key through ioctl `0x8004480f`. `daemon_fsp_app` uses it to unwrap the archive password `6RsSl5v3cRyptor6`, then decrypts the archive using AES-256-ECB and the vendor's double-MD5 `EVP_BytesToKey` derivation with salt `HangZhou`.

The recovered ARM executable has SHA-256 `0dc5c3ee5688dc979911bcd60a1e2cc5bce5ad974c59556923869a48fbf37c72`. The vendor archive has a damaged gzip trailer, but its complete ELF payload extracts successfully.
