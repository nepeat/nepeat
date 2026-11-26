r"""Wrapper for mini.h: a SYNTHETIC ctypesgen-shaped file for ifcs-gen's tests (no vendor content).

Generated with: nothing (written by hand in ctypesgen's shapes).
"""
# the preamble (ctypesgen's helpers) is skipped up to the library load
class String(object):
    pass

_libs["libmini.so"] = load_library("libmini.so")

enum_mini_reason = c_int# /src/mini.h: 3
MINI_OK = 0# /src/mini.h: 3
MINI_FAIL = (MINI_OK + 1)# /src/mini.h: 3
MINI_BUSY = ((MINI_OK + 1) + 1)# /src/mini.h: 3
mini_reason_t = enum_mini_reason# /src/mini.h: 3
enum_mini_kind = c_uint
mini_kind_t = enum_mini_kind
mini_status_t = c_uint32
mini_handle_t = c_uint32
mini_bool_t = c_uint8
mini_mac_t = c_uint8 * 6
mini_text_t = String
mini_alias_t = mini_handle_t
mini_unparsable = (1 +
_mini_private = 5
mini_lib_fn = _lib.get("x")
try:
    MINI_MASK = ((1 << 5) - 1)
except:
    pass
try:
    MINI_HANDLE_MAX = 0x1fffffL
except:
    pass
try:
    MINI_LONG = 10L
except:
    pass
try:
    MINI_NEG = (-3)
except:
    pass
try:
    MINI_DIV = (MINI_LONG // 3)
except:
    pass
try:
    MINI_FLOAT = 1.5
except:
    pass

class struct_mini_list(Structure):
    pass

struct_mini_list.__slots__ = [
    'count',
    'arr',
]
struct_mini_list._fields_ = [
    ('count', c_uint32),
    ('arr', POINTER(c_uint32)),
]

mini_list_t = struct_mini_list

class struct_mini_pair(Structure):
    pass

struct_mini_pair._fields_ = [
    ('a', mini_handle_t),
    ('b', mini_mac_t),
    ('_dirty_a', c_uint8, 1),
    ('_dirty_b', c_uint8, 1),
]

mini_pair_t = struct_mini_pair

class struct_mini_pair2(Structure):
    pass

struct_mini_pair2._fields_ = [
    ('a', mini_handle_t),
    ('b', mini_mac_t),
    ('_dirty_a', c_uint8, 1),
    ('_dirty_b', c_uint8, 1),
]

mini_pair2_t = struct_mini_pair2

class union_mini_value(Union):
    pass

union_mini_value._fields_ = [
    ('u32', c_uint32),
    ('pair', mini_pair_t),
    ('list', mini_list_t),
    ('big', c_uint8 * int(40)),
]

mini_value_t = union_mini_value

class struct_mini_bitmap(Structure):
    pass

struct_mini_bitmap._fields_ = [
    ('u32', c_uint64, 1),
    ('pair', c_uint64, 1),
    ('list', c_uint64, 1),
    ('big', c_uint64, 1),
]

mini_bitmap_t = struct_mini_bitmap

class struct_mini_attr(Structure):
    pass

struct_mini_attr._fields_ = [
    ('id', c_uint32),
    ('bitmap', mini_bitmap_t),
    ('value', mini_value_t),
    ('kind', mini_kind_t),
    ('reason', mini_reason_t),
    ('name', mini_text_t),
]

mini_attr_t = struct_mini_attr

class struct_mini_usage(Structure):
    pass

struct_mini_usage._fields_ = [
    ('max', c_uint32),
    ('current', c_uint32),
    ('extra', c_uint32),
    ('_dirty_max', c_uint8, 1),
    ('_dirty_current', c_uint8, 1),
    ('_dirty_extra', c_uint8, 1),
]

mini_usage_t = struct_mini_usage

class struct_mini_mixed(Structure):
    pass

struct_mini_mixed._fields_ = [
    ('a', c_uint8, 3),
    ('b', c_uint32, 7),
]

mini_mixed_t = struct_mini_mixed

class struct_mini_incomplete(Structure):
    pass

mini_incomplete_t = struct_mini_incomplete

class struct_mini_wide(Structure):
    pass

struct_mini_wide._fields_ = [
    ('first_handle_of_a_long_struct', mini_handle_t),
    ('second_handle_of_a_long_struct', mini_handle_t),
    ('third_handle_of_a_long_struct', mini_handle_t),
    ('callback', CFUNCTYPE(UNCHECKED(c_int), POINTER(None), mini_handle_t)),
    ('names', POINTER(POINTER(c_char))),
    ('macs', mini_mac_t * 2),
    ('opaque', POINTER(mini_list_t)),
]

mini_wide_t = struct_mini_wide

mini_notify_fn_t = CFUNCTYPE(UNCHECKED(None), mini_handle_t, POINTER(mini_attr_t), POINTER(None))# /src/mini.h: 40

# /src/mini.h: 50
if _libs["libmini.so"].has("mini_create", "cdecl"):
    mini_create = _libs["libmini.so"].get("mini_create", "cdecl")
    mini_create.argtypes = [POINTER(mini_handle_t), c_uint32, POINTER(mini_attr_t)]
    mini_create.restype = mini_status_t

if _libs["libmini.so"].has("mini_register", "cdecl"):
    mini_register = _libs["libmini.so"].get("mini_register", "cdecl")
    mini_register.argtypes = [POINTER(mini_notify_fn_t), POINTER(None)]
    mini_register.restype = mini_status_t
    mini_register.restype = c_int

if _libs["libmini.so"].has("mini_name", "cdecl"):
    mini_name = _libs["libmini.so"].get("mini_name", "cdecl")
    mini_name.argtypes = [mini_handle_t, POINTER(mini_usage_t), mini_bool_t]
    mini_name.restype = String

if _libs["libmini.so"].has("mini_void", "cdecl"):
    mini_void = _libs["libmini.so"].get("mini_void", "cdecl")
    mini_void.argtypes = []
