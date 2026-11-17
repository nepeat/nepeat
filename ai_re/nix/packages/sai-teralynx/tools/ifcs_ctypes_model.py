# SPDX-License-Identifier: Apache-2.0
"""Read a ctypesgen output file (IFCS pyctypes/ifcs_ctypes.py) as data.

Nothing in the file is executed: the lines ctypesgen writes (assignments,
class Structure/Union stubs, _fields_ lists, argtypes/restype) are matched
and their right-hand sides parsed with ast and evaluated by the small
interpreter below (integers, names, POINTER/CFUNCTYPE/UNCHECKED, arrays).
Layouts are computed by building ctypes types from that model (host ABI;
the target is LP64 too). Used by gen_ifcs_api.py.
"""
import ast, ctypes, re

PRIM = {   # ctypes name -> (C spelling, ctypes type)
    "c_int8": ("int8_t", ctypes.c_int8), "c_byte": ("int8_t", ctypes.c_byte), "c_char": ("char", ctypes.c_char),
    "c_uint8": ("uint8_t", ctypes.c_uint8), "c_ubyte": ("uint8_t", ctypes.c_ubyte),
    "c_int16": ("int16_t", ctypes.c_int16), "c_short": ("short", ctypes.c_short),
    "c_uint16": ("uint16_t", ctypes.c_uint16), "c_ushort": ("unsigned short", ctypes.c_ushort),
    "c_int32": ("int32_t", ctypes.c_int32), "c_int": ("int", ctypes.c_int),
    "c_uint32": ("uint32_t", ctypes.c_uint32), "c_uint": ("unsigned int", ctypes.c_uint),
    "c_int64": ("int64_t", ctypes.c_int64), "c_long": ("long", ctypes.c_long), "c_longlong": ("long long", ctypes.c_longlong),
    "c_uint64": ("uint64_t", ctypes.c_uint64), "c_ulong": ("unsigned long", ctypes.c_ulong),
    "c_ulonglong": ("unsigned long long", ctypes.c_ulonglong),
    "c_float": ("float", ctypes.c_float), "c_double": ("double", ctypes.c_double), "c_bool": ("_Bool", ctypes.c_bool),
    "c_size_t": ("size_t", ctypes.c_size_t), "c_void_p": ("void*", ctypes.c_void_p),
    "c_char_p": ("const char*", ctypes.c_char_p), "String": ("const char*", ctypes.c_char_p),
    "ReturnString": ("const char*", ctypes.c_char_p),
}


class T:
    """A type expression: kind in prim, name, ptr, arr, fn, void."""
    __slots__ = ("kind", "a", "b")
    def __init__(self, kind, a=None, b=None): self.kind, self.a, self.b = kind, a, b
    def __repr__(self): return "T(%s,%r,%r)" % (self.kind, self.a, self.b)


class Model:
    def __init__(self, path):
        self.path = path
        self.assign = {}     # name -> ast expr (last top-level/try assignment wins, as at import)
        self.aggs = {}       # struct_x / union_x -> "Structure" | "Union"
        self.fields = {}     # agg -> [(name, ast expr, bits|None)]
        self.pack = {}
        self.args = {}       # fn -> [ast expr]
        self.ret = {}        # fn -> ast expr
        self._ct = {}
        self._parse(open(path, encoding="utf-8", errors="replace").read())

    # ------------------------------------------------------------ parsing
    @staticmethod
    def _expr(s):
        s = re.sub(r"\b(0[xX][0-9a-fA-F]+|\d+)[lL]\b", r"\1", s.strip())
        return ast.parse(s, mode="eval").body

    def _parse(self, src):
        lines = src.splitlines()
        i = 0
        while i < len(lines) and not re.match(r'^_libs\[.*\] = load_library', lines[i]):
            i += 1                                   # preamble: ctypesgen's own helpers
        cur_fields = None
        for line in lines[i + 1:]:
            if cur_fields is not None:
                if line.startswith("]"):
                    cur_fields = None
                    continue
                m = re.match(r"^\s+\('(\w+)', (.+?)(?:, (\d+))?\),\s*$", line)
                if m:
                    cur_fields.append((m.group(1), self._expr(m.group(2)), int(m.group(3)) if m.group(3) else None))
                continue
            m = re.match(r"^class (\w+)\((Structure|Union)\):", line)
            if m:
                self.aggs[m.group(1)] = m.group(2)
                continue
            m = re.match(r"^(\w+)\._fields_ = \[\s*$", line)
            if m:
                cur_fields = self.fields[m.group(1)] = []
                continue
            m = re.match(r"^(\w+)\._pack_ = (\d+)", line)
            if m:
                self.pack[m.group(1)] = int(m.group(2))
                continue
            m = re.match(r"^\s+(\w+)\.argtypes = \[(.*)\]\s*$", line)
            if m:
                self.args[m.group(1)] = [self._expr(a) for a in split_top(m.group(2))] if m.group(2).strip() else []
                continue
            m = re.match(r"^\s+(\w+)\.restype = (.+?)\s*$", line)
            if m:
                self.ret.setdefault(m.group(1), self._expr(m.group(2)))
                continue
            m = re.match(r"^( {4})?(\w+) = (.+?)\s*$", line)
            if m and not m.group(3).startswith("_lib.") and not m.group(2).startswith("_"):
                try:
                    self.assign[m.group(2)] = self._expr(m.group(3))
                except SyntaxError:
                    pass

    # ------------------------------------------------------------ values
    def value(self, name, depth=0):
        """Integer value of a constant (enum member or #define), or None."""
        e = self.assign.get(name)
        return None if e is None else self._eval(e, depth)

    def _eval(self, e, depth=0):
        if depth > 50: return None
        if isinstance(e, ast.Constant):
            return e.value if isinstance(e.value, int) and not isinstance(e.value, bool) else None
        if isinstance(e, ast.Name):
            return self.value(e.id, depth + 1)
        if isinstance(e, ast.UnaryOp):
            v = self._eval(e.operand, depth + 1)
            if v is None: return None
            return {ast.USub: lambda x: -x, ast.UAdd: lambda x: x, ast.Invert: lambda x: ~x}.get(type(e.op), lambda x: None)(v)
        if isinstance(e, ast.Call) and isinstance(e.func, ast.Name) and e.func.id == "int" and len(e.args) == 1:
            return self._eval(e.args[0], depth + 1)       # newer ctypesgen: c_uint8 * int(6)
        if isinstance(e, ast.BinOp):
            a, b = self._eval(e.left, depth + 1), self._eval(e.right, depth + 1)
            if a is None or b is None: return None
            ops = {ast.Add: a.__add__, ast.Sub: a.__sub__, ast.Mult: a.__mul__, ast.LShift: a.__lshift__,
                   ast.RShift: a.__rshift__, ast.BitOr: a.__or__, ast.BitAnd: a.__and__, ast.BitXor: a.__xor__,
                   ast.FloorDiv: a.__floordiv__}
            f = ops.get(type(e.op))
            return f(b) if f else None
        return None

    # ------------------------------------------------------------ types
    def typ(self, e):
        """ast expr -> T (names kept: resolve() follows them)."""
        if isinstance(e, ast.Constant) and e.value is None: return T("void")
        if isinstance(e, ast.Name):
            if e.id in PRIM: return T("prim", e.id)
            return T("name", e.id)
        if isinstance(e, ast.Call) and isinstance(e.func, ast.Name):
            f = e.func.id
            if f == "POINTER": return T("ptr", self.typ(e.args[0]))
            if f == "UNCHECKED": return self.typ(e.args[0])
            if f == "CFUNCTYPE": return T("fn", self.typ(e.args[0]), [self.typ(a) for a in e.args[1:]])
        if isinstance(e, ast.BinOp) and isinstance(e.op, ast.Mult):
            n = self._eval(e.right)
            if n is not None: return T("arr", self.typ(e.left), n)
        raise ValueError("unsupported type expression %s" % ast.dump(e))

    def resolve(self, t):
        """Follow name aliases to a prim / agg / ptr / arr / fn / void."""
        seen = 0
        while t.kind == "name" and t.a not in self.aggs:
            e = self.assign.get(t.a)
            if e is None: raise KeyError("%s: type %s not defined" % (self.path, t.a))
            t = self.typ(e)
            seen += 1
            if seen > 50: raise ValueError("alias loop at %s" % t.a)
        return t

    def is_enum(self, name):
        return name.startswith("enum_") or (name in self.assign and isinstance(self.assign[name], ast.Name)
                                            and self.assign[name].id.startswith("enum_"))

    def ctype(self, t):
        t = self.resolve(t)
        if t.kind == "prim": return PRIM[t.a][1]
        if t.kind == "void": return None
        if t.kind == "ptr":
            tgt = self.resolve(t.a)
            if tgt.kind in ("void", "fn"): return ctypes.c_void_p
            if tgt.kind == "name": return ctypes.POINTER(self._shell(tgt.a))
            return ctypes.c_void_p if tgt.kind == "void" else ctypes.POINTER(self.ctype(tgt))
        if t.kind == "arr": return self.ctype(t.a) * t.b
        if t.kind == "fn": return ctypes.c_void_p
        if t.kind == "name": return self.agg_ctype(t.a)
        raise ValueError(t)

    def _shell(self, agg):
        if agg not in self._ct:
            base = ctypes.Union if self.aggs[agg] == "Union" else ctypes.Structure
            self._ct[agg] = type(agg, (base,), {})
        return self._ct[agg]

    def agg_ctype(self, agg):
        c = self._shell(agg)
        if not getattr(c, "_done", False):
            if agg not in self.fields: raise KeyError("%s: %s has no field list (incomplete)" % (self.path, agg))
            if agg in self.pack: c._pack_ = self.pack[agg]
            fl = []
            for n, e, bits in self.fields[agg]:
                ct = self.ctype(self.typ(e))
                fl.append((n, ct, bits) if bits else (n, ct))
            c._fields_ = fl
            c._done = True
        return c

    def agg_of(self, typedef):
        """typedef name -> struct_x/union_x it names, or None."""
        t = self.typ(ast.Name(typedef)) if typedef not in self.aggs else T("name", typedef)
        try:
            r = self.resolve(t)
        except (KeyError, ValueError):
            return None
        return r.a if r.kind == "name" and r.a in self.aggs else None


def split_top(s):
    out, depth, cur = [], 0, ""
    for ch in s:
        if ch in "([": depth += 1
        if ch in ")]": depth -= 1
        if ch == "," and depth == 0:
            out.append(cur); cur = ""
        else:
            cur += ch
    if cur.strip(): out.append(cur)
    return out
