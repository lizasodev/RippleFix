//! Deterministic structural diff between two [`FileSnapshot`]s.
//!
//! Rename detection rule (documented in `docs/architecture.md`): within one
//! model, a removed field and an added field are reported as an **inferred**
//! rename only when *exactly one* field was removed, *exactly one* was added,
//! both have identical annotation and default, and both occupy the same
//! ordinal position. Anything else is reported as separate removal/addition.

use crate::model::{Confidence, DataModel, Evidence, Field, FileSnapshot};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChangeKind {
    ModelAdded,
    ModelRemoved,
    BasesChanged {
        old: Vec<String>,
        new: Vec<String>,
    },
    FieldAdded {
        field: String,
    },
    FieldRemoved {
        field: String,
    },
    FieldRenamed {
        old: String,
        new: String,
    },
    FieldTypeChanged {
        field: String,
        old: String,
        new: String,
    },
    FieldDefaultChanged {
        field: String,
        old: Option<String>,
        new: Option<String>,
    },
    /// The JSON key of a field changed (alias added, removed, or edited).
    WireNameChanged {
        field: String,
        old: Option<String>,
        new: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Change {
    pub model: String,
    #[serde(flatten)]
    pub kind: ChangeKind,
    pub confidence: Confidence,
    /// Whether the change alters the serialized API contract.
    pub breaks_wire_contract: bool,
    /// JSON key of the affected field before the change, when statically known.
    pub old_key: Option<String>,
    /// JSON key of the affected field after the change, when statically known.
    pub new_key: Option<String>,
    pub evidence: Vec<Evidence>,
}

/// Compute structural changes for one file. `old_path`/`new_path` are used
/// only to annotate evidence.
pub fn diff_snapshots(
    old: &FileSnapshot,
    new: &FileSnapshot,
    old_path: &str,
    new_path: &str,
) -> Vec<Change> {
    let mut changes = Vec::new();
    for om in &old.models {
        match new.model(&om.qualified_name) {
            None => changes.push(Change {
                model: om.qualified_name.clone(),
                kind: ChangeKind::ModelRemoved,
                confidence: Confidence::Confirmed,
                breaks_wire_contract: true,
                evidence: vec![
                    Evidence::new(format!("model `{}` no longer present", om.qualified_name))
                        .at(old_path, om.span),
                ],
                old_key: None,
                new_key: None,
            }),
            Some(nm) => diff_model(om, nm, old_path, new_path, &mut changes),
        }
    }
    for nm in &new.models {
        if old.model(&nm.qualified_name).is_none() {
            changes.push(Change {
                model: nm.qualified_name.clone(),
                kind: ChangeKind::ModelAdded,
                confidence: Confidence::Confirmed,
                breaks_wire_contract: false,
                evidence: vec![nm.evidence.clone()],
                old_key: None,
                new_key: None,
            });
        }
    }
    for c in &mut changes {
        fill_keys(c, old, new);
    }
    changes
}

/// Attach the old/new JSON keys of the field a change refers to.
fn fill_keys(c: &mut Change, old: &FileSnapshot, new: &FileSnapshot) {
    let key = |snap: &FileSnapshot, field: &str| {
        snap.model(&c.model)
            .and_then(|m| m.field(field))
            .and_then(|f| f.wire_name().map(str::to_owned))
    };
    let (o, n) = match &c.kind {
        ChangeKind::FieldRenamed { old: of, new: nf } => (key(old, of), key(new, nf)),
        ChangeKind::FieldRemoved { field } => (key(old, field), None),
        ChangeKind::FieldAdded { field } => (None, key(new, field)),
        ChangeKind::FieldTypeChanged { field, .. }
        | ChangeKind::FieldDefaultChanged { field, .. } => (key(old, field), key(new, field)),
        ChangeKind::WireNameChanged { old, new, .. } => (old.clone(), new.clone()),
        _ => (None, None),
    };
    c.old_key = o;
    c.new_key = n;
}

fn diff_model(om: &DataModel, nm: &DataModel, op: &str, np: &str, out: &mut Vec<Change>) {
    let model = &om.qualified_name;
    let mut push = |kind, confidence, breaks, evidence| {
        out.push(Change {
            model: model.clone(),
            kind,
            confidence,
            breaks_wire_contract: breaks,
            old_key: None,
            new_key: None,
            evidence,
        })
    };

    if om.bases != nm.bases {
        push(
            ChangeKind::BasesChanged {
                old: om.bases.clone(),
                new: nm.bases.clone(),
            },
            Confidence::Confirmed,
            // Inherited fields may change; that cannot be disproven from one file.
            true,
            vec![Evidence::new("base class list changed").at(np, nm.span)],
        );
    }

    let removed: Vec<(usize, &Field)> = om
        .fields
        .iter()
        .enumerate()
        .filter(|(_, f)| nm.field(&f.name).is_none())
        .collect();
    let added: Vec<(usize, &Field)> = nm
        .fields
        .iter()
        .enumerate()
        .filter(|(_, f)| om.field(&f.name).is_none())
        .collect();

    let rename = match (removed.as_slice(), added.as_slice()) {
        ([(oi, of)], [(ni, nf)])
            if oi == ni && of.annotation == nf.annotation && of.default == nf.default =>
        {
            Some((*of, *nf))
        }
        _ => None,
    };

    if let Some((of, nf)) = rename {
        let wire_changed =
            of.wire_name() != nf.wire_name() || of.alias_unresolved || nf.alias_unresolved;
        push(
            ChangeKind::FieldRenamed {
                old: of.name.clone(),
                new: nf.name.clone(),
            },
            Confidence::Inferred,
            wire_changed,
            vec![
                Evidence::new(format!("field `{}` removed", of.name)).at(op, of.span),
                Evidence::new(format!(
                    "field `{}` added at the same position with identical type `{}` and default",
                    nf.name, nf.annotation
                ))
                .at(np, nf.span),
            ],
        );
    } else {
        for (_, f) in &removed {
            push(
                ChangeKind::FieldRemoved {
                    field: f.name.clone(),
                },
                Confidence::Confirmed,
                true,
                vec![Evidence::new(format!("field `{}` removed", f.name)).at(op, f.span)],
            );
        }
        for (_, f) in &added {
            // A new required field (no default) breaks producers of the model.
            push(
                ChangeKind::FieldAdded {
                    field: f.name.clone(),
                },
                Confidence::Confirmed,
                f.default.is_none(),
                vec![Evidence::new(format!("field `{}` added", f.name)).at(np, f.span)],
            );
        }
    }

    for of in &om.fields {
        let Some(nf) = nm.field(&of.name) else {
            continue;
        };
        let ev = |d: String| vec![Evidence::new(d).at(np, nf.span)];
        if of.annotation != nf.annotation {
            push(
                ChangeKind::FieldTypeChanged {
                    field: of.name.clone(),
                    old: of.annotation.clone(),
                    new: nf.annotation.clone(),
                },
                Confidence::Confirmed,
                true,
                ev(format!(
                    "annotation `{}` -> `{}`",
                    of.annotation, nf.annotation
                )),
            );
        }
        if of.default != nf.default {
            push(
                ChangeKind::FieldDefaultChanged {
                    field: of.name.clone(),
                    old: of.default.clone(),
                    new: nf.default.clone(),
                },
                Confidence::Confirmed,
                // Becoming required breaks producers that omitted the field.
                of.default.is_some() && nf.default.is_none(),
                ev("default value changed".into()),
            );
        }
        if of.wire_name() != nf.wire_name() || of.alias_unresolved != nf.alias_unresolved {
            let confidence = if nf.alias_unresolved || of.alias_unresolved {
                Confidence::Unresolved
            } else {
                Confidence::Confirmed
            };
            push(
                ChangeKind::WireNameChanged {
                    field: of.name.clone(),
                    old: of.wire_name().map(str::to_owned),
                    new: nf.wire_name().map(str::to_owned),
                },
                confidence,
                true,
                ev("serialization alias changed".into()),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ModelKind, Span};

    fn field(name: &str, ann: &str, idx: usize) -> Field {
        Field {
            name: name.into(),
            annotation: ann.into(),
            default: None,
            alias: None,
            alias_unresolved: false,
            span: Span {
                start_line: idx + 2,
                end_line: idx + 2,
            },
        }
    }

    fn model(fields: Vec<Field>) -> FileSnapshot {
        FileSnapshot {
            models: vec![DataModel {
                kind: ModelKind::PydanticModel,
                qualified_name: "User".into(),
                bases: vec!["BaseModel".into()],
                fields,
                span: Span {
                    start_line: 1,
                    end_line: 5,
                },
                evidence: Evidence::new("test"),
            }],
        }
    }

    #[test]
    fn identical_snapshots_produce_no_changes() {
        let s = model(vec![field("id", "int", 0)]);
        assert!(diff_snapshots(&s, &s, "a.py", "a.py").is_empty());
    }

    #[test]
    fn moved_lines_are_not_changes() {
        let a = model(vec![field("id", "int", 0)]);
        let mut b = a.clone();
        b.models[0].fields[0].span = Span {
            start_line: 40,
            end_line: 40,
        };
        assert!(diff_snapshots(&a, &b, "a.py", "a.py").is_empty());
    }

    #[test]
    fn unambiguous_rename_is_inferred_not_confirmed() {
        let a = model(vec![field("id", "int", 0), field("full_name", "str", 1)]);
        let b = model(vec![field("id", "int", 0), field("display_name", "str", 1)]);
        let c = diff_snapshots(&a, &b, "a.py", "a.py");
        assert_eq!(c.len(), 1);
        assert_eq!(
            c[0].kind,
            ChangeKind::FieldRenamed {
                old: "full_name".into(),
                new: "display_name".into()
            }
        );
        assert_eq!(c[0].confidence, Confidence::Inferred);
        assert!(c[0].breaks_wire_contract);
    }

    #[test]
    fn rename_preserving_alias_does_not_break_wire_contract() {
        let a = model(vec![field("full_name", "str", 0)]);
        let mut nf = field("display_name", "str", 0);
        nf.alias = Some("full_name".into());
        let b = model(vec![nf]);
        let c = diff_snapshots(&a, &b, "a.py", "a.py");
        assert_eq!(c.len(), 1);
        assert!(!c[0].breaks_wire_contract);
    }

    #[test]
    fn type_mismatch_prevents_rename_inference() {
        let a = model(vec![field("full_name", "str", 0)]);
        let b = model(vec![field("display_name", "int", 0)]);
        let kinds: Vec<_> = diff_snapshots(&a, &b, "a.py", "a.py")
            .into_iter()
            .map(|c| c.kind)
            .collect();
        assert_eq!(
            kinds,
            vec![
                ChangeKind::FieldRemoved {
                    field: "full_name".into()
                },
                ChangeKind::FieldAdded {
                    field: "display_name".into()
                },
            ]
        );
    }

    #[test]
    fn ambiguous_multi_rename_is_not_inferred() {
        let a = model(vec![field("a", "str", 0), field("b", "str", 1)]);
        let b = model(vec![field("c", "str", 0), field("d", "str", 1)]);
        let c = diff_snapshots(&a, &b, "a.py", "a.py");
        assert!(c.iter().all(|c| c.confidence == Confidence::Confirmed));
        assert_eq!(c.len(), 4);
    }

    #[test]
    fn type_change_is_confirmed_and_breaking() {
        let a = model(vec![field("id", "int", 0)]);
        let b = model(vec![field("id", "str", 0)]);
        let c = diff_snapshots(&a, &b, "a.py", "a.py");
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].confidence, Confidence::Confirmed);
        assert!(c[0].breaks_wire_contract);
    }
}
