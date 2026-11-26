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

#[cfg(test)]
mod tests {
    use super::*;

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
