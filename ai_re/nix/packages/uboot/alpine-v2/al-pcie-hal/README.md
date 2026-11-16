# Annapurna Labs PCIe HAL 3.4 (reference copy)

The files U-Boot patch 0017 (`drivers/pci/al_pcie/`) builds, verbatim. The
`pcie-init` test checks that the copies inside the patch equal these, and
builds `al_init_pcie_rc.c` against them on the host.

Source: MikroTik's GPL Linux 5.6.3 drop (tikoci/mikrotik-gpl c3e110db,
`drivers/soc/alpine/HAL`; HAL `file_list.mk` says 3.4.0), unpacked in this repo
at `devices/cs8210-32x/analysis/research/mikrotik-added/drivers/soc/alpine/HAL`.

Licence: the Annapurna Labs files are triple-licensed (Annapurna commercial,
GPL-2.0, BSD-2-Clause) and used under GPL-2.0. The Amazon 2017 files
(`al_hal_pbs_regs.h`, `al_hal_pbs_utils.c`, `al_hal_pcie_reg_ptr_set_rev*`,
`al_hal_pcie_regs_fields.h`) say only "All Rights Reserved"; they are here
because MikroTik distributes them as part of that GPL kernel source.

sha256 and path in the drop:

```
a0d499a7127b061b718a776b4ae70ec94c1167a604d09d2e75647080af941f83  include/common/al_hal_common.h
56ffed70e053fff77c6e2bad90123a6066cc0210b194e11eab3a8b9b80cc294c  pbs/include/al_hal_pbs_regs.h
ad050d30ac55b843914af22fb77e485e8357bd33ab06387aa33d568d4d034b2c  pbs/src/al_hal_pbs_utils.c
cc9d6c9faef00cdd754a263d5ed96a1a8c16a46b05a4a9ddde5ab390241e27a0  pbs/include/al_hal_pbs_utils.h
0ba84ade209b20bf6be11dd1886a2026746c735c4e66e10fcf50e69dab2ddd22  pcie/src/al_hal_pcie_axi_reg.h
db0e692c838f3f289d2acc79d47459b29e07cea1d9bdeb960e6b9457f653ae39  pcie/src/al_hal_pcie.c
db17cf6f4ef5f96572b6a62b91f30977e0ce8a560ef3be78933172eef9d41970  pcie/include/al_hal_pcie.h
df0759ea83c7649826d9795d5b2fc5f4d64752d02525ff998b90dff68d617cb8  pcie/src/al_hal_pcie_reg_ptr_set_rev1.h
fb8a898ba893b90cf7fcf9fe8b2d58a2d46d5a8f1146ce7dba821f0315be3716  pcie/src/al_hal_pcie_reg_ptr_set_rev2.c
956a189ef46f3aab4fd57b5efc5de25d57af4a511d20e1e505582324a46a2131  pcie/src/al_hal_pcie_reg_ptr_set_rev2.h
8650a4a6bc130b58025be16713d323018272953751fbd8a864d399aeb379757f  pcie/src/al_hal_pcie_reg_ptr_set_rev3.c
5262ca48969c817e59a1188004b0c10180a1b6fd0bdc6dc75d7108eb45324c85  pcie/src/al_hal_pcie_reg_ptr_set_rev3.h
a581e80f2275169267f6d9ddf0e0639c558822e7194cf1e7583f748cc84c4954  pcie/src/al_hal_pcie_reg_ptr_set_rev4.h
10ae54e6bd68e203aeae32166128af75cde60c6383f7e1e89c9f4810c517902a  pcie/src/al_hal_pcie_reg_ptr_set_rev5.h
c596752ecc84207fdd02b6cfb2218c827e99c33e63c8ceb7cd70757f613c5494  pcie/src/al_hal_pcie_reg_ptr_set_revx.h
8ccd111d7631a2bb5823c03db5044b6ff61dc71778d21b7c5bf5e3cbd6328960  pcie/src/al_hal_pcie_regs_fields.h
aba259d24f58eda129d195a0903a446eb3c6d9f8fa3cd7b13e65fbf63678a7fa  pcie/src/al_hal_pcie_regs.h
c0dd883dfef1eac8a83c78c71b128f12a3ffe69a391e1c097fe425f68b9ef12a  pcie/src/al_hal_pcie_w_reg.h
44d149afb4cb098055bb419796dc3ea4542b94d1704e29ea626f4f1efc44909b  include/common/al_hal_reg_utils.h
2e2eaedb59ac9d2609abde9c421daed968a0afecd9f7fdf9f94ae6e8b261c5c9  include/common/al_hal_types.h
59925c2713dbf4eeb70b235a194e2e8012f231d1be3a9dcd060c2e05942a0a22  include/io_fabric/al_hal_unit_adapter_regs.h
```
