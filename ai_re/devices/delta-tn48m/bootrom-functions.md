# AP806 BootROM 2.03 — annotated function map

All 191 functions in the ROM decompiled, named and commented in the `ghidra-tn48m` Ghidra project
(`dumps/ap806-bootrom.bin`, sha256 `c79632ea...a787`, identical on the TN48M and the AS5114-48X —
see [bootrom.md](bootrom.md)). Done by 5 parallel agents, one per address range, 2026-11-18. Pure
structural documentation: what each function does, not whether any check in it could be defeated.

Open the Ghidra project (`ghidra-tn48m` MCP, or the GUI on `ghidra/tn48m.gpr`) to read the named
decompiles directly; this file is the index. `rename_variable`/`set_variable_type` errored "variable
not found" throughout this session (looks like an MCP/Ghidra version issue, not something agent-side) —
locals are still `uVarN`/`iVarN` etc; some agents worked around it with `set_function_prototype` for
parameters. Function names and plate comments are complete and saved.

## boot flow and hardware bring-up (0xFFFF0400-0xFFFF13E4)
Reset vector, cache/TLB/MMU setup (identity-mapped page tables, SCTLR_EL3 bit helpers), the UART
console (`console_puts`/`print_hex`/`print_labeled_hex`/`print_boot_banner`), the EL3 exception
handler, address-decode-window helpers, and the boot-status/progress-code mechanism at the
0xF06F00A0/00A4 registers (`set_boot_stage`/`set_boot_substage`, a "sticky first failure" latch —
classic BootROM debug aid). `boot_main_loop` (was `FUN_ffff0c14`) is the top-level flash-scan loop.
A dead function was found: `set_boot_stage_and_substage_unused` has zero callers anywhere in the image.

## header validation, IOROM dispatch, UART boot path (0xFFFF1414-0xFFFF2D14)
`efuse_read_item` (OTP reads at 0xF06F9xxx: KAK digest, box/flash ID, CSK revocation slot search),
`verify_header_magic_and_alignment`, `verify_prolog_checksum`, `process_security_extensions`
(the extension-list walker), `authenticate_header` and `verify_and_decrypt_image` (the KAK→CSK→header
signature chain + optional AES decrypt — named/commented factually, not evaluated for weaknesses).
The four `iorom_*` wrappers (init/read/set-params/select-device) forward into the IOROM blob staged at
0xFFE0C000 by `load_iorom_blob_to_sram`. The UART side: `uart_autobaud_scan`/`uart_detect_sync_pattern`
try candidate pin/baud configs against a fixed sync pattern; `uart_boot_receive_xmodem` is a real
XMODEM receiver (SOH/EOT/CAN, block-number+complement, checksum); `run_bootrom_monitor` is the
`bootrom>> ` interactive monitor (h/r/w/j/t). MPP pin-mux helpers support probing/restoring UART pins
during autobaud.

## DER/RSA-PSS/SHA-256/AES-CBC library (0xFFFF2E60-0xFFFF4DC4)
A complete, independent crypto library: SHA-256 (software + Armv8 crypto-extension compression),
AES-CBC encrypt/decrypt with CTS and explicit-IV variants (Armv8 AES extension), a minimal DER/ASN.1
parser (`parse_der_rsa_pubkey` and friends), and full RSA-PSS verification
(`rsa_pubkey_setup_from_der` → `rsa_bignum_import_and_modexp`/`rsa_modexp` → `emsa_pss_verify` per
RFC 8017, with `mgf1_mask`). `rsa_pss_verify` is the top-level entry other code calls.

## bignum / modular-exponentiation engine (0xFFFF4DDC-0xFFFF6748)
The arithmetic underneath the RSA library: `bignum_mod_exp` (binary square-and-multiply modexp),
Montgomery reduction setup, Knuth algorithm-D long division, extended-Euclid mod-inverse, and a
NEON-accelerated Comba multiply (`bignum_comba_mul_8limb` fast path for 256-bit operands,
`wide_mul256`/`wide_mul256_loop` as the fully-unrolled 256×256→512-bit kernel). A cluster of ~11 tiny
one-to-three-instruction functions near the end are NEON register spill/fill/shuffle stubs that Ghidra
can't usefully model as normal functions (no scalar state crosses the call).

## memcmp/memset/memcpy + the IOROM's SD/eMMC driver (0xFFFF6780-0xFFFFD82C)
Three compiler-generated string/memory primitives (word-aligned, `DC ZVA`-accelerated `memset`,
unrolled `memcpy`), unrelated to boot logic. Then the IOROM blob's actual storage driver: a dual-mode
SDHCI-style controller driver (register block at 0xF06E00xx + vendor PHY registers at 0xF06E0170-0178)
with parallel eMMC and SD card-init/read paths. `emmc_init_and_select_partition` does the full
CMD0/1/2/3/7/6 sequence including EXT_CSD `PARTITION_CONFIG`/`BOOT_BUS_WIDTH` (boot-partition select);
`sd_card_init`/`sd_read_blocks` mirror it for SD cards (ACMD41 OCR/HCS/XPC/S18R, CMD8 voltage check).
`report_phy_init_timeout` has zero cross-references — only reachable via an `UNRECOVERED_JUMPTABLE`
Ghidra flagged inside `print_error_with_hex_value`, not traced further. Register-bit layouts match
standard SDHCI; the vendor PHY register semantics are inferred from behavior, not a datasheet — flagged
as inference in their comments, not fact.

## what this is good for
- Confirms the boot flow and crypto primitives described in [bootrom.md](bootrom.md) end to end, with
  every supporting routine now named instead of `FUN_xxxxx`.
- A clean reference for the chainload work (TN48M, AS5114): confirms the IOROM's SD/eMMC driver and the
  UART boot/monitor path are the ROM's only non-authenticated boot options, and that nothing here needs
  touching for the RAM-chainload approach both devices use.
- Any further ROM analysis should extend this file, not re-derive it.
