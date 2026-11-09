//! Plain-text rendering of analysis reports.

use ripplefix_core::{AnalysisReport, Change, ChangeKind, Confidence, FileStatus};
use std::fmt::Write;

pub fn human(report: &AnalysisReport) -> String {
    let mut out = String::new();
    let short = &report.base_commit[..report.base_commit.len().min(12)];
    let _ = writeln!(out, "RippleFix analysis against {short}\n");

    if report.files.is_empty() {
        let _ = writeln!(out, "No changed files handled by an analyzer.");
    }
    for f in &report.files {
        let path = f
            .delta
            .new_path
            .as_deref()
            .or(f.delta.old_path.as_deref())
            .unwrap_or("?");
        match &f.status {
            FileStatus::Cosmetic => {
                let _ = writeln!(out, "  {path}  (no structural change)");
            }
            FileStatus::Unanalyzable { reason } => {
                let _ = writeln!(out, "  {path}  NOT ANALYZED: {reason}");
            }
            FileStatus::Structural => {
                let _ = writeln!(out, "  {path}");
                for c in &f.changes {
                    let _ = writeln!(out, "    {}", line(c));
                    for e in &c.evidence {
                        let loc = match (&e.file, e.span) {
                            (Some(file), Some(s)) => format!("{file}:{}", s.start_line),
                            _ => String::new(),
                        };
                        let _ = writeln!(out, "        evidence: {} {loc}", e.description);
                    }
                }
            }
        }
    }

    let total = report.changes().count();
    let breaking = report.changes().filter(|c| c.breaks_wire_contract).count();
    let inferred = report
        .changes()
        .filter(|c| c.confidence != Confidence::Confirmed)
        .count();
    let _ = writeln!(
        out,
        "\n{total} structural change(s), {breaking} affecting the wire contract, {inferred} not fully confirmed. {} changed file(s) not supported by any analyzer.",
        report.unsupported_files
    );
    out
}

fn line(c: &Change) -> String {
    let what = match &c.kind {
        ChangeKind::ModelAdded => "model added".to_owned(),
        ChangeKind::ModelRemoved => "model removed".to_owned(),
        ChangeKind::BasesChanged { old, new } => {
            format!(
                "bases changed: ({}) -> ({})",
                old.join(", "),
                new.join(", ")
            )
        }
        ChangeKind::FieldAdded { field } => format!("field added: {field}"),
        ChangeKind::FieldRemoved { field } => format!("field removed: {field}"),
        ChangeKind::FieldRenamed { old, new } => format!("field renamed: {old} -> {new}"),
        ChangeKind::FieldTypeChanged { field, old, new } => {
            format!("field type changed: {field}: {old} -> {new}")
        }
        ChangeKind::FieldDefaultChanged { field, old, new } => format!(
            "field default changed: {field}: {} -> {}",
            old.as_deref().unwrap_or("<required>"),
            new.as_deref().unwrap_or("<required>")
        ),
        ChangeKind::WireNameChanged { field, old, new } => format!(
            "wire name changed: {field}: {} -> {}",
            old.as_deref().unwrap_or("<unresolved>"),
            new.as_deref().unwrap_or("<unresolved>")
        ),
    };
    let conf = match c.confidence {
        Confidence::Confirmed => "confirmed",
        Confidence::Inferred => "INFERRED",
        Confidence::Unresolved => "UNRESOLVED",
    };
    let brk = if c.breaks_wire_contract {
        " [breaking]"
    } else {
        ""
    };
    format!("{}: {what} ({conf}){brk}", c.model)
}
