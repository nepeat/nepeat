//! Prometheus text exposition, families grouped (the textfile collector refuses a family
//! that reappears after another).

use std::fmt::Write as _;

struct Family {
    name: String,
    help: String,
    kind: &'static str,
    samples: Vec<String>,
}

/// Collects samples, writes them grouped by family in first-seen order.
#[derive(Default)]
pub struct Writer {
    families: Vec<Family>,
}

fn escape(v: &str) -> String {
    v.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n")
}

impl Writer {
    fn add(&mut self, kind: &'static str, name: &str, help: &str, labels: &[(&str, &str)], value: f64) {
        let mut s = String::from(name);
        if !labels.is_empty() {
            s.push('{');
            for (i, (k, v)) in labels.iter().enumerate() {
                let _ = write!(s, "{}{k}=\"{}\"", if i > 0 { "," } else { "" }, escape(v));
            }
            s.push('}');
        }
        let _ = write!(s, " {value}");
        if let Some(f) = self.families.iter_mut().find(|f| f.name == name) {
            f.samples.push(s);
        } else {
            self.families.push(Family {
                name: name.into(),
                help: help.into(),
                kind,
                samples: vec![s],
            });
        }
    }

    /// A gauge sample.
    pub fn gauge(&mut self, name: &str, help: &str, labels: &[(&str, &str)], value: f64) {
        self.add("gauge", name, help, labels, value);
    }

    /// A counter sample.
    pub fn counter(&mut self, name: &str, help: &str, labels: &[(&str, &str)], value: f64) {
        self.add("counter", name, help, labels, value);
    }

    /// The text.
    #[must_use]
    pub fn finish(self) -> String {
        let mut out = String::new();
        for f in self.families {
            let _ = writeln!(out, "# HELP {} {}", f.name, f.help);
            let _ = writeln!(out, "# TYPE {} {}", f.name, f.kind);
            for s in f.samples {
                out.push_str(&s);
                out.push('\n');
            }
        }
        out
    }
}

/// The Credo PHY die temperatures as metrics, from the vendor command
/// `firmware_get_slice_temp slice_id=all` (`phy_shell`, answered by `phy_server`, the MDIO
/// FPGA's one owner): lines `slice N temp= T degress`, slice N = cage N (1-16). None when
/// no line parses.
#[must_use]
pub fn credo_temps(text: &str) -> Option<String> {
    let mut w = Writer::default();
    let mut n = 0;
    for line in text.lines() {
        let Some(rest) = line.trim().strip_prefix("slice ") else {
            continue;
        };
        let Some((slice, t)) = rest.split_once(" temp=") else {
            continue;
        };
        let (Ok(slice), Some(Ok(t))) = (
            slice.trim().parse::<u8>(),
            t.split_whitespace().next().map(str::parse::<f64>),
        ) else {
            continue;
        };
        if !(1..=16).contains(&slice) || !(-40.0..=150.0).contains(&t) {
            continue;
        }
        let s = slice.to_string();
        w.gauge(
            "cs8210_credo_temperature_celsius",
            "Credo CMS50216 MACsec PHY die temperature (firmware slice temperature); slice = cage.",
            &[("cage", &s)],
            t,
        );
        n += 1;
    }
    (n > 0).then(|| w.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credo_lines() {
        let t = credo_temps(
            "slice 1 temp= 51.46 degress\nslice 16 temp= 46.88 degress\njunk\nslice 17 temp= 1.0 degress\n",
        )
        .unwrap();
        assert!(t.contains("cs8210_credo_temperature_celsius{cage=\"1\"} 51.46\n"));
        assert!(t.contains("{cage=\"16\"} 46.88"));
        assert!(!t.contains("cage=\"17\""));
        assert_eq!(credo_temps("phy_server is not running"), None);
    }

    #[test]
    fn grouped_and_escaped() {
        let mut w = Writer::default();
        w.gauge("a", "A.", &[("port", "1")], 1.0);
        w.gauge("b", "B.", &[], 2.5);
        w.gauge("a", "A.", &[("port", "2"), ("x", "q\"uote")], 0.0);
        let t = w.finish();
        assert_eq!(
            t,
            "# HELP a A.\n# TYPE a gauge\na{port=\"1\"} 1\na{port=\"2\",x=\"q\\\"uote\"} 0\n# HELP b B.\n# TYPE b gauge\nb 2.5\n"
        );
    }
}
