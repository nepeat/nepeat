//! [`phy_seq::Op`] to and from the header's `phy_seq_op`. No unsafe: an operation from C is a
//! plain struct of integers here, and becomes a typed operation only if every field is in range.

use phy_seq::{
    ByteSrc, Compare, Dest, Dev, Format, Op, OpKind, ParamByte, ParamName, PhySel, Reg, ResultName, Slot, Tries, Usec,
    Value, Word,
};

use crate::sys;

/// A `PHY_SEQ_*` macro constant (bindgen: `u32`) as the `u8` a struct field holds.
const fn field(constant: u32) -> u8 {
    // every constant of the header that goes into a u8 field is below 256
    constant.to_le_bytes()[0]
}

pub(crate) const VALUE_CONST: u8 = field(sys::PHY_SEQ_VALUE_CONST);
pub(crate) const VALUE_BYTES: u8 = field(sys::PHY_SEQ_VALUE_BYTES);
pub(crate) const VALUE_KEPT: u8 = field(sys::PHY_SEQ_VALUE_KEPT);
pub(crate) const DEST_KEPT: u8 = field(sys::PHY_SEQ_DEST_KEPT);
pub(crate) const DEST_RESULT: u8 = field(sys::PHY_SEQ_DEST_RESULT);
const PARAM_NONE: u8 = field(sys::PHY_SEQ_PARAM_NONE);

/// `PHY_SEQ_PARAM_*` of a parameter.
pub(crate) fn param_id(name: ParamName) -> u8 {
    field(match name {
        ParamName::Key => sys::PHY_SEQ_PARAM_KEY,
        ParamName::Hkey => sys::PHY_SEQ_PARAM_HKEY,
        ParamName::Sci => sys::PHY_SEQ_PARAM_SCI,
        ParamName::Salt => sys::PHY_SEQ_PARAM_SALT,
        ParamName::Ssci => sys::PHY_SEQ_PARAM_SSCI,
        ParamName::Iv => sys::PHY_SEQ_PARAM_IV,
        ParamName::Pn => sys::PHY_SEQ_PARAM_PN,
        ParamName::Window => sys::PHY_SEQ_PARAM_WINDOW,
    })
}

/// The parameter with that `PHY_SEQ_PARAM_*`.
pub(crate) fn param_of(id: u8) -> Option<ParamName> {
    ParamName::ALL.into_iter().find(|&name| param_id(name) == id)
}

const EMPTY: sys::phy_seq_op = sys::phy_seq_op {
    line: 0,
    tries: 1,
    usec: 0,
    name_off: 0,
    name_len: 0,
    phy: 0,
    reg: 0,
    value: 0,
    mask: 0xffff,
    bits: 0,
    op: 0,
    port_phy: 0,
    dev: 0,
    value_kind: VALUE_CONST,
    dest_kind: DEST_KEPT,
    kept: 0,
    word: 0,
    hi_param: PARAM_NONE,
    hi_value: 0,
    lo_param: PARAM_NONE,
    lo_value: 0,
    reserved: [0; 3],
};

fn byte_fields(src: ByteSrc) -> (u8, u8) {
    match src {
        ByteSrc::Const(c) => (PARAM_NONE, c),
        ByteSrc::Param(p) => (param_id(p.name()), p.index()),
    }
}

/// The header's form of an operation. A result name is appended to `names`.
pub(crate) fn to_flat(op: &Op, names: &mut Vec<u8>) -> sys::phy_seq_op {
    let mut f = sys::phy_seq_op {
        line: op.line,
        op: u8::try_from(op.letter().as_char()).unwrap_or(0),
        ..EMPTY
    };
    if let Some(at) = op.reg() {
        match at.phy {
            PhySel::Port => f.port_phy = 1,
            PhySel::Fixed(phy) => f.phy = phy,
        }
        f.dev = at.dev.get();
        f.reg = at.reg;
    }
    match &op.kind {
        OpKind::Write { value, .. } => match *value {
            Value::Const(c) => f.value = c,
            Value::Kept { slot, mask, bits } => {
                f.value_kind = VALUE_KEPT;
                f.kept = slot.get();
                f.mask = mask;
                f.bits = bits;
            }
            Value::Bytes { hi, lo } => {
                f.value_kind = VALUE_BYTES;
                (f.hi_param, f.hi_value) = byte_fields(hi);
                (f.lo_param, f.lo_value) = byte_fields(lo);
            }
        },
        OpKind::Read { .. } => {}
        OpKind::Expect { cmp, .. } | OpKind::Cond { cmp, .. } => {
            f.value = cmp.want();
            f.mask = cmp.mask();
        }
        OpKind::Poll {
            cmp, tries, interval, ..
        } => {
            f.value = cmp.want();
            f.mask = cmp.mask();
            f.tries = u32::from(tries.get());
            f.usec = interval.get();
        }
        OpKind::Keep {
            dest: Dest::Kept(slot), ..
        } => f.kept = slot.get(),
        OpKind::Keep {
            dest: Dest::Result { name, word },
            ..
        } => {
            f.dest_kind = DEST_RESULT;
            f.word = word.get();
            // a sequence has at most 8192 operations with names of one table line each
            f.name_off = u32::try_from(names.len()).unwrap_or(u32::MAX);
            f.name_len = u32::try_from(name.as_str().len()).unwrap_or(u32::MAX);
            names.extend_from_slice(name.as_str().as_bytes());
        }
        OpKind::Delay(us) => f.usec = us.get(),
    }
    f
}

fn byte_src(param: u8, value: u8) -> Result<ByteSrc, &'static str> {
    if param == PARAM_NONE {
        return Ok(ByteSrc::Const(value));
    }
    let name = param_of(param).ok_or("unknown parameter")?;
    ParamByte::new(name, value)
        .map(ByteSrc::Param)
        .ok_or("parameter index beyond the parameter's length")
}

/// The typed operation of a `phy_seq_op` from C, or what is wrong with it. `format` decides
/// which letters exist: `X` and `D` in ext-macsec, `C` in ext-phy.
pub(crate) fn from_flat(f: &sys::phy_seq_op, names: &[u8], format: Format) -> Result<Op, &'static str> {
    let macsec = format == Format::ExtMacsec;
    let usec = || Usec::new(f.usec).ok_or("delay or interval above 100000 usec");
    if f.op == b'D' {
        if !macsec {
            return Err("D in an ext-phy sequence");
        }
        return Ok(Op {
            line: f.line,
            kind: OpKind::Delay(usec()?),
        });
    }
    let phy = match f.port_phy {
        0 => PhySel::Fixed(f.phy),
        1 if macsec => PhySel::Port,
        _ => return Err("port_phy is not 0 or 1 (or 1 in an ext-phy sequence)"),
    };
    let at = Reg {
        phy,
        dev: Dev::new(f.dev).ok_or("device is not 1..31")?,
        reg: f.reg,
    };
    let cmp = || Compare::new(f.value, f.mask).ok_or("compared value has bits outside the mask");
    let kept = || Slot::new(f.kept).ok_or("kept value is not $0..$15");
    let kind = match f.op {
        b'W' => {
            let value = match f.value_kind {
                VALUE_CONST => Value::Const(f.value),
                VALUE_KEPT if macsec => Value::Kept {
                    slot: kept()?,
                    mask: f.mask,
                    bits: f.bits,
                },
                VALUE_BYTES if macsec => Value::Bytes {
                    hi: byte_src(f.hi_param, f.hi_value)?,
                    lo: byte_src(f.lo_param, f.lo_value)?,
                },
                _ => return Err("unknown value kind (ext-phy writes constants only)"),
            };
            OpKind::Write { at, value }
        }
        b'R' => OpKind::Read { at },
        b'E' => OpKind::Expect { at, cmp: cmp()? },
        b'P' => {
            let tries = u16::try_from(f.tries)
                .ok()
                .and_then(Tries::new)
                .ok_or("tries is not 1..1000")?;
            OpKind::Poll {
                at,
                cmp: cmp()?,
                tries,
                interval: usec()?,
            }
        }
        b'C' if !macsec => OpKind::Cond { at, cmp: cmp()? },
        b'X' if macsec => {
            let dest = match f.dest_kind {
                DEST_KEPT => Dest::Kept(kept()?),
                DEST_RESULT => {
                    let start = usize::try_from(f.name_off).map_err(|_| "result name outside the names")?;
                    let len = usize::try_from(f.name_len).map_err(|_| "result name outside the names")?;
                    let name = start
                        .checked_add(len)
                        .and_then(|end| names.get(start..end))
                        .ok_or("result name outside the names")?;
                    let name = ResultName::new(name).ok_or("result name is not of a-z 0-9 _")?;
                    Dest::Result {
                        name,
                        word: Word::new(f.word).ok_or("result word is not 0..7")?,
                    }
                }
                _ => return Err("unknown destination kind"),
            };
            OpKind::Keep { at, dest }
        }
        _ => return Err("unknown operation letter for this format"),
    };
    Ok(Op { line: f.line, kind })
}

#[cfg(test)]
mod tests {
    use phy_seq::{ExtPhyTable, MacsecTable, Section};

    use super::*;

    #[test]
    fn every_kind_of_operation_survives_the_round_trip() {
        let table = MacsecTable::parse(
            b"ext-macsec 1\nseq s\nW p 1 a000 1234\nW 1e0 1f ffff key[31].00\nW p 1 1 ff.window[3]\nX p 2 3 $15\nW p 1 1 $15&ff00|1\n\
              R p 1 2\nE p 1 2 1 3\nE 7 1 2 ffff\nP p 1 2 0 8000 1000 100000\nX p 1 2 in_pkts_ok[7]\nX p 1 2 pn[0]\nD 100000\nD 0\n",
        )
        .unwrap();
        let ops = table.sequence("s").unwrap().ops();
        let mut names = Vec::new();
        let flat: Vec<_> = ops.iter().map(|op| to_flat(op, &mut names)).collect();
        assert_eq!(names, b"in_pkts_okpn");
        let back: Vec<_> = flat
            .iter()
            .map(|f| from_flat(f, &names, Format::ExtMacsec).unwrap())
            .collect();
        assert_eq!(back, ops);
        // none of them is an ext-phy operation, except the plain ones
        let as_phy: Vec<_> = flat
            .iter()
            .map(|f| from_flat(f, &names, Format::ExtPhy).is_ok())
            .collect();
        assert_eq!(
            as_phy,
            [
                false, false, false, false, false, false, false, true, false, false, false, false, false
            ]
        );

        let table = ExtPhyTable::parse(
            b"ext-phy 1\nport 1\nlink\nW 1 1 2 3\nR 1 1 2\nE 1 1 2 3\nP 1 1 2 1 1 3 7\nC 1 1f 2 3 3\n",
        )
        .unwrap();
        let ops = table.ports()[0].ops(Section::Link);
        let flat: Vec<_> = ops.iter().map(|op| to_flat(op, &mut names)).collect();
        let back: Vec<_> = flat
            .iter()
            .map(|f| from_flat(f, &[], Format::ExtPhy).unwrap())
            .collect();
        assert_eq!(back, ops);
        assert!(from_flat(&flat[4], &[], Format::ExtMacsec).is_err());
    }

    #[test]
    fn nonsense_from_c_is_refused_field_by_field() {
        let good = sys::phy_seq_op {
            op: b'W',
            dev: 1,
            ..EMPTY
        };
        assert!(from_flat(&good, &[], Format::ExtMacsec).is_ok());
        let bad = |change: fn(&mut sys::phy_seq_op), format| {
            let mut f = good;
            change(&mut f);
            from_flat(&f, b"pn", format).unwrap_err()
        };
        let m = Format::ExtMacsec;
        assert_eq!(bad(|f| f.dev = 0, m), "device is not 1..31");
        assert_eq!(bad(|f| f.dev = 32, m), "device is not 1..31");
        assert_eq!(bad(|f| f.op = b'N', m), "unknown operation letter for this format");
        assert_eq!(bad(|f| f.op = 0, m), "unknown operation letter for this format");
        assert_eq!(bad(|f| f.op = b'C', m), "unknown operation letter for this format");
        assert_eq!(
            bad(|f| f.op = b'X', Format::ExtPhy),
            "unknown operation letter for this format"
        );
        assert_eq!(bad(|f| f.op = b'D', Format::ExtPhy), "D in an ext-phy sequence");
        assert_eq!(
            bad(|f| f.port_phy = 2, m),
            "port_phy is not 0 or 1 (or 1 in an ext-phy sequence)"
        );
        assert_eq!(
            bad(|f| f.port_phy = 1, Format::ExtPhy),
            "port_phy is not 0 or 1 (or 1 in an ext-phy sequence)"
        );
        assert_eq!(
            bad(|f| f.value_kind = 3, m),
            "unknown value kind (ext-phy writes constants only)"
        );
        assert_eq!(
            bad(|f| f.value_kind = VALUE_BYTES, Format::ExtPhy),
            "unknown value kind (ext-phy writes constants only)"
        );
        assert_eq!(
            bad(|f| (f.value_kind, f.kept) = (VALUE_KEPT, 16), m),
            "kept value is not $0..$15"
        );
        assert_eq!(
            bad(|f| (f.value_kind, f.hi_param) = (VALUE_BYTES, 9), m),
            "unknown parameter"
        );
        assert_eq!(
            bad(
                |f| (f.value_kind, f.lo_param, f.lo_value) = (VALUE_BYTES, field(sys::PHY_SEQ_PARAM_HKEY), 16),
                m
            ),
            "parameter index beyond the parameter's length"
        );
        assert_eq!(
            bad(|f| (f.op, f.value, f.mask) = (b'E', 2, 1), m),
            "compared value has bits outside the mask"
        );
        assert_eq!(bad(|f| (f.op, f.tries) = (b'P', 0), m), "tries is not 1..1000");
        assert_eq!(bad(|f| (f.op, f.tries) = (b'P', 1001), m), "tries is not 1..1000");
        assert_eq!(
            bad(|f| (f.op, f.usec) = (b'P', 100_001), m),
            "delay or interval above 100000 usec"
        );
        assert_eq!(
            bad(|f| (f.op, f.usec) = (b'D', 100_001), m),
            "delay or interval above 100000 usec"
        );
        assert_eq!(bad(|f| (f.op, f.dest_kind) = (b'X', 2), m), "unknown destination kind");
        assert_eq!(bad(|f| (f.op, f.kept) = (b'X', 16), m), "kept value is not $0..$15");
        assert_eq!(
            bad(|f| (f.op, f.dest_kind, f.name_len) = (b'X', DEST_RESULT, 3), m),
            "result name outside the names"
        );
        assert_eq!(
            bad(
                |f| (f.op, f.dest_kind, f.name_off, f.name_len) = (b'X', DEST_RESULT, u32::MAX, 2),
                m
            ),
            "result name outside the names"
        );
        assert_eq!(
            bad(|f| (f.op, f.dest_kind) = (b'X', DEST_RESULT), m),
            "result name is not of a-z 0-9 _"
        );
        assert_eq!(
            bad(
                |f| (f.op, f.dest_kind, f.name_len, f.word) = (b'X', DEST_RESULT, 2, 8),
                m
            ),
            "result word is not 0..7"
        );
    }

    #[test]
    fn parameter_ids_are_the_headers() {
        for name in ParamName::ALL {
            assert_eq!(param_of(param_id(name)), Some(name));
        }
        assert_eq!((param_of(0), param_of(9)), (None, None));
        assert_eq!(u32::from(param_id(ParamName::Window)), sys::PHY_SEQ_PARAM_WINDOW);
    }
}
