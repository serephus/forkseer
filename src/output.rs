use std::io::Write;

use anyhow::Result;
use serde::Serialize;

use crate::scan::{RepoMatch, ScanReport};

/// Renders a scan report as either JSON (stdout) or human-readable text.
///
/// Matches are written to `out`; diagnostics are written to `err` so that
/// `--json` output on stdout stays machine-readable.
pub fn render(
    json: bool,
    report: &ScanReport,
    out: &mut impl Write,
    err: &mut impl Write,
) -> Result<()> {
    if json {
        render_json(report, out)
    } else {
        render_text(report, out, err)
    }
}

fn render_json(report: &ScanReport, out: &mut impl Write) -> Result<()> {
    #[derive(Serialize)]
    struct JsonOutput<'a> {
        matches: &'a [RepoMatch],
        failures: &'a [String],
    }

    writeln!(
        out,
        "{}",
        serde_json::to_string_pretty(&JsonOutput {
            matches: &report.matches,
            failures: &report.failures,
        })?
    )?;

    Ok(())
}

fn render_text(report: &ScanReport, out: &mut impl Write, err: &mut impl Write) -> Result<()> {
    for repo_match in &report.matches {
        writeln!(out, "{} [{}]", repo_match.full_name, repo_match.branch)?;
        writeln!(out, "  {}", repo_match.html_url)?;
        writeln!(out, "  - {}", repo_match.path)?;
    }

    if !report.failures.is_empty() {
        writeln!(err, "failures:")?;
        for failure in &report.failures {
            writeln!(err, "  - {failure}")?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_report() -> ScanReport {
        ScanReport {
            matches: vec![RepoMatch {
                full_name: "owner/fork".to_string(),
                branch: "main".to_string(),
                html_url: "https://github.com/owner/fork".to_string(),
                path: "flake.nix".to_string(),
            }],
            failures: vec!["boom".to_string()],
        }
    }

    #[test]
    fn text_output_lists_matches_and_failures() {
        let mut out = Vec::new();
        let mut err = Vec::new();

        render(false, &sample_report(), &mut out, &mut err).unwrap();

        let out = String::from_utf8(out).unwrap();
        let err = String::from_utf8(err).unwrap();
        assert!(out.contains("owner/fork [main]"));
        assert!(out.contains("https://github.com/owner/fork"));
        assert!(out.contains("  - flake.nix"));
        assert!(err.contains("failures:"));
        assert!(err.contains("boom"));
    }

    #[test]
    fn json_output_is_valid_and_machine_readable() {
        let mut out = Vec::new();
        let mut err = Vec::new();

        render(true, &sample_report(), &mut out, &mut err).unwrap();

        assert!(err.is_empty());
        let value: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(value["matches"][0]["full_name"], "owner/fork");
        assert_eq!(
            value["matches"][0]["html_url"],
            "https://github.com/owner/fork"
        );
        assert_eq!(value["failures"][0], "boom");
    }
}
