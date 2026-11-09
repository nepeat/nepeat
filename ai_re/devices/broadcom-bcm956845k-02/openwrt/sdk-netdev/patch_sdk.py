#!/usr/bin/env python3
"""Append our own SDK/TAP adapter to the exact owned factory SDK. No KNET claim.

Uses one optional PT_NOTE program header for an explicit extra executable
segment. The disabled bcm_esw_knet_init stub becomes the adapter bootstrap;
all other KNET APIs remain unavailable. No existing SDK text is moved.
"""
import hashlib, json, struct
from pathlib import Path
ORIGINAL_SHA256='ea4fdc1f4e172cb641e64abd6cd239baaaaa762f2eab1a67ce34992de75d96a8'
def patch_sdk(original: bytes, build: Path):
    if hashlib.sha256(original).hexdigest()!=ORIGINAL_SHA256: raise ValueError('Unsupported SDK hash')
    data=bytearray(original.replace(b'/dev/mem\0',b'/dev/bcm\0'))
    meta=json.loads((build/'bridge.json').read_text());payload=(build/'bridge.bin').read_bytes()
    if len(payload)!=meta['filesz'] or meta['address']!=0x11400000: raise ValueError('Unexpected adapter layout')
    offset=struct.unpack_from('>I',data,28)[0];size,count=struct.unpack_from('>HH',data,42)
    headers=[(offset+i*size,struct.unpack_from('>8I',data,offset+i*size)) for i in range(count)]
    notes=[o for o,p in headers if p[0]==4]
    if len(notes)!=1: raise ValueError('Expected one replaceable PT_NOTE header')
    hook=0x10422ee4
    loads=[p for _,p in headers if p[0]==1]
    if any(p[2]+p[5]>meta['address'] for p in loads): raise ValueError('Adapter overlaps SDK')
    segment=next(p for p in loads if p[2]<=hook<p[2]+p[4]);patch=segment[1]+hook-segment[2]
    if data[patch:patch+8]!=bytes.fromhex('3860fff04e800020'): raise ValueError('KNET stub differs')
    delta=meta['entry']-hook
    if delta%4 or not -0x2000000<=delta<0x2000000: raise ValueError('PPC branch out of range')
    struct.pack_into('>I',data,patch,0x48000000|(delta&0x03fffffc))
    fileoffset=(len(data)+0xffff)&~0xffff
    data.extend(b'\0'*(fileoffset-len(data)));data.extend(payload)
    struct.pack_into('>8I',data,notes[0],1,fileoffset,meta['address'],meta['address'],len(payload),meta['memsz'],7,0x10000)
    return bytes(data)
if __name__=='__main__':
    import argparse
    p=argparse.ArgumentParser();p.add_argument('sdk',type=Path);p.add_argument('build',type=Path);p.add_argument('output',type=Path);a=p.parse_args();a.output.write_bytes(patch_sdk(a.sdk.read_bytes(),a.build))
