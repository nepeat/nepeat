#!/usr/bin/env python3
"""Analysis helpers over the simulation Verilog that xo3vlog.py --sim writes. Generic.

    import xo3ana; D = xo3ana.Design('sim.v')
    D.regs[name]      -> {kind: ff|latch|iol, clk, edge, arst, arst_val, srst, ce, d, init, tag}
    D.expr[name]      -> combinational expression (python-evaluable form) of a wire
    D.support(net)    -> set of leaves (register outputs, pad inputs, constants excluded)
    D.tt(net, leaves) -> truth table (int, bit i = value for leaf assignment i, leaf k = bit k of i)
    D.fanout[net]     -> set of nets/regs that read it

Command line:
    xo3ana.py SIM.v summary            register classes, carry chains
    xo3ana.py SIM.v cone NET [NET...]  support of each net
    xo3ana.py SIM.v func NET           minimised SOP over its support (<= 12 leaves)

A leaf is any reg output or `*_in` pad port. `gsr` is a wire like any other.
Constants: 1'b1 = -1 (all ones), 1'b0 = 0, so truth tables are python ints
evaluated bit-parallel and masked at the end.
"""
import re, sys, collections
from builtins import compile

TOK = re.compile(r"1'b[01]|[A-Za-z_][A-Za-z0-9_]*|[~&|^()?:]")


def to_py(e):
    """Verilog expr (~ & | ^ ?: parentheses, 1'bX) -> python expression string over V[...]"""
    e = e.strip()
    m = re.fullmatch(r'(\S+) \? (\S+) : (\S+)', e)
    if m:
        s, a, b = (to_py(x) for x in m.groups())
        return '((%s)&(%s))|((~(%s))&(%s))' % (s, a, s, b)
    out = []
    for t in TOK.findall(e):
        if t == "1'b1":
            out.append('(-1)')
        elif t == "1'b0":
            out.append('0')
        elif re.match(r'[A-Za-z_]', t):
            out.append('V[%r]' % t)
        else:
            out.append(t)
    return ''.join(out)


def names_in(e):
    return [t for t in TOK.findall(e) if re.match(r'[A-Za-z_]', t)]


class Design:
    def __init__(self, path, cuts=()):
        src = open(path).read()
        self.src = src
        self.vexpr = {}      # verilog text
        self.regs = {}
        self.ports_in = re.findall(r'^\s*input\s+(\w+)', src, re.M)
        self.ports_out = re.findall(r'^\s*output\s+(\w+)', src, re.M)
        for m in re.finditer(r'^assign (\w+) = (.*?);(?: //.*)?$', src, re.M):
            self.vexpr[m.group(1)] = m.group(2)
        for m in re.finditer(r'^wire (\w+) = (.*?);(?: //.*)?$', src, re.M):
            self.vexpr[m.group(1)] = m.group(2)
        tags = {m.group(1): (m.group(2), m.group(3)) for m in
                re.finditer(r'^initial (\w+) = (1\'b[01]); // (.*)$', src, re.M)}
        for m in re.finditer(r'^always @\*\s*if \((.*?)\) (\w+) = (1\'b[01]); else if \((.*?)\) \w+ = (\w+);', src, re.M):
            r, q, v, en, d = m.groups()
            self.regs[q] = dict(kind='latch', clk=None, edge=None, arst=r, arst_val=v, srst=None, ce=None, en=en, d=d)
        for m in re.finditer(r'^always @\((.*?)\) (.*)$', src, re.M):
            sens, body = m.groups()
            if sens == '*':
                continue
            parts = [p.strip() for p in sens.split(' or ')]
            edge, clk = parts[0].split()
            asyn = None
            if len(parts) > 1:
                e2, n2 = parts[1].split()
                asyn = (e2, n2)
            stm = [s.strip() for s in body.split(' else ')]
            q = re.findall(r'(\w+) <=', body)[-1]
            r = dict(kind='ff', clk=clk, edge=edge, arst=None, arst_val=None, srst=None, srst_val=None, ce=None, d=None)
            for s in stm:
                mm = re.fullmatch(r'if \((.*?)\) (\w+) <= (.*?);', s)
                if mm:
                    cond, _, val = mm.groups()
                    if asyn and r['arst'] is None and cond.lstrip('~') == asyn[1]:
                        r['arst'] = cond; r['arst_val'] = val
                    elif val in ("1'b0", "1'b1") and r['srst'] is None and s is not stm[-1]:
                        r['srst'] = cond; r['srst_val'] = val
                    else:
                        r['ce'] = cond; r['d'] = val
                else:
                    mm = re.fullmatch(r'(\w+) <= (.*?);', s)
                    r['d'] = mm.group(2)
            if 'IOLOGIC' in q:
                r['kind'] = 'iol'
            self.regs[q] = r
        for q, (v, tag) in tags.items():
            if q in self.regs:
                self.regs[q]['init'] = v
                self.regs[q]['tag'] = tag
        self.leaves = set(self.regs) | {p for p in self.ports_in} | set(cuts)
        self.py = {n: compile(to_py(e), n, 'eval') for n, e in self.vexpr.items()}
        self.deps = {n: set(names_in(e)) for n, e in self.vexpr.items()}
        self.fanout = collections.defaultdict(set)
        for n, ds in self.deps.items():
            for d in ds:
                self.fanout[d].add(n)
        for q, r in self.regs.items():
            for k in ('clk', 'arst', 'srst', 'ce', 'd', 'en'):
                if r.get(k):
                    for d in names_in(r[k]):
                        self.fanout[d].add(q)
        self._sup = {}

    def support(self, n):
        if n in self._sup:
            return self._sup[n]
        if n in self.leaves:
            return {n}
        out = set()
        st = [n]
        seen = set()
        while st:
            x = st.pop()
            if x in seen:
                continue
            seen.add(x)
            if x in self.leaves:
                out.add(x); continue
            if x in self.deps:
                st.extend(self.deps[x])
            else:
                out.add(x)
        self._sup[n] = out
        return out

    def support_expr(self, e):
        s = set()
        for n in names_in(e):
            s |= self.support(n)
        return s

    def evaluate(self, n, env, memo):
        if n in env:
            return env[n]
        if n in self.leaves:
            raise KeyError('leaf %s not assigned' % n)
        if n in memo:
            return memo[n]
        if n not in self.py:
            raise KeyError('leaf %s not assigned' % n)
        V = _Lazy(self, env, memo)
        v = eval(self.py[n], {'V': V})
        memo[n] = v
        return v

    def eval_expr(self, e, env, memo=None):
        memo = {} if memo is None else memo
        V = _Lazy(self, env, memo)
        return eval(compile(to_py(e), 'e', 'eval'), {'V': V})

    def tt(self, e, leaves):
        """truth table of expr/net e over leaves (list)."""
        n = len(leaves)
        M = (1 << (1 << n)) - 1
        env = {}
        for k, l in enumerate(leaves):
            pat = 0
            blk = 1 << k
            unit = ((1 << blk) - 1) << blk
            period = blk * 2
            for s in range(0, 1 << n, period):
                pat |= unit << s
            env[l] = pat
        return self.eval_expr(e, env) & M


class _Lazy:
    def __init__(self, D, env, memo):
        self.D, self.env, self.memo = D, env, memo

    def __getitem__(self, n):
        return self.D.evaluate(n, self.env, self.memo)


def sop(tt, names):
    """minimised sum of products (Quine-McCluskey, fine up to ~12 vars)."""
    n = len(names)
    ones = [m for m in range(1 << n) if (tt >> m) & 1]
    if not ones:
        return "0"
    if len(ones) == 1 << n:
        return "1"
    terms = {(m, 0) for m in ones}
    primes = set()
    while terms:
        nxt, used = set(), set()
        bym = collections.defaultdict(list)
        for t in terms:
            bym[t[1]].append(t)
        for mk, lst in bym.items():
            s = set(v for v, _ in lst)
            for v in s:
                for b in range(n):
                    if not (mk >> b) & 1 and not (v >> b) & 1 and (v | 1 << b) in s:
                        nxt.add((v, mk | 1 << b)); used.add((v, mk)); used.add((v | 1 << b, mk))
        primes |= terms - used
        terms = nxt

    def cov(p, m):
        return (m & ~p[1]) == (p[0] & ~p[1])
    left = set(ones)
    chosen = []
    while left:
        ess = None
        for m in sorted(left):
            c = [p for p in primes if cov(p, m)]
            if len(c) == 1:
                ess = c[0]; break
        if ess is None:
            ess = max(primes, key=lambda p: (sum(1 for m in left if cov(p, m)), bin(p[1]).count('1')))
        chosen.append(ess)
        left = {m for m in left if not cov(ess, m)}
    out = []
    for v, mk in chosen:
        lits = [(names[i] if (v >> i) & 1 else '~' + names[i]) for i in range(n) if not (mk >> i) & 1]
        out.append('&'.join(lits) if lits else '1')
    return ' | '.join(out)


def main():
    D = Design(sys.argv[1])
    op = sys.argv[2]
    if op == 'summary':
        c = collections.Counter((r['kind'], r['clk'], bool(r.get('arst')), bool(r.get('srst')), bool(r.get('ce'))) for r in D.regs.values())
        for k, v in c.most_common():
            print(v, k)
    elif op == 'cone':
        for n in sys.argv[3:]:
            print(n, sorted(D.support(n)))
    elif op == 'func':
        for n in sys.argv[3:]:
            s = sorted(D.support(n))
            print(n, '=', sop(D.tt(n, s), s) if len(s) <= 14 else 'support %d: %s' % (len(s), s))


if __name__ == '__main__':
    main()
