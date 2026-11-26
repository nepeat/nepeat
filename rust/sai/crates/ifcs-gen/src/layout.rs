//! Sizes and alignments the reference generator asks ctypes for (`ctypes.sizeof` of a union it
//! pads, of a `@bits` struct and its storage type, of an `@as` enum): the model's `ctype` /
//! `agg_ctype` with the layout rules of Python 3.14's `ctypes._layout` (the Python of the Nix
//! build): the GCC / System V layout by default, the MSVC one for a struct with `_pack_` (3.14's
//! implicit choice). LP64 sizes (`x86_64`, `aarch64` Linux), as on the build host.

use crate::Fatal;
use crate::model::{AggKind, Model, T, prim};

/// A size and an alignment in bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    /// `ctypes.sizeof`.
    pub size: u64,
    /// `ctypes.alignment`.
    pub align: u64,
}

/// What a field's ctypes type is, for the layout.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CType {
    pub(crate) layout: Layout,
    /// An integer (or bool) type: may hold a bitfield.
    pub(crate) int_like: bool,
}

const PTR: CType = CType {
    layout: Layout { size: 8, align: 8 },
    int_like: false,
};

fn round_up(x: u64, m: u64) -> u64 {
    x.div_ceil(m) * m
}

fn round_down(x: u64, m: u64) -> u64 {
    x - x % m
}

impl Model {
    /// `ctype`: the ctypes type of a type expression (`None`: void).
    pub(crate) fn ctype(&self, t: &T) -> Result<Option<CType>, Fatal> {
        let t = self.resolve(t)?;
        Ok(match t {
            T::Prim(p) => prim(p).map(|&(_, _, size, int_like)| CType {
                layout: Layout { size, align: size },
                int_like,
            }),
            T::Void => None,
            T::Ptr(inner) => {
                let tgt = self.resolve(&inner)?;
                if !matches!(tgt, T::Void | T::Fn(..) | T::Name(_)) && self.ctype(&tgt)?.is_none() {
                    return Err(Fatal::Type("POINTER(None)".into()));
                }
                Some(PTR)
            }
            T::Arr(elem, n) => {
                let Some(e) = self.ctype(&elem)? else {
                    return Err(Fatal::Type(
                        "can't multiply sequence by non-int of type 'NoneType'".into(),
                    ));
                };
                let n = u64::try_from(n).map_err(|_| Fatal::Value(format!("Array length must be >= 0, not {n}")))?;
                let size = e
                    .layout
                    .size
                    .checked_mul(n)
                    .ok_or_else(|| Fatal::Limit("array too large".into()))?;
                Some(CType {
                    layout: Layout {
                        size,
                        align: e.layout.align,
                    },
                    int_like: false,
                })
            }
            T::Fn(..) => Some(PTR),
            T::Name(a) => Some(CType {
                layout: self.agg_layout(&a)?,
                int_like: false,
            }),
        })
    }

    /// `agg_ctype`: the layout of a struct / union of the file (built once, as the Python's class).
    pub(crate) fn agg_layout(&self, agg: &str) -> Result<Layout, Fatal> {
        if let Some(r) = self.layouts.borrow().get(agg) {
            return r.clone();
        }
        if !self.building.borrow_mut().insert(agg.to_owned()) {
            return Err(Fatal::Recursion(format!("{agg} contains itself")));
        }
        let r = self.compute(agg);
        self.building.borrow_mut().remove(agg);
        self.layouts.borrow_mut().insert(agg.to_owned(), r.clone());
        r
    }

    fn compute(&self, agg: &str) -> Result<Layout, Fatal> {
        let Some(fields) = self.fields.get(agg) else {
            return Err(Fatal::Key(format!(
                "{}: {agg} has no field list (incomplete)",
                self.path
            )));
        };
        let mut fl = Vec::with_capacity(fields.len());
        for f in fields {
            let Some(ct) = self.ctype(&self.typ(&f.ty)?)? else {
                return Err(Fatal::Type(format!(
                    "{agg}.{}: second item in _fields_ must be a C type",
                    f.name
                )));
            };
            // the model passes a width only when it is non-zero (`if bits`)
            fl.push((f.name.as_str(), ct, f.bits.filter(|b| *b != 0)));
        }
        let is_struct = self.aggs.get(agg) != Some(&AggKind::Union);
        layout(&fl, is_struct, self.pack.get(agg).copied().filter(|p| *p != 0))
    }
}

/// `ctypes._layout.get_layout` for fields (name, type, bit width).
pub(crate) fn layout(
    fields: &[(&str, CType, Option<u64>)],
    is_struct: bool,
    pack: Option<u64>,
) -> Result<Layout, Fatal> {
    let mut align = 1u64;
    let (mut next_bit, mut next_byte, mut last_field_bits) = (0i128, 0u64, 0u64);
    let (mut struct_size, mut union_size) = (0u64, 0u64);
    for &(name, ct, bits) in fields {
        if !is_struct {
            last_field_bits = 0;
            next_bit = 0;
            next_byte = 0;
        }
        let type_size = ct.layout.size;
        let type_bits = type_size * 8;
        let bit_size = match bits {
            Some(b) => {
                if b > type_bits {
                    return Err(Fatal::Value(format!("number of bits invalid for bit field '{name}'")));
                }
                if !ct.int_like {
                    return Err(Fatal::Type(format!("bit fields not allowed for the type of '{name}'")));
                }
                b
            }
            None => type_bits,
        };
        let mut type_align = ct.layout.align.max(1);
        match pack {
            None => {
                let (nb, tba) = (u64::try_from(next_bit).unwrap_or(0), type_align * 8);
                let slot_end = round_down(nb, tba) + type_bits;
                let nb = if nb + bit_size > slot_end {
                    round_up(nb, tba)
                } else {
                    nb
                };
                let nb = nb + bit_size;
                next_bit = i128::from(nb);
                struct_size = round_up(nb, 8) / 8;
            }
            Some(pack) => {
                type_align = type_align.min(pack);
                if 0 < next_bit + i128::from(bit_size) || type_bits != last_field_bits {
                    next_byte = round_up(next_byte, type_align) + type_size;
                    last_field_bits = type_bits;
                    next_bit = -i128::from(last_field_bits);
                }
                next_bit += i128::from(bit_size);
                struct_size = next_byte;
            }
        }
        union_size = union_size.max(struct_size);
        align = align.max(type_align);
    }
    let total = if is_struct { struct_size } else { union_size };
    Ok(Layout {
        size: round_up(total, align),
        align,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn int(size: u64) -> CType {
        CType {
            layout: Layout { size, align: size },
            int_like: true,
        }
    }

    // expected values: CPython 3.14 ctypes (Structure / Union subclasses with these _fields_)
    #[test]
    fn gcc_layout() {
        let l = |f: &[(&str, CType, Option<u64>)], s| layout(f, s, None).expect("layout");
        assert_eq!(
            l(&[("a", int(1), None), ("b", int(4), None)], true),
            Layout { size: 8, align: 4 }
        );
        assert_eq!(
            l(
                &[("a", int(4), None), ("b", int(1), Some(1)), ("c", int(1), Some(1))],
                true
            ),
            Layout { size: 8, align: 4 }
        );
        assert_eq!(
            l(&[("a", int(4), Some(3)), ("b", int(1), Some(6))], true),
            Layout { size: 4, align: 4 }
        );
        assert_eq!(
            l(&[("a", int(1), Some(5)), ("b", int(1), Some(5))], true),
            Layout { size: 2, align: 1 }
        );
        assert_eq!(l(&[("a", int(8), Some(1)); 20], true), Layout { size: 8, align: 8 });
        assert_eq!(
            l(&[("a", int(1), None), ("b", int(8), None)], false),
            Layout { size: 8, align: 8 }
        );
        assert_eq!(l(&[], true), Layout { size: 0, align: 1 });
    }

    #[test]
    fn ms_layout_with_pack() {
        let l = |f: &[(&str, CType, Option<u64>)], p| layout(f, true, Some(p)).expect("layout");
        assert_eq!(
            l(&[("a", int(1), None), ("b", int(4), None)], 1),
            Layout { size: 5, align: 1 }
        );
        assert_eq!(
            l(&[("a", int(1), None), ("b", int(4), None)], 2),
            Layout { size: 6, align: 2 }
        );
        assert_eq!(
            l(&[("a", int(4), Some(3)), ("b", int(1), Some(6))], 4),
            Layout { size: 8, align: 4 }
        );
    }

    #[test]
    fn bad_bitfields() {
        assert!(matches!(
            layout(&[("a", int(1), Some(9))], true, None),
            Err(Fatal::Value(_))
        ));
        assert!(matches!(
            layout(&[("a", PTR, Some(1))], true, None),
            Err(Fatal::Type(_))
        ));
    }
}
