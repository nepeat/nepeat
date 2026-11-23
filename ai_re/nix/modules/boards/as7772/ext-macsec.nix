# AS7772-32X: the external MACsec table for sai-openbcm
# (packages/sai-openbcm/src/ext_macsec.h, sai.profile SAI_OPENBCM_EXT_MACSEC_FILE).
# The MACsec engine of every port is in its BCM82391 "Evora" PHY: an EIP-163
# classifier and an EIP-164 transform engine per direction. This file gives
# sai-openbcm the register sequences that realise the SAI MACsec objects.
# What the registers are, how the sequences were recorded and checked against
# the chip: devices/edge-core-as7772-32x/macsec.md ("Under SONiC").
#
# Nothing here is a vendor file: the sequences are what the bus carried when
# the vendor library (re-hosted, devices/.../evora-rehost/macsec.py) did the
# same operations on the real PHYs, written out by register. evora-rehost/
# seqcheck.py renders this table and compares it with such recordings
# operation by operation.
#
# Layout per PHY and direction (fixed, so every sequence is a constant apart
# from the key material):
#   vPort 0 "data"   rule 1: every frame, priority 0, drop flag = the gate
#   vPort 1 "ctl"    rule 0: EtherType 888e (EAPOL/MKA), priority 7 -> SA 0 = BYPASS
#   SA slot 1 + AN   the SC's SA with that AN (context id = slot)
# Two ways into the PHY, both on clause 45 device 1:
#   LMI      1.900a status, 1.9005 command/data FIFO: one 32-bit engine register
#   message  bytes in 1.a000.. (low byte first), 1.8222 = 1201 (write) / 1211 (read),
#            1.8221 reads 12e3 when done, read data from 1.a101..: register arrays
#            (SA record, TCAM words, counter blocks)
{ lib, cages }:
let
  inherit (lib) concatLists concatMap concatStringsSep genList imap0 toLower toHexString mod optionals range;
  x = lib.fromHexString;
  hex = n: toLower (toHexString n);
  hex2 = n: lib.fixedWidthString 2 "0" (hex n);
  phyIds = concatMap (base: genList (i: base + i) 4) (map x [ "0" "24" "48" "6c" "110" "134" "158" "17c" ]);

  # ---- value tokens: an int, or a table token string ----
  tok = v: if builtins.isInt v then hex v else v;
  # two bytes (ints or "param[i]") -> one 16-bit token, high.low
  word = lo: hi: if builtins.isInt lo && builtins.isInt hi then hex (hi * 256 + lo)
    else "${if builtins.isInt hi then hex2 hi else hi}.${if builtins.isInt lo then hex2 lo else lo}";
  le32 = v: [ (mod v 256) (mod (v / 256) 256) (mod (v / 65536) 256) (v / 16777216) ];
  p = name: i: "${name}[${toString i}]";
  bytes = name: n: genList (p name) n;
  zeros = n: genList (_: 0) n;

  # ---- LMI: engine blocks (cmd0 bits 11:8), lane mask 1 ----
  # egress: classifier a, transform b; ingress: classifier 2, transform 3
  eng = { egress = { cls = 10; sec = 11; id = 1; }; ingress = { cls = 2; sec = 3; id = 0; }; };
  done = "P p 1 900a 1 1 20 100";        # write accepted (bit 0)
  ready = "P p 1 900a 7 7 20 100";       # read data in the FIFO
  # write one 32-bit register; lo/hi = 16-bit tokens
  lmiW' = blk: addr: lo: hi: [ "R p 1 900a" "D 200" "W p 1 9005 ${hex (4096 + blk * 256 + 8)}" "W p 1 9005 ${tok lo}" "W p 1 9005 ${tok hi}"
    "W p 1 9005 ${hex addr}" done "D 200" ];
  lmiW = blk: addr: v: lmiW' blk addr (mod v 65536) (v / 65536);
  # read one; lo/hi = what to do with the two data words ("R ..." / "X ... dest" / "E ... val mask" without the address)
  lmiR = blk: addr: lo: hi: [ "R p 1 900a" "D 200" "W p 1 9005 ${hex (4096 + blk * 256)}" "W p 1 9005 ${hex addr}" ready "D 200" ]
    ++ map (o: if o == null then "R p 1 9005" else "${builtins.head o} p 1 9005 ${builtins.elemAt o 1}") [ lo hi ];
  clear = blk: addr: n: concatMap (i: lmiW blk (addr + 4 * i) 0) (range 0 (n - 1));
  # the engines' "synchronise" request: set bit 31, read it back three times (the third after 10 ms); it must have cleared
  sync = blk: addr: lmiW blk addr (x "80000000") ++ lmiR blk addr null null ++ lmiR blk addr null null ++ [ "D 10000" ]
    ++ lmiR blk addr null [ "E" "0 8000" ];

  # ---- firmware messages ----
  msgWords = bs: genList (i: word (builtins.elemAt bs (2 * i)) (builtins.elemAt bs (2 * i + 1))) (builtins.length bs / 2);
  msgAck = "P p 1 8221 12e3 ffff 50 200";
  msgSend = cmd: bs: imap0 (i: w: "W p 1 ${hex (40960 + i)} ${w}") (msgWords bs) ++ [ "W p 1 8222 ${cmd}" "D 1000" msgAck ];
  addr3 = a: [ (mod a 256) (mod (a / 256) 256) (a / 65536) ];
  # write n 32-bit words at addr of engine block kind (0 = classifier, 1 = transform): the last byte of an odd message is not sent
  msgWrite = dir: kind: addr: data: let n = builtins.length data / 4; in
    msgSend "1201" (lib.init ([ (4 * n + 7) 0 eng.${dir}.id kind 0 n ] ++ addr3 addr ++ data));
  # read n 32-bit words; dests = list of 2n items (null = ignore, [ "X" "name[i]" ], [ "E" "val mask" ])
  msgRead = dir: kind: addr: n: dests: msgSend "1211" ([ (4 * n + 6) 0 eng.${dir}.id kind n ] ++ addr3 addr)
    ++ imap0 (i: d: if d == null then "R p 1 ${hex (41217 + i)}" else "${builtins.head d} p 1 ${hex (41217 + i)} ${builtins.elemAt d 1}") dests;
  # a block of 64-bit counters: names in hardware order (null = not reported)
  counters = names: concatMap (n: genList (i: if n == null then null else [ "X" (p n i) ]) 4) names;

  # ---- classifier rules: rule r = control word, 4 TCAM key words, mask control, 4 mask words, policy ----
  ruleKey = et: [ 0 0 0 9  1 1 0 0 ] ++ (if et then [ 136 142 0 0 ] else zeros 4) ++ zeros 4;
  ruleMask = et: zeros 8 ++ (if et then [ 255 255 0 0 ] else zeros 4) ++ zeros 4;
  # maskCtl: 0x00030301 when a rule is added, 0x00000301 when it is updated (what the library writes)
  ruleWrite = dir: r: et: maskCtl: policy: let c = eng.${dir}.cls; in
    lmiW c (r * 64) 1 ++ msgWrite dir 0 (r * 64 + 4) (ruleKey et) ++ lmiW c (r * 64 + 32) maskCtl
    ++ msgWrite dir 0 (r * 64 + 36) (ruleMask et) ++ lmiW c (x "8800" + 4 * r) policy;
  ruleAdd = dir: r: et: policy: ruleWrite dir r et (x "30301") policy ++ sync eng.${dir}.cls (x "8104") ++ lmiW eng.${dir}.cls (x "8080") (if r == 0 then 1 else 2);
  ruleDel = dir: r: sync eng.${dir}.cls (x "8104") ++ lmiW eng.${dir}.cls (x "80c0") (if r == 0 then 1 else 2) ++ lmiW eng.${dir}.cls (x "8800" + 4 * r) 0;
  dataDrop = x "40000000";               # rule 1 policy: vPort 0, drop
  ctlPolicy = x "b8000001";              # rule 0 policy: vPort 1, priority 7, control frame

  # per-vPort counter blocks of the transform engine (cleared when the vPort is made)
  secyWords = { egress = 6; ingress = 16; };
  ifcWords = { egress = 14; ingress = 12; };
  saWords = { egress = 8; ingress = 20; };
  vport = dir: v: let e = eng.${dir}; in
    lmiW e.cls (x "9000" + 4 * v) (if dir == "egress" then 12 else x "c00")
    ++ clear e.sec (x "6000" + 128 * v) secyWords.${dir} ++ clear e.sec (x "c000" + 64 * v) ifcWords.${dir} ++ clear e.sec (x "d000" + 64 * v) 12;

  enable = dir: let e = eng.${dir}; in
    lmiR e.sec (x "fe00") null null ++ lmiW e.sec (x "fe00") 0              # engine out of bypass
    ++ vport dir 0 ++ lmiW e.cls (x "fe00") 0 ++ lmiW e.cls (x "a00c") 2    # data vPort; exceptions: internal drop
    ++ vport dir 1                                                           # control vPort
    ++ lmiW e.sec (x "4a08") 0 ++ lmiW e.sec (x "4808") (x "8000") ++ lmiW e.sec (x "4a08") (x "80")   # its flow: SA 0, BYPASS
    ++ ruleAdd dir 0 true ctlPolicy
    ++ ruleAdd dir 1 false dataDrop;
  # what a previous owner of the PHY may have left: the SC's SA table and flow, the four SA records (keys)
  reset = dir: let e = eng.${dir}; in
    lmiW e.sec (x "4800") 0 ++ optionals (dir == "ingress") (lmiW e.sec (x "4804") 0 ++ lmiW e.sec (x "47fc") (x "40000000")) ++ lmiW e.sec (x "4a00") 0
    ++ concatMap (an: msgWrite dir 1 (128 * slot an) (zeros 96)) ans;
  probe = dir: lmiR eng.${dir}.sec (x "fe00") [ "X" "bypass[0]" ] null;
  disable = dir: let e = eng.${dir}; in
    ruleDel dir 1 ++ ruleDel dir 0 ++ lmiW e.cls (x "9004") 0 ++ lmiW e.cls (x "9000") 0
    # the classifier back to its low-latency bypass, no exception drop: with these left as "enable" set them a
    # frame is dropped for want of a rule even though the transform engine is in bypass (found the hard way)
    ++ lmiW e.cls (x "fe00") 1 ++ lmiW e.cls (x "a00c") 0
    ++ lmiW e.sec (x "fe00") 1;
  gate = dir: open: ruleWrite dir 1 false (x "301") (if open then 0 else dataDrop);

  # ---- SAs ----
  ciphers = {
    "gcm-aes-128" = { klen = 16; xpn = false; };
    "gcm-aes-256" = { klen = 32; xpn = false; };
    "gcm-aes-xpn-128" = { klen = 16; xpn = true; };
    "gcm-aes-xpn-256" = { klen = 32; xpn = true; };
  };
  slot = an: an + 1;
  # the transform record's control word (4 bytes, low first)
  control = dir: an: c: if dir == "egress"
    then [ (x "66") (x "e0") (if c.klen == 32 then x "4f" else x "4b") ((if c.xpn then x "a2" else x "92") + 4 * an) ]
    else [ (if c.xpn then x "ef" else x "6f") (if c.xpn then x "a0" else x "e0") (if c.klen == 32 then x "4f" else x "4b") (if c.xpn then x "e2" else x "d2") ];
  # 24 words: control, context id, key (8), hash key (4), PN low / high, window, XPN nonce (3), SCI (2), 0 (2)
  record = dir: an: c:
    control dir an c ++ le32 (slot an)
    ++ bytes "key" c.klen ++ zeros (32 - c.klen) ++ bytes "hkey" 16
    ++ genList (p "pn") 4 ++ (if c.xpn then genList (i: p "pn" (i + 4)) 4 else zeros 4)
    ++ (if dir == "ingress" then bytes "window" 4 else zeros 4)
    ++ (if c.xpn then bytes "iv" 12 else zeros 12)
    ++ (if dir == "egress" || !c.xpn then bytes "sci" 8 else zeros 8)
    ++ zeros 8;
  inuse = an: x "8000" + slot an;
  # The ingress engine's SecTAG checks (channel 0, 0x5000: bits 0-6 = EtherType, V, KaY, C/E, SC, SL, PN; high half =
  # the EtherType 88e5). The chip's reset value 0xe588007f has them all on and the vendor flow never writes it. Bit 6
  # refuses a frame whose 32-bit PN field is 0 (in_pkts_bad_tag): right for a 32-bit packet number, wrong with XPN,
  # where the field is the low half and is 0 once every 2^32 frames (one frame lost every 4 min at 100G; found under
  # load, devices/edge-core-as7772-32x/macsec-loss.md). Not from a recording: our own write, after the SC's first SA.
  sectagChecks = c: lmiW eng.ingress.sec (x "5000") (x "e5880000" + (if c.xpn then x "3f" else x "7f"));
  saAdd = dir: an: c: first: replay: let e = eng.${dir}; s = slot an; cw = control dir an c;
      camReg = x "4800" + 4 * (an / 2); even = mod an 2 == 0; in
    optionals first (lmiW e.sec (x "4a00") 0)
    ++ msgWrite dir 1 (128 * s) (record dir an c)
    ++ (if dir == "egress" then lmiW e.sec (x "4800") (inuse an)            # the vPort transmits with this SA from here on
        else optionals first (lmiW' e.sec (x "4000") (word (p "sci" 0) (p "sci" 1)) (word (p "sci" 2) (p "sci" 3))
               ++ lmiW' e.sec (x "4004") (word (p "sci" 4) (p "sci" 5)) (word (p "sci" 6) (p "sci" 7))
               ++ lmiW e.sec (x "4008") 0 ++ lmiW e.sec (x "47fc") (x "4000"))
             # the SC's SA table: ANs 0/1 in 0x4800, 2/3 in 0x4804, 16 bits each (in use + slot); keep the other half
             ++ lmiR e.sec camReg [ "X" "$0" ] [ "X" "$1" ]
             ++ lmiW' e.sec camReg (if even then inuse an else "$0") (if even then "$1" else inuse an))
    # read the control word back: the record is there
    ++ msgRead dir 1 (128 * s) 1 [ [ "E" "${word (builtins.elemAt cw 0) (builtins.elemAt cw 1)} ffff" ] [ "E" "${word (builtins.elemAt cw 2) (builtins.elemAt cw 3)} ffff" ] ]
    ++ lmiW e.sec (128 * s + 92) (x "80004000" + (if dir == "ingress" then an * 536870912 + s else 0))
    ++ optionals first (lmiW e.sec (x "4a00") (if dir == "egress" then x "270083" else x "12008a" + (if replay then 65536 else 0))
                        ++ lmiW e.sec (x "4a04") (x "c00"))
    ++ clear e.sec (x "8000" + 128 * s) saWords.${dir}
    ++ optionals (first && dir == "ingress") (sectagChecks c);
  saDel = dir: an: last: let e = eng.${dir}; camReg = x "4800" + 4 * (an / 2); even = mod an 2 == 0; in
    if dir == "egress" then optionals last (lmiW e.sec (x "4800") (slot an) ++ lmiW e.sec (x "4a00") 0)
    else lmiR e.sec camReg [ "X" "$0" ] [ "X" "$1" ] ++ lmiW' e.sec camReg (if even then "$0&7fff" else "$0") (if even then "$1" else "$1&7fff")
      ++ optionals last (lmiW e.sec (x "4800") 0 ++ lmiW e.sec (x "4804") 0 ++ lmiW e.sec (x "47fc") (x "40000000") ++ lmiW e.sec (x "4a00") 0);

  # ---- counters (64 bit, clear on read; names = the SAI statistics they feed, x_* = not in SAI) ----
  saStats = dir: an: sync eng.${dir}.sec (x "5104") ++ msgRead dir 1 (x "8000" + 128 * slot an) saWords.${dir} (counters (
    if dir == "egress" then [ "octets_encrypted" "out_pkts_encrypted" "x_out_pkts_too_long" "x_out_pkts_sa_not_inuse" ]
    else [ "octets_encrypted" "octets_protected" "in_pkts_unchecked" "in_pkts_delayed" "in_pkts_late" "in_pkts_ok" "in_pkts_invalid"
           "in_pkts_not_valid" "in_pkts_not_using_sa" "in_pkts_unused_sa" ]));
  flowStats = dir: let s = eng.${dir}.sec; cast = k: [ "ucast_pkts_${k}" "multicast_pkts_${k}" "broadcast_pkts_${k}" "x_drop_pkts_${k}" "x_err_pkts_${k}" ]; in
    sync s (x "5104") ++ msgRead dir 1 (x "6000") secyWords.${dir} (counters (
      if dir == "egress" then [ "other_err" "control_pkts" "pkts_untagged" ]
      else [ "other_err" "control_pkts" "pkts_untagged" "in_pkts_no_tag" "in_pkts_bad_tag" "in_pkts_no_sci" "in_pkts_unknown_sci" "in_tagged_control_pkts" ]))
    ++ sync s (x "5104")
    ++ msgRead dir 1 (x "c000") ifcWords.${dir} (counters ([ "octets_uncontrolled" ] ++ optionals (dir == "egress") [ "octets_controlled" ] ++ cast "uncontrolled"))
    ++ msgRead dir 1 (x "d000") 12 (counters ([ (if dir == "egress" then "out_octets_common" else "octets_controlled") ] ++ cast "controlled"));
  pnRead = dir: an: msgRead dir 1 (128 * slot an + 56) 2 (genList (i: [ "X" (p "pn" i) ]) 4);

  seq = name: ops: [ "seq ${name}" ] ++ ops;
  dirs = [ "egress" "ingress" ];
  ans = range 0 3;
in
assert builtins.length cages == builtins.length phyIds;
concatStringsSep "\n" ([
  "ext-macsec 1"
  "# generated by nix/modules/boards/as7772/ext-macsec.nix"
  "counters clear-on-read"
  # ASIC MAC: 48 bytes average inter-frame gap while MACsec is on (the PHY adds 32 bytes per frame)
  "ifg-bits 384"
] ++ imap0 (i: cageLanes: "port ${toString (builtins.head cageLanes)} ${hex (builtins.elemAt phyIds i)}") cages
  ++ concatMap (dir:
    seq "enable.${dir}" (enable dir) ++ seq "disable.${dir}" (disable dir)
    ++ seq "reset.${dir}" (reset dir) ++ seq "probe.${dir}" (probe dir)
    ++ seq "gate-open.${dir}" (gate dir true) ++ seq "gate-close.${dir}" (gate dir false)
    ++ concatMap (an:
      concatLists (lib.mapAttrsToList (cn: c:
        seq "sa-add.${dir}.${toString an}.${cn}" (saAdd dir an c false false)
        ++ seq "sa-add-first.${dir}.${toString an}.${cn}" (saAdd dir an c true false)
        ++ optionals (dir == "ingress") (seq "sa-add-first.${dir}.${toString an}.${cn}.replay" (saAdd dir an c true true))) ciphers)
      ++ seq "sa-del.${dir}.${toString an}" (saDel dir an false) ++ seq "sa-del-last.${dir}.${toString an}" (saDel dir an true)
      ++ seq "pn.${dir}.${toString an}" (pnRead dir an) ++ seq "stats-sa.${dir}.${toString an}" (saStats dir an)) ans
    ++ seq "stats-flow.${dir}" (flowStats dir)) dirs) + "\n"
