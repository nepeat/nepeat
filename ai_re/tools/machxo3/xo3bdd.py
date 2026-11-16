#!/usr/bin/env python3
"""Tiny ROBDD for xo3ana expressions (exact cofactors / supports of netlist cones). Generic.

    from xo3bdd import BDD; B = BDD(D)            # D = xo3ana.Design
    f = B.net(name, env)   env: leaf -> 0 | 1 | None (None = free variable)
    B.support(f), B.sop(f) (minimised, <= 16 vars), B.const(f) -> 0/1/None
Variables are created on first use, in that order.
"""
import xo3ana


class Node:
    __slots__ = ('B', 'id')

    def __init__(self, B, i):
        self.B, self.id = B, i

    def _o(self, x):
        if isinstance(x, Node):
            return x.id
        return 1 if x == -1 or x == 1 else 0

    def __and__(self, x): return Node(self.B, self.B.ite(self.id, self._o(x), 0))
    __rand__ = __and__

    def __or__(self, x): return Node(self.B, self.B.ite(self.id, 1, self._o(x)))
    __ror__ = __or__

    def __xor__(self, x):
        o = self._o(x)
        return Node(self.B, self.B.ite(self.id, self.B.neg(o), o))
    __rxor__ = __xor__

    def __invert__(self): return Node(self.B, self.B.neg(self.id))


class BDD:
    def __init__(self, D):
        self.D = D
        self.var, self.lo, self.hi = [10 ** 9, 10 ** 9], [0, 1], [0, 1]   # ids 0 / 1 = constants
        self.uniq, self.memo = {}, {}
        self.vars, self.vindex = [], {}

    def mk(self, v, l, h):
        if l == h:
            return l
        k = (v, l, h)
        if k not in self.uniq:
            self.var.append(v); self.lo.append(l); self.hi.append(h)
            self.uniq[k] = len(self.var) - 1
        return self.uniq[k]

    def neg(self, f):
        return self.ite(f, 0, 1)

    def ite(self, f, g, h):
        if f == 1: return g
        if f == 0: return h
        if g == 1 and h == 0: return f
        if g == h: return g
        k = (f, g, h)
        if k in self.memo:
            return self.memo[k]
        v = min(self.var[f], self.var[g], self.var[h])
        def cof(x, b):
            if self.var[x] != v: return x
            return self.hi[x] if b else self.lo[x]
        r = self.mk(v, self.ite(cof(f, 0), cof(g, 0), cof(h, 0)), self.ite(cof(f, 1), cof(g, 1), cof(h, 1)))
        self.memo[k] = r
        return r

    def leaf(self, name):
        if name not in self.vindex:
            self.vindex[name] = len(self.vars); self.vars.append(name)
        return self.mk(self.vindex[name], 0, 1)

    def net(self, n, env, cache=None):
        cache = {} if cache is None else cache
        D = self.D
        B = self

        class V:
            def __getitem__(s, k):
                return Node(B, B.net(k, env, cache))
        if n in cache:
            return cache[n]
        if n in env and env[n] is not None:
            r = 1 if env[n] else 0
        elif n in D.leaves or n not in D.py:
            r = self.leaf(n)
        else:
            v = eval(D.py[n], {'V': V()})
            r = v.id if isinstance(v, Node) else (1 if v == -1 or v == 1 else 0)
        cache[n] = r
        return r

    def expr(self, e, env):
        cache = {}
        B = self

        class V:
            def __getitem__(s, k):
                return Node(B, B.net(k, env, cache))
        v = eval(xo3ana.compile(xo3ana.to_py(e), 'e', 'eval'), {'V': V()})
        return v.id if isinstance(v, Node) else (1 if v == -1 or v == 1 else 0)

    def support(self, f):
        out, st, seen = set(), [f], set()
        while st:
            x = st.pop()
            if x < 2 or x in seen: continue
            seen.add(x); out.add(self.vars[self.var[x]])
            st += [self.lo[x], self.hi[x]]
        return sorted(out)

    def evalf(self, f, asg):
        while f > 1:
            f = self.hi[f] if asg[self.vars[self.var[f]]] else self.lo[f]
        return f

    def sop(self, f, maxv=16):
        if f in (0, 1):
            return str(f)
        s = self.support(f)
        if len(s) > maxv:
            return '<%d vars: %s>' % (len(s), ' '.join(s))
        tt = 0
        for i in range(1 << len(s)):
            if self.evalf(f, {x: (i >> k) & 1 for k, x in enumerate(s)}):
                tt |= 1 << i
        return xo3ana.sop(tt, s)
