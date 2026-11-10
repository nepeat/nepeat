#!/usr/bin/env python3
"""Testbench for the simulation netlist (simnet.py): PCIe TLPs in, I2C bus out.

    simtb.py TOP.v CHANNEL.json OUT_tb.sv [LOGIC.json TOP.map.json PROBES.txt]
             (CHANNEL.json = a layer-6 chNN.json; PROBES.txt = label=templateElement ...)
    then:  verilator --binary --timing -j 0 -Wno-fatal --top-module tb TOP.v OUT_tb.sv
           obj_dir/Vtb +ch=17 +sel=0 +rd=1 +addr=50 +len=4 +off=10 ...

What the testbench drives (assumptions, all I unless stated):
  - PCIE_2_1 TRN RX, 64-bit, active-high: TLP DW0 in TRNRD[63:32] of the
    first beat (Xilinx 7-series TRN convention); TRNRSOF/TRNREOF/
    TRNRSRCRDY; a beat is taken on a rising clock edge with TRNRDSTRDY high;
    TRNRBARHIT0 = BAR0.  Payload dwords big-endian within the dword (host
    byte 0 in TRNRD[31:24]) -- confirmed in layer 7 by the profile register.
  - TX side: TRNTDSTRDY = 1; completions collected from TRNTD when
    TRNTSRCRDY.  Link up, USERRSTN released after reset, LTSSM = L0.
  - All other pads high (pulled up, modules absent / not interrupting).
  - The channel's SDA/SCL are open drain with a pull-up; an I2C device model
    (7-bit address `+dev`, default 0x50) ACKs its address and every written
    byte, serves mem[i] = i ^ 0xA5 on reads, never stretches SCL.
Everything is logged with the clock cycle: TLPs, completions, bus events
(START, STOP, bytes with the ACK bit), SCL rising edges.
"""
import json
import re
import sys


def ports(top):
    ins, outs = [], []
    head = open(top).read().split(');', 1)[0]
    for m in re.finditer(r'\b(input|output)\s+(\w+)', head):
        if m.group(2) != 'clk':
            (ins if m.group(1) == 'input' else outs).append(m.group(2))
    return ins, outs


def probes(chj, logic_path, mapj, names):
    """[(label, verilog id)] for template elements of the instance (Q / O nets)"""
    I = json.load(open(chj))
    logic = json.load(open(logic_path))
    ids = json.load(open(mapj))
    P = {p['name']: p for p in logic['prims']}
    out = []
    for lab, eid in names:
        if eid.startswith('net:'):
            if eid[4:] in ids:
                out.append((lab, ids[eid[4:]]))
            continue
        if eid.startswith('port:'):
            ns = I['bind'].get(eid[5:]) or []
            if ns and ns[0] in ids:
                out.append((lab, ids[ns[0]]))
            continue
        node = I['placement'].get(eid)
        if node is None:
            continue
        prim, _, o = node.partition('#')
        p = P[prim]
        net = p['conns'].get(o or 'Q')
        if net in ids:
            out.append((lab, ids[net]))
    return out


def main():
    top, chj, out = sys.argv[1:4]
    probe = []
    if len(sys.argv) > 6:
        names = [tuple(x.split('=')) for x in open(sys.argv[6]).read().split()]
        probe = probes(chj, sys.argv[4], sys.argv[5], names)
    ins, outs = ports(top)
    I = json.load(open(chj))
    tpl = json.load(open(chj.rsplit('/', 1)[0] + '/template.json'))
    # pad roles: template element whose T comes from the SCL release FF; layer 7
    # found pad 1 (second element) = SCL, pad 0 = SDA in both families
    # pad roles (layer 5/7, V): the SDA pad's open-drain FF has a 3-input OR as
    # next state, the SCL pad's a 4-input function; found per template
    T = tpl['families'][I['family']]
    E = T['elements']
    pads = [e for e in T['order'] if E[e]['kind'] == 'IOBUF'][:2]

    def dsize(pad):
        t = E[pad]['ports'].get('T', '')
        ff = E.get(t.split('.')[0], {})
        d = ff.get('ports', {}).get('D', '')
        lut = E.get(d.split('.')[0], {})
        return lut.get('k', 0), lut.get('table')
    k0, k1 = dsize(pads[0]), dsize(pads[1])
    roles = pads if (k0[0], k1[0]) == (3, 4) else pads[::-1]
    if sorted((k0[0], k1[0])) != [3, 4]:
        raise SystemExit('cannot tell SDA from SCL: %s %s' % (k0, k1))
    sda, scl = (I['placement'][e] for e in roles)
    defaults = {'pcie_TRNLNKUP': 1, 'pcie_TRNTDSTRDY': 1, 'pcie_TRNTDSTRDY0': 1, 'pcie_PLLINKGEN2CAP': 1}
    L = ['`timescale 1ns/1ps', 'module tb;', '  reg clk = 0; always #8 clk = ~clk;   // 62.5 MHz',
         '  integer cyc = 0; always @(posedge clk) cyc <= cyc + 1;']
    multi = {}
    for p in ins:
        m = re.match(r'(pcie_[A-Z]+?)(\d+)$', p)
        if m and m.group(1) in ('pcie_TRNRD', 'pcie_TRNRBARHIT', 'pcie_TRNTBUFAV', 'pcie_PLLTSSMSTATE',
                                'pcie_PLSELLNKWIDTH', 'pcie_CFGPCIELINKSTATE'):
            multi.setdefault(m.group(1), set()).add(int(m.group(2)))
    for b, idx in multi.items():
        L.append('  reg [%d:0] %s = 0;' % (max(idx), b))
    L.append("  initial begin pcie_TRNTBUFAV = 6'h3f; pcie_PLLTSSMSTATE = 6'h16; end")
    for p in ins:
        if re.match(r'(pcie_[A-Z]+?)(\d+)$', p) and re.match(r'(pcie_[A-Z]+?)(\d+)$', p).group(1) in multi:
            continue
        if p.startswith('pin_'):
            continue
        L.append('  reg %s = %d;' % (p, defaults.get(p, 0)))
    L.append('  reg dev_sda_low = 0;')
    ibufs = []
    for p in ins:
        if p.startswith('pin_'):
            site = p[4:]
            if site == sda:
                L.append('  wire %s = t_%s & ~dev_sda_low;' % (p, site))
            elif site == scl:
                L.append('  wire %s = t_%s;' % (p, site))
            elif 't_' + site in outs:
                L.append('  wire %s = t_%s;' % (p, site))
            else:
                L.append('  reg %s = 1;' % p)
                ibufs.append(site)
    for p in outs:
        L.append('  wire %s;' % p)
    conn = ['.clk(clk)']
    for p in ins + outs:
        m = re.match(r'(pcie_[A-Z]+?)(\d+)$', p)
        if m and m.group(1) in multi:
            conn.append('.%s(%s[%s])' % (p, m.group(1), m.group(2)))
        else:
            conn.append('.%s(%s)' % (p, p))
    L.append('  top dut(%s);' % ', '.join(conn))
    L.append('  wire sda = pin_%s, scl = pin_%s;' % (sda, scl))
    L.append('  wire [63:0] txd = {%s};' % ', '.join('pcie_TRNTD%d' % i for i in range(63, -1, -1)))
    L.append('  task set_in(input integer k, input v); begin case (k)')
    for k, site in enumerate(ibufs):
        L.append('    %d: pin_%s = v;' % (k, site))
    L.append('  endcase end endtask')
    L.append('  localparam N_IN = %d;' % len(ibufs))
    L.append('  task show_ins; begin')
    for k, site in enumerate(ibufs):
        L.append('    $display("in %d %s", %d);' % (k, site, k))
    L.append('  end endtask')
    obufs = [p[2:] for p in outs if p.startswith('o_')]
    L.append('  wire [%d:0] ovec = {%s};' % (len(obufs) - 1, ', '.join('o_%s' % x for x in reversed(obufs))))
    L.append('  task show_outs; begin')
    for k, site in enumerate(obufs):
        L.append('    $display("out %d %s", %d);' % (k, site, k))
    L.append('  end endtask')
    L.append(TB_BODY)
    L.append('  task show_probes; begin')
    for lab, i in probe:
        L.append('    $display("[%%0d] probe %-14s %%0d", cyc, dut.%s);' % (lab, i))
    L.append('  end endtask')
    L.append('  always @(posedge clk) if (trace && cyc >= trace_from && cyc < trace_to) show_probes();')
    if probe:
        L.append('  wire [%d:0] pvec = {%s};' % (len(probe) - 1, ', '.join('dut.%s' % i for lab, i in reversed(probe))))
        L.append('  reg [%d:0] pvec_q = 0; integer vlog = 0; initial void\'($value$plusargs("vlog=%%d", vlog));' % (len(probe) - 1))
        L.append('  always @(posedge clk) begin if (vlog && pvec !== pvec_q) $display("[%0d] vec %b", cyc, pvec); pvec_q <= pvec; end')
        open(out + '.probes', 'w').write('\n'.join(lab for lab, i in probe) + '\n')
    L.append('endmodule')
    open(out, 'w').write('\n'.join(L) + '\n')
    print('SDA %s SCL %s family %s channel %s' % (sda, scl, I['family'], I['channel']))


TB_BODY = r'''
  // ------------------------------------------------------------ helpers
  function [31:0] bswap(input [31:0] v); bswap = {v[7:0], v[15:8], v[23:16], v[31:24]}; endfunction
  reg [31:0] base = 32'hf000_0000;
  task beat(input [63:0] d, input sof, input eof);
    begin
      @(negedge clk);
      pcie_TRNRD = d; pcie_TRNRSOF = sof; pcie_TRNREOF = eof; pcie_TRNRSRCRDY = 1; pcie_TRNRBARHIT = 1;
      @(posedge clk); begin : w integer t; t = 0;
        while (!pcie_TRNRDSTRDY && t < 5000) begin @(posedge clk); t = t + 1; end
        if (t >= 5000) begin $display("[%0d] TRNRDSTRDY stuck low", cyc); $fflush; stuck = 1; if (!sideband) $finish; end end
      @(negedge clk); pcie_TRNRSRCRDY = 0; pcie_TRNRSOF = 0; pcie_TRNREOF = 0;
    end
  endtask
  integer tag = 0; reg stuck = 0; integer trace = 0, trace_from = 0, trace_to = 400;
  initial begin void'($value$plusargs("trace=%d", trace)); void'($value$plusargs("from=%d", trace_from));
    void'($value$plusargs("to=%d", trace_to)); end
  task mwr(input [19:0] off, input [31:0] v);
    begin
      $display("[%0d] MWr  0x%05x <= 0x%08x", cyc, off, v); $fflush;
      beat({32'h4000_0001, 32'h0000_000f}, 1, 0);
      beat({base | off, bswap(v)}, 0, 1);
      repeat (4) @(posedge clk);
    end
  endtask
  reg [31:0] cpl_data; reg cpl_got;
  reg [63:0] cpl_b0 = 0, cpl_b1 = 0; integer cpl_n = 0, ntx = 0;
  always @(posedge clk) if (pcie_TRNTSRCRDY) begin
    if (pcie_TRNTSOF) cpl_n = 0;
    if (cpl_n == 0) cpl_b0 = txd; else if (cpl_n == 1) cpl_b1 = txd;
    cpl_n = cpl_n + 1;
    if (pcie_TRNTEOF) begin
      ntx = ntx + 1;
      if (ntx < 40) begin $display("[%0d] TX TLP %016x %016x (%0d beats)", cyc, cpl_b0, cpl_b1, cpl_n); $fflush; end
      if (cpl_b0[62:56] == 7'b1001010) begin cpl_data = bswap(cpl_b1[31:0]); cpl_got = 1; end
    end
  end
  task mrd(input [19:0] off, output [31:0] v);
    integer t;
    begin
      cpl_got = 0;
      beat({32'h0000_0001, 16'h0000, tag[7:0], 8'h0f}, 1, 0);
      beat({base | off, 32'h0}, 0, 1);
      tag = tag + 1;
      t = 0; while (!cpl_got && t < 2000) begin @(posedge clk); t = t + 1; end
      v = cpl_got ? cpl_data : 32'hdead_dead;
      if (!cpl_got) $display("[%0d] MRd  0x%05x: no completion", cyc, off);
    end
  endtask
  // ------------------------------------------------------------ I2C device model
  integer dev = 'h50;
  reg [7:0] mem [0:255]; integer k; initial for (k = 0; k < 256; k = k + 1) mem[k] = k ^ 8'ha5;
  reg psda = 1, pscl = 1; integer nbit = 0; reg [7:0] sh = 0; integer state = 0;  // 0 idle 1 addr 2 wr 3 rd
  reg [7:0] ptr = 0; reg first = 0; reg ack_phase = 0; reg mack = 0; integer nscl = 0; integer last_rise = 0;
  integer per_sum = 0, per_n = 0, per_min = 1 << 30, per_max = 0;
  always @(posedge clk) begin
    if (scl && !pscl) begin  // SCL rising: sample
      if (last_rise > 0 && nbit != 0) begin
        per_sum = per_sum + (cyc - last_rise); per_n = per_n + 1;
        if (cyc - last_rise < per_min) per_min = cyc - last_rise;
        if (cyc - last_rise > per_max) per_max = cyc - last_rise;
      end
      last_rise = cyc; nscl = nscl + 1;
      if (state != 0) begin
        if (nbit < 8) begin if (state != 3) sh = {sh[6:0], sda}; nbit = nbit + 1; end
        else begin mack = sda; end
      end
    end
    if (!scl && pscl && state != 0) begin  // SCL falling: drive
      if (nbit == 8 && !ack_phase) begin
        ack_phase = 1;
        if (state == 1) begin
          $display("[%0d] bus: address 0x%02x %s -> %s", cyc, sh[7:1], sh[0] ? "R" : "W", (sh[7:1] == dev) ? "ACK" : "NACK");
          if (sh[7:1] == dev) begin dev_sda_low = 1; state = sh[0] ? 3 : 2; first = 1; end else state = 0;
        end else if (state == 2) begin
          $display("[%0d] bus: %s byte 0x%02x -> ACK", cyc, first ? "offset" : "data", sh);
          if (first) begin ptr = sh; first = 0; end else begin mem[ptr] = sh; ptr = ptr + 1; end
          dev_sda_low = 1;
        end else if (state == 3) begin
          dev_sda_low = 0;   // release for the master's ACK
        end
      end else if (ack_phase) begin
        ack_phase = 0; nbit = 0;
        if (state == 3) begin
          if (!first) begin
            $display("[%0d] bus: read byte 0x%02x, master %s", cyc, mem[ptr - 1], mack ? "NACK" : "ACK");
            if (mack) state = 0;
          end
          first = 0;
          if (state == 3) begin sh = mem[ptr]; ptr = ptr + 1; dev_sda_low = ~sh[7]; end else dev_sda_low = 0;
        end else dev_sda_low = 0;
      end else if (state == 3 && nbit > 0 && nbit < 8) begin
        dev_sda_low = ~sh[7 - nbit];
      end
    end
    if (scl && pscl && psda && !sda) begin $display("[%0d] bus: START", cyc); state = 1; nbit = 0; ack_phase = 0; dev_sda_low = 0; end
    if (scl && pscl && !psda && sda) begin $display("[%0d] bus: STOP", cyc); state = 0; nbit = 0; dev_sda_low = 0; end
    psda = sda; pscl = scl;
  end
  // ------------------------------------------------------------ passive bus monitor (independent of the model)
  reg mpsda = 1, mpscl = 1; integer mbits = 0; reg [8:0] mframe = 0;
  always @(posedge clk) begin
    if (scl && !mpscl) begin mframe = {mframe[7:0], sda}; mbits = mbits + 1;
      if (mbits == 9) begin $display("[%0d] wire: 0x%02x %s", cyc, mframe[8:1], mframe[0] ? "NACK" : "ACK"); mbits = 0; end end
    if (scl && mpscl && mpsda && !sda) begin $display("[%0d] wire: START", cyc); mbits = 0; end
    if (scl && mpscl && !mpsda && sda) begin $display("[%0d] wire: STOP", cyc); mbits = 0; end
    mpsda = sda; mpscl = scl;
  end
  // ------------------------------------------------------------ scenario
  integer ch = 17, sel = 0, rd = 1, addr = 'h50, len = 4, off = 'h10, i, polls; reg [31:0] v, cb, st;
  integer maxpoll = 400, pollgap = 500, ctx = 0, wdat = 32'h44332211, settle = 2000, again = 0, dev2 = 'h50, w1c = 0, cmdx = 0, rnd = 0, rnd_n = 40, sideband = 0, sbwait = 3000;
  initial begin
    void'($value$plusargs("ch=%d", ch)); void'($value$plusargs("sel=%d", sel)); void'($value$plusargs("rd=%d", rd));
    void'($value$plusargs("addr=%h", addr)); void'($value$plusargs("len=%d", len)); void'($value$plusargs("off=%h", off));
    void'($value$plusargs("dev=%h", dev)); void'($value$plusargs("maxpoll=%d", maxpoll));
    void'($value$plusargs("pollgap=%d", pollgap)); void'($value$plusargs("ctx=%d", ctx));
    void'($value$plusargs("wdat=%h", wdat)); void'($value$plusargs("settle=%d", settle));
    void'($value$plusargs("again=%d", again)); void'($value$plusargs("dev2=%h", dev2)); void'($value$plusargs("w1c=%h", w1c));
    void'($value$plusargs("cmdx=%h", cmdx)); void'($value$plusargs("rnd=%d", rnd)); void'($value$plusargs("rnd_n=%d", rnd_n));
    void'($value$plusargs("sideband=%d", sideband)); void'($value$plusargs("sbwait=%d", sbwait));
    cb = 32'h1000 + ch * 32'h100;
    if (sideband) begin : sideband_mode
      integer k, j; reg [31:0] r140, r160, b140, b160; reg [255:0] o0; reg [127:0] pres_mask; pres_mask = 0;
      pcie_USERRSTN = 0; repeat (30) @(posedge clk); pcie_USERRSTN = 1; repeat (200) @(posedge clk);
      show_ins(); show_outs();
      mwr('h158, 32'hffff_ffff);   // mask the presence-change interrupt, as the vendor probe does
      mwr('h170, 0); mwr('h178, 0); repeat (50) @(posedge clk); o0 = ovec;
      $display("outs with 0x170=0 0x178=0: %b", ovec);
      for (k = 0; k < 32; k = k + 1) begin
        mwr('h170, 32'd1 << k); repeat (50) @(posedge clk);
        for (j = 0; j < $bits(ovec); j = j + 1) if (ovec[j] !== o0[j]) $display("LPMODE bit %0d -> out %0d now %0d", k, j, ovec[j]);
      end
      mwr('h170, 0); repeat (50) @(posedge clk);
      for (k = 0; k < 32; k = k + 1) begin
        mwr('h178, 32'd1 << k); repeat (50) @(posedge clk);
        for (j = 0; j < $bits(ovec); j = j + 1) if (ovec[j] !== o0[j]) $display("RESET bit %0d -> out %0d now %0d", k, j, ovec[j]);
      end
      mwr('h178, 0); repeat (50) @(posedge clk);
      mrd('h140, b140); mrd('h160, b160); $display("baseline 0x140=%08x 0x160=%08x", b140, b160);
      for (k = 0; k < N_IN; k = k + 1) begin
        set_in(k, 0); repeat (sbwait) @(posedge clk);
        stuck = 0; mrd('h140, r140); if (!stuck) mrd('h160, r160);
        if (stuck) begin $display("IN %0d low -> core stops taking TLPs (reset-like input)", k);
          set_in(k, 1); pcie_TRNRSRCRDY = 0; repeat (sbwait) @(posedge clk); stuck = 0; mrd('h140, r140);
          $display("IN %0d released -> 0x140=%08x", k, r140); continue; end
        if (r140 !== b140 || r160 !== b160) $display("IN %0d low -> 0x140 ^ %08x, 0x160 ^ %08x", k, r140 ^ b140, r160 ^ b160);
        if (r140 !== b140) pres_mask[k] = 1;
        set_in(k, 1); repeat (sbwait) @(posedge clk);
      end
      // LPMode again with every input that maps to 0x140 held low (module present)
      for (k = 0; k < N_IN; k = k + 1) if (pres_mask[k]) set_in(k, 0);
      repeat (sbwait) @(posedge clk); mrd('h140, r140); $display("all present: 0x140=%08x", r140);
      o0 = ovec;
      for (k = 0; k < 32; k = k + 1) begin
        mwr('h170, 32'd1 << k); repeat (50) @(posedge clk);
        for (j = 0; j < $bits(ovec); j = j + 1) if (ovec[j] !== o0[j]) $display("LPMODE(present) bit %0d -> out %0d now %0d", k, j, ovec[j]);
      end
      mwr('h170, 0); repeat (200) @(posedge clk);
      mrd('h160, b160); mrd('h140, b140); $display("present baseline 0x140=%08x 0x160=%08x", b140, b160);
      for (k = 0; k < N_IN; k = k + 1) if (!pres_mask[k] && k != 23) begin
        set_in(k, 0); repeat (sbwait) @(posedge clk); stuck = 0; mrd('h160, r160);
        if (!stuck && r160 !== b160) $display("INTL %0d low -> 0x160 ^ %08x", k, r160 ^ b160);
        if (stuck) $display("IN %0d low (present) -> core stops taking TLPs", k);
        set_in(k, 1); repeat (sbwait) @(posedge clk);
      end
      $finish;
    end
    if (rnd) begin : random_mode
      integer k, r, o; reg [31:0] d, rv;
      void'($urandom(rnd));
      pcie_USERRSTN = 0; repeat (30) @(posedge clk); pcie_USERRSTN = 1; repeat (100) @(posedge clk);
      mwr(cb + 'h0c, 32'h8000_0000); mwr(cb + 'h00, 32'h0066_0918); mwr(cb + 'h08, 1); repeat (2000) @(posedge clk);
      for (k = 0; k < rnd_n; k = k + 1) begin
        r = $urandom % 16; o = ($urandom % 64) * 4; d = $urandom;
        if (r < 3) begin   // a plausible transaction with random fields
          mwr(cb + 'h50, d & 32'h3fff_ffff); mwr(cb + 'h54, 32'h8000_0000 | (d & 32'hff));
        end else if (r < 4) mwr(cb + 'h58 + (d[0] ? 4 : 0), d);
        else if (r < 5) begin mrd(cb + o, rv); $display("[%0d] rd 0x%02x = 0x%08x", cyc, o, rv); end
        else if (r < 6) mwr(cb + 'h60, d);
        else if (r < 7) mwr(cb + o, d);
        repeat ($urandom % 3000) @(posedge clk);
      end
      repeat (20000) @(posedge clk);
      for (o = 0; o < 'h100; o = o + 4) begin mrd(cb + o, rv); $display("[%0d] final 0x%02x = 0x%08x", cyc, o, rv); end
      $finish;
    end
    pcie_USERRSTN = 0; repeat (30) @(posedge clk); pcie_USERRSTN = 1; repeat (100) @(posedge clk);
    $display("[%0d] after reset: TRNRDSTRDY=%0d", cyc, pcie_TRNRDSTRDY); $fflush;
    mwr(cb + 'h0c, 32'h8000_0000); mwr(cb + 'h08, 0); mwr(cb + 'h00, 32'h0066_0918);
    if (sel != 0) mwr(cb + 'h08, sel);
    repeat (settle) @(posedge clk);   // the 0x0C bit-31 pulse restarts the channel's BRAM wipe (~512 clocks)
    show_probes();
    mrd(cb + 'h00, v); $display("[%0d] read profile 0x%08x", cyc, v);
    mrd(cb + 'h08, v); $display("[%0d] read profsel 0x%08x", cyc, v);
    mrd(cb + 'h60, v); $display("[%0d] read status before 0x%08x", cyc, v);
    if (!rd) for (i = 0; i < (len + 3) / 4; i = i + 1) mwr('h4000 + ch * 'h200 + 4 * i, wdat + i * 32'h04040404);
    mwr(cb + (ctx ? 'h58 : 'h50), (rd ? 32'h1000_0000 : 0) | (addr << 8) | len | cmdx);
    mwr(cb + (ctx ? 'h5c : 'h54), 32'h8000_0000 | off);
    $display("[%0d] started", cyc);
    polls = 0;
    do begin repeat (pollgap) @(posedge clk); mrd(cb + (ctx ? 'h5c : 'h54), v); polls = polls + 1;
             if (polls < 4 || !v[31]) $display("[%0d] poll start reg 0x%08x", cyc, v); end
    while (v[31] && polls < maxpoll);
    mrd(cb + 'h60, st); $display("[%0d] status 0x%08x (bit1 %0d)", cyc, st, st[1]);
    for (i = 0; i < (len + 3) / 4; i = i + 1) begin mrd('h8000 + ch * 'h200 + 4 * i, v); $display("[%0d] read window[%0d] 0x%08x", cyc, i, v); end
    mrd(cb + 'h50, v); $display("[%0d] read cmd reg 0x%08x", cyc, v);
    if (w1c) begin mwr(cb + 'h60, w1c); mrd(cb + 'h60, st); $display("[%0d] status after writing 0x%02x to 0x60: 0x%08x", cyc, w1c, st); end
    if (again) begin
      dev = dev2; $display("[%0d] second transaction, device now 0x%02x", cyc, dev);
      mwr(cb + 'h50, (rd ? 32'h1000_0000 : 0) | (addr << 8) | len);
      mrd(cb + 'h60, st); $display("[%0d] status after cmd write, before start 0x%08x", cyc, st);
      mwr(cb + 'h54, 32'h8000_0000 | off);
      mrd(cb + 'h60, st); $display("[%0d] status just after start 0x%08x", cyc, st);
      polls = 0;
      do begin repeat (pollgap) @(posedge clk); mrd(cb + 'h54, v); polls = polls + 1; end while (v[31] && polls < maxpoll);
      mrd(cb + 'h60, st); $display("[%0d] status after second 0x%08x", cyc, st);
    end
    $display("SCL periods (rising to rising, within bytes): n=%0d avg=%0d.%02d min=%0d max=%0d",
             per_n, per_n ? per_sum / per_n : 0, per_n ? (per_sum * 100 / per_n) % 100 : 0, per_min, per_max);
    $finish;
  end
'''

if __name__ == '__main__':
    main()
