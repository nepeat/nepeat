#!/usr/bin/env python3
"""Verify an R6 SADP password-reset response against a DeviceKey."""

import argparse
import base64
import binascii

from cryptography.hazmat.primitives.serialization import load_der_public_key


PUBLIC_KEYS = {
    "key0": (
        "MIGJAoGBAOgj9E3AFyFNBTY11p97Lmdd5WLnRISiFRd31KSp3DQT+38lWkY9DLGP"
        "lrQR38uvqo/zlzxlAEqxlRH8GQxfOytQJ/7FrTf7XqFBh+v7wJp9YqBSOGb2XoQ"
        "PWode/IWqgmMs+2L8LUqvHPtHE8WUpO9HPE0wSXHfqSz3sbAE/p1DAgMBAAE="
    ),
    "key1": (
        "MIGJAoGBAOZwSLecBmsjYjEixnXdeYfDeZJ39mDk6CH/cduiKSYz9KHAT6uqvWsY"
        "A5kT6JtWfitnl6fnPSd4/K9DYsVEMxs8esFElmV+HqVo8owInBkHAol++kbH4SPw"
        "4L+RxkOgZ5zQuVlrZ1l6Lr08+Uli6clxxG2f7WxH8bEtyURJqPLzAgMBAAE="
    ),
}


def recover_payload(signature: bytes, encoded_key: str) -> bytes | None:
    public_key = load_der_public_key(base64.b64decode(encoded_key))
    numbers = public_key.public_numbers()
    size = (numbers.n.bit_length() + 7) // 8
    if len(signature) != size:
        return None

    block = pow(int.from_bytes(signature, "big"), numbers.e, numbers.n).to_bytes(size, "big")
    if not block.startswith(b"\x00\x01"):
        return None
    separator = block.find(b"\x00", 2)
    padding = block[2:separator]
    if separator < 10 or any(byte != 0xFF for byte in padding):
        return None
    return block[separator + 1 :]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("device_key", help="base64 DeviceKey exported by SADP")
    parser.add_argument("security_code", help="base64 RSA response from Hikvision")
    args = parser.parse_args()

    try:
        challenge = base64.b64decode(args.device_key, validate=True)
        signature = base64.b64decode(args.security_code, validate=True)
    except binascii.Error as error:
        parser.error(f"invalid base64: {error}")

    print(f"DeviceKey bytes: {challenge.hex()}")
    for name, encoded_key in PUBLIC_KEYS.items():
        payload = recover_payload(signature, encoded_key)
        if payload == challenge:
            print(f"valid response using {name}")
            return 0
        if payload is not None:
            print(f"{name} recovered: {payload.hex()}")
    print("response does not validate with either R6 key")
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
