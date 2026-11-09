#!/usr/bin/env python3
"""Build a freestanding TAP adapter for the hash-locked factory SDK."""
from pathlib import Path
import argparse, subprocess, struct, json
ROOT=Path(__file__).resolve().parent
SYMBOLS={'open':'sdk_open','close':'sdk_close','read':'sdk_read','write':'sdk_write','ioctl':'sdk_ioctl','select':'sdk_select','socket':'sdk_socket','printf':'sdk_printf','sal_usleep':'sdk_usleep','sal_thread_create':'sdk_thread_create','bcm_pkt_alloc':'sdk_pkt_alloc','bcm_pkt_free':'sdk_pkt_free','bcm_tx':'sdk_tx','bcm_rx_active':'sdk_rx_active','bcm_rx_start':'sdk_rx_start','bcm_rx_register':'sdk_rx_register','bcm_port_link_status_get':'sdk_port_link','bcm_port_speed_get':'sdk_port_speed','bcm_port_enable_get':'sdk_port_enabled','bcm_port_enable_set':'sdk_port_enable','bcm_port_untagged_vlan_get':'sdk_port_pvid','bcm_l2_addr_t_init':'sdk_l2_init','bcm_l2_addr_add':'sdk_l2_add','bcm_l2_addr_delete':'sdk_l2_delete','sal_snprintf':'sdk_snprintf','sal_time_usecs':'sdk_time_usecs'}
def main():
    p=argparse.ArgumentParser();p.add_argument('--prefix',required=True);p.add_argument('--sdk',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args();a.output.mkdir(parents=True,exist_ok=True)
    from patch_sdk import ORIGINAL_SHA256
    import hashlib
    if hashlib.sha256(a.sdk.read_bytes()).hexdigest()!=ORIGINAL_SHA256: raise SystemExit('Unsupported factory SDK')
    symbols={}
    for line in subprocess.check_output([a.prefix+'nm',str(a.sdk)],text=True).splitlines():
        fields=line.split()
        if len(fields)==3: symbols[fields[2]]=int(fields[0],16)
    script='PHDRS { plugin PT_LOAD FLAGS(7); }\nSECTIONS { . = 0x11400000; .text : { *(.text.bridge_start) *(.text*) } :plugin\n.rodata : { *(.rodata*) } :plugin\n.data : { *(.data*) *(.sdata*) *(.got*) } :plugin\n.bss : { *(.bss*) *(.sbss*) *(COMMON) } :plugin\n/DISCARD/ : { *(.eh_frame*) *(.comment*) *(.note*) }\n}\n'
    for name,alias in SYMBOLS.items(): script+=f'PROVIDE({alias} = 0x{symbols[name]:08x});\n'
    ld=a.output/'bridge.ld';ld.write_text(script)
    elf=a.output/'bridge.elf'
    subprocess.run([a.prefix+'gcc','-Os','-Wall','-Wextra','-Werror','-std=gnu11','-ffreestanding','-fno-builtin','-fno-stack-protector','-fno-pic','-fno-pie','-msdata=none','-ffunction-sections','-fdata-sections','-nostdlib','-static','-Wl,--build-id=none','-Wl,-e,bridge_start','-Wl,-T,'+str(ld),str(ROOT/'bridge.c'),'-o',str(elf)],check=True)
    data=elf.read_bytes();off=struct.unpack_from('>I',data,28)[0];size,count=struct.unpack_from('>HH',data,42)
    loads=[struct.unpack_from('>8I',data,off+i*size) for i in range(count) if struct.unpack_from('>I',data,off+i*size)[0]==1]
    if len(loads)!=1: raise SystemExit('Expected single load segment')
    _,offset,address,_,filesz,memsz,_,_=loads[0]
    (a.output/'bridge.bin').write_bytes(data[offset:offset+filesz]);(a.output/'bridge.json').write_text(json.dumps({'address':address,'filesz':filesz,'memsz':memsz,'entry':struct.unpack_from('>I',data,24)[0]},indent=2)+'\n')
    print('TAP adapter built',filesz,'bytes, memory',memsz)
if __name__=='__main__': main()
