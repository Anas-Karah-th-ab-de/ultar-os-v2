//! Export of a log directory for support, with a redaction report.
//!
//! Records of services contain no personal data by construction (see
//! `ultar_service::log::record`), so they are exported as they are. The
//! lines of the **kernel** are free text; they are left out unless the
//! person who exports asks for them, and the report says how many lines
//! were withheld. The export states whether the chain verified.

use crate::chain::Chain;
use crate::core::to_bus;
use crate::format::format_record;
use crate::segment::{parse_segment_name, scan};
use crate::storage::Storage;
use crate::verify::{verify, Report};
use std::collections::BTreeMap;
use std::io::Write;

/// What to export.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ExportOptions {
    /// Include the kernel's own lines.
    pub include_kernel_text: bool,
}

/// What an export contains and what it left out.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct ExportReport {
    /// Records written.
    pub records: u64,
    /// Records per source name.
    pub sources: BTreeMap<String, u64>,
    /// Kernel records whose text was withheld.
    pub kernel_text_redacted: u64,
    /// Kernel records written with their text.
    pub kernel_text_included: u64,
    /// Result of the chain verification.
    pub chain: Report,
    /// Whether the chain was verified with a key.
    pub keyed: bool,
}

fn header(out: &mut dyn Write, report: &ExportReport) -> std::io::Result<()> {
    writeln!(out, "# ultar log export v1")?;
    writeln!(
        out,
        "# chain: {} ({})",
        if report.chain.is_intact() {
            "intact"
        } else {
            "NOT INTACT"
        },
        if report.keyed { "keyed" } else { "unkeyed" }
    )?;
    for problem in &report.chain.problems {
        writeln!(out, "# problem: {}", problem.key())?;
    }
    for (first, last) in &report.chain.declared_gaps {
        writeln!(out, "# never written to disk: {first}-{last}")?;
    }
    writeln!(out, "# seq\ttime\tsource\tname\tlevel\tkey\tfields\ttext")
}

fn trailer(out: &mut dyn Write, report: &ExportReport) -> std::io::Result<()> {
    writeln!(out, "# redaction report")?;
    writeln!(out, "# records: {}", report.records)?;
    writeln!(
        out,
        "# kernel text withheld: {}",
        report.kernel_text_redacted
    )?;
    writeln!(
        out,
        "# kernel text included: {}",
        report.kernel_text_included
    )?;
    writeln!(
        out,
        "# service records carry keys, numbers and short identifiers only (enforced by type)"
    )?;
    for (name, count) in &report.sources {
        writeln!(out, "# source {name}: {count}")?;
    }
    Ok(())
}

/// Writes the log in `storage` to `out` and returns what was written.
/// Segments are processed one at a time; memory use is bounded by
/// `max_segment_bytes`.
pub fn export(
    storage: &dyn Storage,
    chain: &Chain,
    options: ExportOptions,
    max_segment_bytes: u64,
    out: &mut dyn Write,
) -> std::io::Result<ExportReport> {
    let mut report = ExportReport {
        chain: verify(storage, chain, max_segment_bytes),
        keyed: chain.is_keyed(),
        ..ExportReport::default()
    };
    header(out, &report)?;
    let mut names: Vec<(u64, String)> = storage
        .list()?
        .into_iter()
        .filter_map(|(name, _)| parse_segment_name(&name).map(|base| (base, name)))
        .collect();
    names.sort();
    for (_, name) in names {
        let Ok(data) = storage.read(&name, max_segment_bytes) else {
            continue;
        };
        for record in scan(chain, &data).records {
            // Free text (kernel lines, output of services) is withheld
            // unless asked for.
            let is_kernel = record.kind.has_text();
            let with_text = is_kernel && options.include_kernel_text;
            if is_kernel && with_text {
                report.kernel_text_included += 1;
            } else if is_kernel {
                report.kernel_text_redacted += 1;
            }
            report.records += 1;
            *report.sources.entry(record.service.clone()).or_insert(0) += 1;
            writeln!(out, "{}", format_record(&to_bus(record, with_text)))?;
        }
    }
    trailer(out, &report)?;
    Ok(report)
}
