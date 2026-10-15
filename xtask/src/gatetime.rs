//! `gatetime`: the wall time of the gated test steps, as medians of
//! several runs on prebuilt binaries, so a phase can be required to leave
//! them no slower than it found them.
//!
//! `--record` runs the steps for HEAD and writes
//! `gatetime/<hash>.gatetime.tsv`, one `step`, median seconds and the runs
//! in order per line. `--since <hash>` records HEAD the same way and fails
//! on any step whose median is not lower than `<hash>`'s.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::Instant;

use crate::{cargo, gate_dir, git, head_hash, require_clean_tree, run, write};

/// Runs of each step; the median of them is the step's time.
const RUNS: usize = 3;

struct Step {
    name: &'static str,
    /// The arguments of `cargo test`.
    args: &'static [&'static str],
    /// The fixture suite runs with each fixture's type dump on, as `gates`
    /// runs it.
    dump_types: bool,
}

const STEPS: [Step; 2] = [
    Step {
        name: "lib tests",
        args: &["--workspace", "--lib", "--quiet"],
        dump_types: false,
    },
    Step {
        name: "fixtures",
        args: &["--profile", "gate", "--test", "fixtures", "--quiet"],
        dump_types: true,
    },
];

fn tsv_path(hash: &str) -> PathBuf {
    gate_dir()
        .join("gatetime")
        .join(format!("{hash}.gatetime.tsv"))
}

pub(crate) fn gatetime(args: &[String]) -> Result<(), String> {
    let since = match args {
        [flag] if flag == "--record" => None,
        [flag, hash] if flag == "--since" => Some(git(&["rev-parse", hash])?),
        _ => return Err("usage: cargo xtask gatetime (--record | --since <hash>)".to_string()),
    };
    require_clean_tree()?;
    let hash = head_hash()?;
    let measured = measure()?;
    write(&tsv_path(&hash), format_tsv(&measured).as_bytes())?;
    let Some(base) = since else {
        return Ok(());
    };
    let text = std::fs::read_to_string(tsv_path(&base))
        .map_err(|e| format!("no gate times recorded for {base}: {e}"))?;
    let slower = not_lower(&measured, &parse_tsv(&text)?);
    if slower.is_empty() {
        eprintln!("every gated step is lower than at {base}");
        Ok(())
    } else {
        Err(slower.join("\n"))
    }
}

/// Builds every step's binaries, then times each step `RUNS` times.
fn measure() -> Result<Vec<(&'static str, Vec<f64>)>, String> {
    for step in &STEPS {
        run(cargo().arg("test").args(step.args).arg("--no-run"))?;
    }
    let dumps = gate_dir().join(".gatetime-types.tmp");
    let mut measured = Vec::new();
    for step in &STEPS {
        let mut runs = Vec::new();
        for _ in 0..RUNS {
            let _ = std::fs::remove_dir_all(&dumps);
            std::fs::create_dir_all(&dumps).map_err(|e| e.to_string())?;
            let mut cmd = cargo();
            cmd.arg("test").args(step.args);
            if step.dump_types {
                cmd.env("PHPSTAN_RS_DUMP_TYPES", &dumps);
            }
            let started = Instant::now();
            run(&mut cmd)?;
            let secs = started.elapsed().as_secs_f64();
            eprintln!("[gatetime] {}: {secs:.1}s", step.name);
            runs.push(secs);
        }
        measured.push((step.name, runs));
    }
    let _ = std::fs::remove_dir_all(&dumps);
    Ok(measured)
}

fn median(runs: &[f64]) -> f64 {
    let mut sorted = runs.to_vec();
    sorted.sort_by(f64::total_cmp);
    sorted[sorted.len() / 2]
}

fn format_tsv(measured: &[(&str, Vec<f64>)]) -> String {
    let mut tsv = String::new();
    for (name, runs) in measured {
        let shown: Vec<String> = runs.iter().map(|r| format!("{r:.1}")).collect();
        writeln!(tsv, "{name}\t{:.1}\t{}", median(runs), shown.join(",")).unwrap();
    }
    tsv
}

/// The step names and medians of a recorded file.
fn parse_tsv(text: &str) -> Result<Vec<(String, f64)>, String> {
    text.lines()
        .map(|line| {
            let mut fields = line.split('\t');
            let name = fields.next().unwrap_or_default();
            let median = fields
                .next()
                .and_then(|m| m.parse().ok())
                .ok_or_else(|| format!("bad gate time line: {line}"))?;
            Ok((name.to_string(), median))
        })
        .collect()
}

/// A line for each of `measured`'s steps whose median is not lower than
/// the base's, or that the base lacks.
fn not_lower(measured: &[(&str, Vec<f64>)], base: &[(String, f64)]) -> Vec<String> {
    let mut slower = Vec::new();
    for (name, runs) in measured {
        let now = median(runs);
        match base.iter().find(|(n, _)| n == name) {
            Some((_, then)) if now < *then => {}
            Some((_, then)) => {
                slower.push(format!("{name}: {now:.1}s is not lower than {then:.1}s"))
            }
            None => slower.push(format!("{name}: no time recorded to compare with")),
        }
    }
    slower
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_median_is_the_middle_run() {
        assert_eq!(median(&[3.0, 1.0, 2.0]), 2.0);
        assert_eq!(median(&[5.0]), 5.0);
    }

    #[test]
    fn a_recorded_file_reads_back_its_medians() {
        let tsv = format_tsv(&[
            ("lib tests", vec![9.0, 7.0, 8.0]),
            ("fixtures", vec![4.0, 4.5, 5.0]),
        ]);
        assert_eq!(
            tsv,
            "lib tests\t8.0\t9.0,7.0,8.0\nfixtures\t4.5\t4.0,4.5,5.0\n"
        );
        assert_eq!(
            parse_tsv(&tsv).unwrap(),
            vec![
                ("lib tests".to_string(), 8.0),
                ("fixtures".to_string(), 4.5)
            ]
        );
    }

    #[test]
    fn a_step_must_be_lower_than_the_base() {
        let base = vec![
            ("lib tests".to_string(), 8.0),
            ("fixtures".to_string(), 4.5),
        ];
        let faster = [
            ("lib tests", vec![7.0, 7.0, 7.0]),
            ("fixtures", vec![4.0, 4.0, 4.0]),
        ];
        assert!(not_lower(&faster, &base).is_empty());
        let equal = [
            ("lib tests", vec![8.0, 8.0, 8.0]),
            ("fixtures", vec![4.0, 4.0, 4.0]),
        ];
        assert_eq!(not_lower(&equal, &base).len(), 1);
        let missing = [("new step", vec![1.0, 1.0, 1.0])];
        assert_eq!(not_lower(&missing, &base).len(), 1);
    }

    #[test]
    fn a_malformed_line_is_an_error() {
        assert!(parse_tsv("lib tests\tfast\t1.0").is_err());
    }
}
