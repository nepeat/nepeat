#!/usr/bin/env python3
"""Stage this owned board's recovered SDK; never fetch a replacement binary."""
import hashlib
import json
import re
import os
import sys
import subprocess
import importlib.util
from pathlib import Path

SDK_SHA256 = "ea4fdc1f4e172cb641e64abd6cd239baaaaa762f2eab1a67ce34992de75d96a8"


def prepare_sdk(source: Path, device: Path):
    archive = (device / "firmware/linux-rootfs.bin").read_bytes()
    offset = 0
    executable = None
    while archive[offset:offset + 6] == b"070701":
        fields = [int(archive[offset + 6 + n * 8:offset + 14 + n * 8], 16)
                  for n in range(13)]
        size, namesize = fields[6], fields[11]
        name = archive[offset + 110:offset + 110 + namesize - 1].decode()
        start = (offset + 110 + namesize + 3) & ~3
        if name == "/broadcom/bcm.user":
            executable = archive[start:start + size]
            break
        offset = (start + size + 3) & ~3
    if executable is None or hashlib.sha256(executable).hexdigest() != SDK_SHA256:
        raise SystemExit("Recovered SDK binary missing or checksum mismatch")
    replacements = executable.count(b"/dev/mem\0")
    if not replacements:
        raise SystemExit("Expected SDK memory-device references are absent")
    patched = executable.replace(b"/dev/mem\0", b"/dev/bcm\0")
    tap_enabled = os.environ.get("OPENWRT_SDK_TAP", "1") != "0"
    adapter = None
    if tap_enabled:
        work = source.parent
        original = work / "factory-sdk/bcm.user"
        original.parent.mkdir(exist_ok=True)
        original.write_bytes(executable)
        compilers = list(work.glob("openwrt-toolchain-*/toolchain-*/bin/powerpc-openwrt-linux-musl-gcc"))
        if len(compilers) != 1:
            raise SystemExit("Expected one SDK adapter cross compiler")
        helpers = Path(__file__).resolve().parent / "sdk-netdev"
        output = work / "sdk-netdev"
        subprocess.run([sys.executable, str(helpers / "build.py"), "--prefix", str(compilers[0])[:-3],
                        "--sdk", str(original), "--output", str(output)], check=True)
        spec = importlib.util.spec_from_file_location("sdk_tap_patch", helpers / "patch_sdk.py")
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        patched = module.patch_sdk(executable, output)
        adapter = json.loads((output / "bridge.json").read_text())
        adapter["sha256"] = hashlib.sha256((output / "bridge.bin").read_bytes()).hexdigest()
    destination = source / "files/usr/lib/bcm-sdk"
    destination.mkdir(parents=True, exist_ok=True)
    (destination / "bcm.user").write_bytes(patched)
    (destination / "bcm.user").chmod(0o755)
    config = (device / "firmware/config.bcm").read_text()
    # Start with software IRQ polling while validating the compatibility ABI.
    # No persistent firmware writes are involved.
    config_10g = config + "\npolled_irq_mode=1\n"
    mappings = sorted((int(logical), int(physical)) for logical, physical in
                      re.findall(r"^portmap_(\d+)=(\d+):10", config, re.M))
    groups = [physical for _, physical in mappings if (physical - 1) % 4 == 0]
    if len(mappings) != 64 or len(groups) != 16:
        raise SystemExit("Unexpected board lane map; review the 40G profile")
    config_40g = re.sub(r"^portmap_\d+=.*\n", "", config_10g, flags=re.M)
    config_40g = re.sub(r"^pbmp_xport_xe=.*$", "pbmp_xport_xe=0x1fffe", config_40g, flags=re.M)
    config_40g += "".join(f"portmap_{index + 1}={physical}:40\n"
                           for index, physical in enumerate(groups))
    (destination / "config-10g.bcm").write_text(config_10g)
    (destination / "config-40g.bcm").write_text(config_40g)
    (destination / "config.bcm").write_text(config_40g)
    startup = (device / "firmware/rc.soc").read_text()
    if startup.count("init bcm;") != 1:
        raise SystemExit("Unexpected vendor startup script; review port-disable placement")
    startup = startup.replace("init bcm;", "init bcm; port xe enable=false;", 1)
    startup += "\nport xe enable=false\n"
    if tap_enabled:
        (destination / "start-tap.cint").write_text("/* Experimental SDK TAP bootstrap, not a working KNET ABI. */\nprint bcm_knet_init(0);\nprint bcm_switch_control_set(0, bcmSwitchArpRequestToCpu, 1);\nprint bcm_switch_control_set(0, bcmSwitchArpReplyToCpu, 1);\nprint bcm_switch_control_set(0, bcmSwitchNdPktToCpu, 1);\nprint bcm_switch_control_set(0, bcmSwitchDhcpPktToCpu, 1);\n")
        startup += "cint /usr/lib/bcm-sdk/start-tap.cint\n"
    (destination / "rc.soc").write_text(startup)
    provenance = {"source": "owned board's recovered Linux initramfs",
                  "original_sha256": SDK_SHA256,
                  "patched_sha256": hashlib.sha256(patched).hexdigest(),
                  "patch": "/dev/mem -> /dev/bcm (equal-length pathname substitution)",
                  "replacement_count": replacements,
                  "profiles": {"40g": {"ports": 16, "group_physical_bases": groups},
                               "10g": {"ports": 64, "source": "factory lane mapping"}},
                  "kernel_interface": "bcm-bde-compat; ASIC BAR and dedicated DMA pool only"}
    provenance["packet_interface"] = "SDK packet API/TAP; factory KNET APIs are disabled" if tap_enabled else "none"
    if tap_enabled:
        provenance["adapter"] = adapter
        provenance["patch"] += "; appended owned GPL TAP adapter; disabled KNET init stub used only as explicit bootstrap"
    (destination / "provenance.json").write_text(json.dumps(provenance, indent=2) + "\n")


if __name__ == "__main__":
    root = Path(__file__).resolve().parent
    prepare_sdk(root / "work/source", root.parent)
