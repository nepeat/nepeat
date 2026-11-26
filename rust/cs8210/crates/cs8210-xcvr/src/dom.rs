//! Module monitors in one shape for CMIS and SFF-8636 (what the daemon caches, serves and
//! exports to Prometheus).

use serde::Serialize;

/// One lane's monitors and flags (None: not supported or not read).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Lane {
    /// 1-based lane (media lane for optics)
    pub lane: u8,
    /// Tx optical power (mW)
    pub tx_power_mw: Option<f64>,
    /// Rx optical power (mW)
    pub rx_power_mw: Option<f64>,
    /// Tx bias current (mA)
    pub tx_bias_ma: Option<f64>,
    /// Rx loss of signal (latched flag)
    pub rx_los: Option<bool>,
    /// Tx fault (latched flag)
    pub tx_fault: Option<bool>,
    /// Tx CDR loss of lock (latched flag)
    pub tx_cdr_lol: Option<bool>,
    /// Rx CDR loss of lock (latched flag)
    pub rx_cdr_lol: Option<bool>,
}

/// Module-level and lane monitors.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct Monitors {
    /// module case temperature (deg C)
    pub temperature_c: Option<f64>,
    /// supply voltage (V)
    pub voltage_v: Option<f64>,
    /// lanes
    pub lanes: Vec<Lane>,
}

/// Milliwatts as dBm (None for zero or negative power).
#[must_use]
pub fn dbm(mw: f64) -> Option<f64> {
    (mw > 0.0).then(|| 10.0 * mw.log10())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dbm_values() {
        assert!((dbm(1.0).unwrap()).abs() < 1e-12);
        assert!((dbm(0.1).unwrap() + 10.0).abs() < 1e-12);
        assert_eq!(dbm(0.0), None);
    }
}
