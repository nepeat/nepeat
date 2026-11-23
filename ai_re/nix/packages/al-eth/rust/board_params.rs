// SPDX-License-Identifier: GPL-2.0

//! al_eth board parameters: the three scratch registers the boot loader fills
//! and the driver reads at probe (media type, PHY, MDIO, SerDes, retimer, GPIOs).
//!
//! A port of al_eth_board_params_set()/_get() of ../src/al_hal_eth_main.c with
//! the same bit layout and the same treatment of every input: fields are masked
//! to their width on encode, reserved bits are ignored on decode, and enum-like
//! fields carry whatever the bits say (the C enums are not range-checked
//! either; use the is_known() helpers before acting on one). Only core is used
//! and there is no unsafe code, so the file can be a kernel module's
//! "mod board_params;" as it is. Checked against the C on the build host:
//! ../rust.nix.

/// The registers, in MAC BAR order of use: mac_1g.scratch (+0x4),
/// mac_10g.scratch (+0x404), mac_1g.mac_0 (+0xc).
pub type Regs = [u32; 3];

macro_rules! raw_enum {
    ($(#[$doc:meta])* $name:ident { $($(#[$vdoc:meta])* $variant:ident = $value:expr,)+ }) => {
        $(#[$doc])*
        #[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
        pub struct $name(pub u8);

        impl $name {
            $($(#[$vdoc])* pub const $variant: Self = Self($value);)+

            /// Whether the value is one the HAL names.
            pub const fn is_known(self) -> bool {
                matches!(self.0, $($value)|+)
            }
        }
    };
}

raw_enum! {
    /// enum al_eth_board_media_type (4 bits).
    MediaType {
        AUTO_DETECT = 0,
        RGMII = 1,
        TEN_GBASE_SR = 2,
        SGMII = 3,
        THOUSAND_BASE_X = 4,
        AUTO_DETECT_AUTO_SPEED = 5,
        SGMII_2_5G = 6,
        NBASE_T = 7,
        TWENTY_FIVE_G = 8,
        TWENTY_FIVE_G_10G_AUTO = 9,
        FORTY_G = 10,
    }
}

raw_enum! {
    /// enum al_eth_board_mdio_freq (2 bits).
    MdioFreq {
        MHZ_2_5 = 0,
        MHZ_1 = 1,
    }
}

raw_enum! {
    /// enum al_eth_board_ext_phy_if (2 bits).
    PhyIf {
        MDIO = 0,
        XMDIO = 1,
        I2C = 2,
    }
}

raw_enum! {
    /// enum al_eth_ref_clk_freq (3 bits).
    RefClkFreq {
        MHZ_375 = 0,
        MHZ_187_5 = 1,
        MHZ_250 = 2,
        MHZ_500 = 3,
        MHZ_428 = 4,
    }
}

raw_enum! {
    /// enum al_eth_board_1g_speed (2 bits): the speed when autonegotiation is off.
    Speed1g {
        M1000 = 0,
        M100 = 1,
        M10 = 2,
    }
}

raw_enum! {
    /// enum al_eth_retimer_type (2 bits).
    RetimerType {
        BR_210 = 0,
        BR_410 = 1,
        DS_25 = 2,
    }
}

/// struct al_eth_board_params. Numeric fields hold only as many bits as the
/// registers do (the width is in each field's comment); encode drops the rest.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct BoardParams {
    // register 1
    pub media_type: MediaType,
    /// An external PHY exists.
    pub phy_exist: bool,
    /// MDIO address of the external PHY (5 bits).
    pub phy_mdio_addr: u8,
    pub sfp_plus_module_exist: bool,
    pub autoneg_enable: bool,
    pub kr_lt_enable: bool,
    pub kr_fec_enable: bool,
    pub mdio_freq: MdioFreq,
    /// I2C adapter of the SFP+ module (4 bits).
    pub i2c_adapter_id: u8,
    pub phy_if: PhyIf,
    /// Autonegotiation in band (else out of band).
    pub an_in_band: bool,
    /// SerDes group (3 bits, stored as bits 26:25 and bit 23).
    pub serdes_grp: u8,
    /// SerDes lane (2 bits).
    pub serdes_lane: u8,
    pub ref_clk_freq: RefClkFreq,
    // register 2
    pub dont_override_serdes: bool,
    pub force_1000_base_x: bool,
    pub an_disable: bool,
    pub speed: Speed1g,
    pub half_duplex: bool,
    pub fc_disable: bool,
    pub retimer_exist: bool,
    /// I2C bus of the retimer (4 bits).
    pub retimer_bus_id: u8,
    /// I2C address of the retimer (7 bits).
    pub retimer_i2c_addr: u8,
    /// Retimer Rx channel, A = 0 .. H = 7 (3 bits, stored as bit 19 and bits 28:27).
    pub retimer_channel: u8,
    /// Assume a direct attached cable when detection is off or fails.
    pub dac: bool,
    /// Assumed cable length (4 bits).
    pub dac_len: u8,
    pub retimer_type: RetimerType,
    /// Retimer Tx channel (3 bits).
    pub retimer_tx_channel: u8,
    // register 3
    /// GPIO of SFP present, 0 = none (6 bits).
    pub gpio_sfp_present: u8,
    pub auto_fec_enable: bool,
    /// GPIO of the 1G/10G speed indication, 0 = none (6 bits).
    pub gpio_spd_1g_10g: u8,
    /// GPIO of the 25G speed indication, 0 = none (6 bits).
    pub gpio_spd_25g: u8,
}

/// Bits msb..=lsb of reg.
const fn get(reg: u32, msb: u32, lsb: u32) -> u8 {
    // Every field is at most 7 bits wide, so the value fits a u8.
    ((reg >> lsb) & ((1 << (msb - lsb + 1)) - 1)) as u8
}

const fn bit(reg: u32, n: u32) -> bool {
    reg & (1 << n) != 0
}

/// val, cut to the width of bits msb..=lsb and moved there.
const fn put(val: u8, msb: u32, lsb: u32) -> u32 {
    ((val as u32) & ((1 << (msb - lsb + 1)) - 1)) << lsb
}

const fn flag(val: bool, n: u32) -> u32 {
    (val as u32) << n
}

impl BoardParams {
    /// Reads the registers; None when the boot loader left them unset (register
    /// 1 is 0: the C function's -ENOENT).
    pub const fn decode(regs: Regs) -> Option<Self> {
        let [r1, r2, r3] = regs;
        if r1 == 0 {
            return None;
        }
        Some(Self {
            media_type: MediaType(get(r1, 3, 0)),
            phy_exist: bit(r1, 4),
            phy_mdio_addr: get(r1, 9, 5),
            sfp_plus_module_exist: bit(r1, 10),
            autoneg_enable: bit(r1, 11),
            kr_lt_enable: bit(r1, 12),
            kr_fec_enable: bit(r1, 13),
            mdio_freq: MdioFreq(get(r1, 15, 14)),
            i2c_adapter_id: get(r1, 19, 16),
            phy_if: PhyIf(get(r1, 21, 20)),
            an_in_band: bit(r1, 22),
            serdes_grp: get(r1, 26, 25) | (get(r1, 23, 23) << 2),
            serdes_lane: get(r1, 28, 27),
            ref_clk_freq: RefClkFreq(get(r1, 31, 29)),
            dont_override_serdes: bit(r2, 0),
            force_1000_base_x: bit(r2, 1),
            an_disable: bit(r2, 2),
            speed: Speed1g(get(r2, 4, 3)),
            half_duplex: bit(r2, 5),
            fc_disable: bit(r2, 6),
            retimer_exist: bit(r2, 7),
            retimer_bus_id: get(r2, 11, 8),
            retimer_i2c_addr: get(r2, 18, 12),
            retimer_channel: get(r2, 19, 19) | (get(r2, 28, 27) << 1),
            dac: bit(r2, 24),
            dac_len: get(r2, 23, 20),
            retimer_type: RetimerType(get(r2, 26, 25)),
            retimer_tx_channel: get(r2, 31, 29),
            gpio_sfp_present: get(r3, 5, 0),
            auto_fec_enable: bit(r3, 6),
            gpio_spd_1g_10g: get(r3, 12, 7),
            gpio_spd_25g: get(r3, 18, 13),
        })
    }

    /// The register values; None when register 1 would be 0, which reads back
    /// as "unset" (the C function asserts on it).
    pub const fn encode(&self) -> Option<Regs> {
        let r1 = put(self.media_type.0, 3, 0)
            | flag(self.phy_exist, 4)
            | put(self.phy_mdio_addr, 9, 5)
            | flag(self.sfp_plus_module_exist, 10)
            | flag(self.autoneg_enable, 11)
            | flag(self.kr_lt_enable, 12)
            | flag(self.kr_fec_enable, 13)
            | put(self.mdio_freq.0, 15, 14)
            | put(self.i2c_adapter_id, 19, 16)
            | put(self.phy_if.0, 21, 20)
            | flag(self.an_in_band, 22)
            | put(self.serdes_grp, 26, 25)
            | put(self.serdes_grp >> 2, 23, 23)
            | put(self.serdes_lane, 28, 27)
            | put(self.ref_clk_freq.0, 31, 29);
        if r1 == 0 {
            return None;
        }
        let r2 = flag(self.dont_override_serdes, 0)
            | flag(self.force_1000_base_x, 1)
            | flag(self.an_disable, 2)
            | put(self.speed.0, 4, 3)
            | flag(self.half_duplex, 5)
            | flag(self.fc_disable, 6)
            | flag(self.retimer_exist, 7)
            | put(self.retimer_bus_id, 11, 8)
            | put(self.retimer_i2c_addr, 18, 12)
            | put(self.retimer_channel, 19, 19)
            | put(self.retimer_channel >> 1, 28, 27)
            | put(self.dac_len, 23, 20)
            | flag(self.dac, 24)
            | put(self.retimer_type.0, 26, 25)
            | put(self.retimer_tx_channel, 31, 29);
        let r3 = put(self.gpio_sfp_present, 5, 0)
            | flag(self.auto_fec_enable, 6)
            | put(self.gpio_spd_1g_10g, 12, 7)
            | put(self.gpio_spd_25g, 18, 13);
        Some([r1, r2, r3])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fields as numbers, in the order of struct al_eth_board_params (the
    /// order of the C generator's lines, ../board-params-vectors.c).
    fn fields(p: &BoardParams) -> [u32; 32] {
        [
            p.media_type.0.into(),
            p.phy_exist.into(),
            p.phy_mdio_addr.into(),
            p.sfp_plus_module_exist.into(),
            p.autoneg_enable.into(),
            p.kr_lt_enable.into(),
            p.kr_fec_enable.into(),
            p.mdio_freq.0.into(),
            p.i2c_adapter_id.into(),
            p.phy_if.0.into(),
            p.an_in_band.into(),
            p.serdes_grp.into(),
            p.serdes_lane.into(),
            p.ref_clk_freq.0.into(),
            p.dont_override_serdes.into(),
            p.force_1000_base_x.into(),
            p.an_disable.into(),
            p.speed.0.into(),
            p.half_duplex.into(),
            p.fc_disable.into(),
            p.retimer_exist.into(),
            p.retimer_bus_id.into(),
            p.retimer_i2c_addr.into(),
            p.retimer_channel.into(),
            p.dac.into(),
            p.dac_len.into(),
            p.retimer_type.0.into(),
            p.retimer_tx_channel.into(),
            p.gpio_sfp_present.into(),
            p.auto_fec_enable.into(),
            p.gpio_spd_1g_10g.into(),
            p.gpio_spd_25g.into(),
        ]
    }

    fn from_fields(f: &[u32]) -> BoardParams {
        let n = |i: usize| u8::try_from(f[i]).expect("field fits a u8");
        let b = |i: usize| match f[i] {
            0 => false,
            1 => true,
            v => panic!("field {i}: {v} is not a bool"),
        };
        BoardParams {
            media_type: MediaType(n(0)),
            phy_exist: b(1),
            phy_mdio_addr: n(2),
            sfp_plus_module_exist: b(3),
            autoneg_enable: b(4),
            kr_lt_enable: b(5),
            kr_fec_enable: b(6),
            mdio_freq: MdioFreq(n(7)),
            i2c_adapter_id: n(8),
            phy_if: PhyIf(n(9)),
            an_in_band: b(10),
            serdes_grp: n(11),
            serdes_lane: n(12),
            ref_clk_freq: RefClkFreq(n(13)),
            dont_override_serdes: b(14),
            force_1000_base_x: b(15),
            an_disable: b(16),
            speed: Speed1g(n(17)),
            half_duplex: b(18),
            fc_disable: b(19),
            retimer_exist: b(20),
            retimer_bus_id: n(21),
            retimer_i2c_addr: n(22),
            retimer_channel: n(23),
            dac: b(24),
            dac_len: n(25),
            retimer_type: RetimerType(n(26)),
            retimer_tx_channel: n(27),
            gpio_sfp_present: n(28),
            auto_fec_enable: b(29),
            gpio_spd_1g_10g: n(30),
            gpio_spd_25g: n(31),
        }
    }

    /// What our U-Boot writes for the DMC-12's port (uboot patch 0023; decoded
    /// by the C driver on u01 as RGMII, no PHY, 1000 Mb/s fixed).
    #[test]
    fn dmc12_port() {
        let p = BoardParams::decode([0x6000_0001, 0x0000_0004, 0]).unwrap();
        let want = BoardParams {
            media_type: MediaType::RGMII,
            ref_clk_freq: RefClkFreq::MHZ_500,
            an_disable: true,
            speed: Speed1g::M1000,
            ..BoardParams::default()
        };
        assert_eq!(p, want);
        assert_eq!(want.encode(), Some([0x6000_0001, 0x0000_0004, 0]));
    }

    #[test]
    fn unset_registers() {
        assert_eq!(BoardParams::decode([0, 0xffff_ffff, 0xffff_ffff]), None);
        assert_eq!(BoardParams::default().encode(), None);
        // Only a reserved bit set: initialised, every field zero.
        assert_eq!(BoardParams::decode([1 << 24, 0, 0]), Some(BoardParams::default()));
    }

    #[test]
    fn split_fields() {
        for grp in 0..8u8 {
            for ch in 0..8u8 {
                let p = BoardParams {
                    media_type: MediaType::SGMII,
                    serdes_grp: grp,
                    retimer_channel: ch,
                    ..BoardParams::default()
                };
                assert_eq!(BoardParams::decode(p.encode().unwrap()), Some(p));
            }
        }
    }

    #[test]
    fn every_bit_is_one_field() {
        // Each defined register bit changes the decoded value, and encoding the
        // result gives that bit back; reserved bits (r1 bit 24, r3 19..31) do neither.
        let base = [1u32, 0, 0];
        let base_p = BoardParams::decode(base).unwrap();
        for r in 0..3 {
            for n in 0..32 {
                let mut regs = base;
                regs[r] ^= 1 << n;
                if regs[0] == 0 {
                    continue;
                }
                let p = BoardParams::decode(regs).unwrap();
                let reserved = (r == 0 && n == 24) || (r == 2 && n >= 19);
                assert_eq!(p == base_p, reserved, "register {r} bit {n}");
                if !reserved {
                    assert_eq!(p.encode(), Some(regs), "register {r} bit {n}");
                }
            }
        }
    }

    #[test]
    fn known_values() {
        assert!(MediaType::FORTY_G.is_known() && !MediaType(11).is_known());
        assert!(RefClkFreq::MHZ_428.is_known() && !RefClkFreq(5).is_known());
        assert!(!RetimerType(3).is_known() && !PhyIf(3).is_known() && !Speed1g(3).is_known());
        assert!(MdioFreq::MHZ_1.is_known() && !MdioFreq(2).is_known());
    }

    /// Against the C functions: the lines ../board-params-vectors.c printed
    /// (path in AL_ETH_C_VECTORS; ../rust.nix sets it and requires this test).
    ///   D r1 r2 r3 rc f0..f31     al_eth_board_params_get() of the registers
    ///   E f0..f31 r1 r2 r3        al_eth_board_params_set() of the fields
    #[test]
    fn same_as_the_c_functions() {
        let Ok(path) = std::env::var("AL_ETH_C_VECTORS") else {
            eprintln!("AL_ETH_C_VECTORS not set: comparison with the C skipped");
            return;
        };
        let text = std::fs::read_to_string(path).unwrap();
        let (mut decoded, mut encoded) = (0, 0);
        for line in text.lines() {
            let mut words = line.split_whitespace();
            let kind = words.next().unwrap();
            let v: Vec<i64> = words.map(|w| w.parse().unwrap()).collect();
            let u = |i: usize| u32::try_from(v[i]).unwrap();
            match kind {
                "D" => {
                    let regs = [u(0), u(1), u(2)];
                    let got = BoardParams::decode(regs);
                    if v[3] != 0 {
                        assert_eq!(got, None, "{line}");
                    } else {
                        let want: Vec<u32> = (4..36).map(u).collect();
                        assert_eq!(fields(&got.unwrap())[..], want[..], "{line}");
                    }
                    decoded += 1;
                }
                "E" => {
                    let f: Vec<u32> = (0..32).map(u).collect();
                    let got = from_fields(&f).encode();
                    assert_eq!(got, Some([u(32), u(33), u(34)]), "{line}");
                    encoded += 1;
                }
                _ => panic!("unknown line: {line}"),
            }
        }
        assert!(decoded >= 100_000 && encoded >= 100_000, "{decoded} D, {encoded} E lines");
        println!("same as the C functions: {decoded} decodes, {encoded} encodes");
    }
}
