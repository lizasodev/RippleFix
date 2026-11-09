//! Project index and evidence-backed impact analysis.
//!
//! Given the structural changes found by `ripplefix-core`, this crate finds
//! the components they affect:
//!
//! | Relationship | How it is established | Confidence |
//! |---|---|---|
//! | Subclass models (any file) | base name resolved through `import` statements | confirmed |
//! | FastAPI routes | `response_model=` / return annotation / typed parameter resolved to an affected model | confirmed |
//! | TypeScript object types | every property is a JSON key of the model's *old* shape, includes the changed key, has at least 2 properties | inferred |
//! | TypeScript property uses | old key used in a file that declares or references a matched type | inferred |
//! | Other TypeScript uses | old key used elsewhere | unresolved |
//! | Python tests | an HTTP call matching an affected route plus a string literal equal to the old key | inferred |
//!
//! Name resolution never guesses: an import that matches several files, or no
//! file, simply produces no edge.

mod graph;
mod index;

pub use graph::{GraphEdge, GraphExport, GraphNode, NodeKind};
pub use index::{ClassId, ProjectIndex, SourceFile};

use graph::GraphBuilder;
use ripplefix_core::{AnalysisReport, Change, ChangeKind, Confidence, Evidence, Span};
use ripplefix_python::HttpCall;
use serde::Serialize;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    Model,
    Route,
    TypescriptType,
    TypescriptUsage,
    Test,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImpactItem {
    pub kind: ItemKind,
    pub label: String,
    pub file: String,
    pub span: Span,
    pub confidence: Confidence,
    pub evidence: Vec<Evidence>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChangeImpact {
    pub file: String,
    pub change: Change,
    /// Limitations that apply to this result (e.g. unresolved keys).
    pub notes: Vec<String>,
    pub items: Vec<ImpactItem>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImpactReport {
    pub indexed_files: usize,
    /// Files that could not be parsed and were therefore not searched.
    pub unparsed_files: Vec<(String, String)>,
    pub impacts: Vec<ChangeImpact>,
    pub graph: GraphExport,
}

/// Field-level changes whose consumers are searched.
fn field_name(c: &Change) -> Option<&str> {
    match &c.kind {
        ChangeKind::FieldRenamed { new, .. } => Some(new),
        ChangeKind::FieldRemoved { field }
        | ChangeKind::FieldTypeChanged { field, .. }
        | ChangeKind::WireNameChanged { field, .. } => Some(field),
        _ => None,
    }
}

pub fn compute_impact(index: &ProjectIndex, report: &AnalysisReport) -> ImpactReport {
    let mut graph = GraphBuilder::default();
    let mut impacts = Vec::new();
    for file in &report.files {
        let Some(path) = file
            .delta
            .new_path
            .as_deref()
            .or(file.delta.old_path.as_deref())
        else {
            continue;
        };
        for change in &file.changes {
            if !change.breaks_wire_contract {
                continue;
            }
            let Some(field) = field_name(change) else {
                continue;
            };
            impacts.push(change_impact(index, path, change, field, &mut graph));
        }
    }
    ImpactReport {
        indexed_files: index.file_count(),
        unparsed_files: index.failures(),
        impacts,
        graph: graph.export(),
    }
}

fn change_impact(
    index: &ProjectIndex,
    path: &str,
    change: &Change,
    field: &str,
    graph: &mut GraphBuilder,
) -> ChangeImpact {
    let mut out = ChangeImpact {
        file: path.to_owned(),
        change: change.clone(),
        notes: Vec::new(),
        items: Vec::new(),
    };
    let change_node = graph.node(
        format!("change:{path}:{}:{field}", change.model),
        NodeKind::Change,
        format!("{}.{field}", change.model),
        Some(path),
        change.evidence.first().and_then(|e| e.span),
    );
    let Some(root) = index.find_model(path, &change.model) else {
        out.notes.push(format!(
            "model `{}` not found in the project index of the working tree; impact not computed",
            change.model
        ));
        return out;
    };
    let Some(old_key) = change.old_key.clone() else {
        out.notes.push(
            "the previous JSON key is not statically known (dynamic alias or alias generator); consumers were not searched"
                .into(),
        );
        return out;
    };
    let new_key = change.new_key.clone();

    // 1. Affected models: the changed model plus subclasses that do not redeclare the field.
    let mut affected = vec![root];
    for (sub, chain) in index.subclasses(root) {
        if index.declares_field(sub, field) {
            continue;
        }
        let (file, span) = index.location(sub);
        out.items.push(ImpactItem {
            kind: ItemKind::Model,
            label: index.class_name(sub).to_owned(),
            file: file.to_owned(),
            span,
            confidence: Confidence::Confirmed,
            evidence: chain.clone(),
        });
        affected.push(sub);
    }
    let model_nodes: Vec<_> = affected
        .iter()
        .map(|&m| {
            let (f, s) = index.location(m);
            let n = graph.node(
                model_id(index, m),
                NodeKind::Model,
                index.class_name(m),
                Some(f),
                Some(s),
            );
            (m, n)
        })
        .collect();
    graph.edge(
        change_node,
        model_nodes[0].1,
        "changes",
        Confidence::Confirmed,
        change.evidence.clone(),
    );
    for (sub, chain) in index.subclasses(root) {
        if let (Some(a), Some(b)) = (
            model_nodes.iter().find(|(m, _)| *m == root),
            model_nodes.iter().find(|(m, _)| *m == sub),
        ) {
            graph.edge(a.1, b.1, "inherited_by", Confidence::Confirmed, chain);
        }
    }

    // 2. FastAPI routes referencing an affected model.
    let mut affected_routes = Vec::new();
    for (file, routes) in index.routes() {
        for r in routes {
            for (role, names) in [
                ("response", &r.response_names),
                ("request", &r.request_names),
            ] {
                for name in names {
                    let Some((target, mut ev)) = index.resolve_name(file, name) else {
                        continue;
                    };
                    let Some(&(m, mnode)) = model_nodes.iter().find(|(m, _)| *m == target) else {
                        continue;
                    };
                    let label = format!(
                        "{} {} ({})",
                        r.method.to_uppercase(),
                        r.path.as_deref().unwrap_or("<dynamic path>"),
                        r.function
                    );
                    ev.insert(
                        0,
                        Evidence::new(format!(
                            "route uses `{name}` as its {role} type, which resolves to model `{}`",
                            index.class_name(m)
                        ))
                        .at(file, r.span),
                    );
                    let rn = graph.node(
                        format!("route:{file}:{}", r.function),
                        NodeKind::Route,
                        &label,
                        Some(file),
                        Some(r.span),
                    );
                    graph.edge(mnode, rn, role, Confidence::Confirmed, ev.clone());
                    out.items.push(ImpactItem {
                        kind: ItemKind::Route,
                        label,
                        file: file.to_owned(),
                        span: r.span,
                        confidence: Confidence::Confirmed,
                        evidence: ev,
                    });
                    affected_routes.push((r.method.clone(), r.path.clone(), rn));
                }
            }
        }
    }
    if affected_routes.is_empty() {
        out.notes
            .push("no FastAPI route was found that references the affected model(s)".into());
    }

    // 3. TypeScript types mirroring an affected model's old shape.
    let mut matched_types: BTreeSet<String> = BTreeSet::new();
    for &(m, mnode) in &model_nodes {
        let (keys, complete) = index.wire_keys(m);
        let mut old_shape: BTreeSet<String> = keys.into_iter().collect();
        if let Some(nk) = &new_key {
            old_shape.remove(nk);
        }
        old_shape.insert(old_key.clone());
        if !complete {
            out.notes.push(format!(
                "model `{}` has fields with unresolved JSON keys; TypeScript matching may miss consumers",
                index.class_name(m)
            ));
        }
        for (file, ts) in index.typescript() {
            for t in &ts.types {
                let props: BTreeSet<&str> = t.properties.iter().map(|p| p.name.as_str()).collect();
                if props.len() < 2
                    || !props.contains(old_key.as_str())
                    || !props.iter().all(|p| old_shape.contains(*p))
                {
                    continue;
                }
                let ev = vec![Evidence::new(format!(
                    "all {} properties of `{}` are JSON keys of `{}` before the change, including `{old_key}`",
                    props.len(),
                    t.name,
                    index.class_name(m)
                ))
                .at(file, t.span)];
                let tn = graph.node(
                    format!("ts:{file}:{}", t.name),
                    NodeKind::TypescriptType,
                    &t.name,
                    Some(file),
                    Some(t.span),
                );
                graph.edge(mnode, tn, "mirrored_by", Confidence::Inferred, ev.clone());
                matched_types.insert(t.name.clone());
                out.items.push(ImpactItem {
                    kind: ItemKind::TypescriptType,
                    label: t.name.clone(),
                    file: file.to_owned(),
                    span: t.span,
                    confidence: Confidence::Inferred,
                    evidence: ev,
                });
            }
        }
    }

    // 4. TypeScript uses of the old key.
    for (file, ts) in index.typescript() {
        let linked: Vec<&String> = matched_types
            .iter()
            .filter(|t| ts.type_refs.contains(*t) || ts.types.iter().any(|d| &d.name == *t))
            .collect();
        for u in ts.usages.iter().filter(|u| u.name == old_key) {
            let (confidence, why) = match linked.first() {
                Some(t) => (
                    Confidence::Inferred,
                    format!("`{old_key}` used in a file that references matched type `{t}`"),
                ),
                None => (
                    Confidence::Unresolved,
                    format!("`{old_key}` used, but no link to an affected type was found"),
                ),
            };
            let ev = vec![Evidence::new(why).at(file, u.span)];
            let un = graph.node(
                format!("use:{file}:{}", u.span.start_line),
                NodeKind::TypescriptUsage,
                format!("{file}:{}", u.span.start_line),
                Some(file),
                Some(u.span),
            );
            graph.edge(change_node, un, "uses_key", confidence, ev.clone());
            out.items.push(ImpactItem {
                kind: ItemKind::TypescriptUsage,
                label: format!("{old_key} ({:?})", u.kind).to_lowercase(),
                file: file.to_owned(),
                span: u.span,
                confidence,
                evidence: ev,
            });
        }
    }

    // 5. Python tests calling an affected route and mentioning the old key.
    for (file, module) in index.python() {
        if !is_test_path(file) {
            continue;
        }
        let literals: Vec<&Span> = module
            .strings
            .iter()
            .filter(|(s, _)| *s == old_key)
            .map(|(_, sp)| sp)
            .collect();
        if literals.is_empty() {
            continue;
        }
        for call in &module.http_calls {
            let Some((_, _, rn)) = affected_routes
                .iter()
                .find(|(m, p, _)| route_matches(m, p.as_deref(), call))
            else {
                continue;
            };
            let mut ev = vec![
                Evidence::new(format!(
                    "test calls {} {}",
                    call.method.to_uppercase(),
                    call.url
                ))
                .at(file, call.span),
            ];
            ev.extend(
                literals.iter().map(|sp| {
                    Evidence::new(format!("string literal \"{old_key}\"")).at(file, **sp)
                }),
            );
            let tn = graph.node(
                format!("test:{file}:{}", call.span.start_line),
                NodeKind::Test,
                file,
                Some(file),
                Some(call.span),
            );
            graph.edge(*rn, tn, "exercised_by", Confidence::Inferred, ev.clone());
            out.items.push(ImpactItem {
                kind: ItemKind::Test,
                label: format!("{file}:{}", call.span.start_line),
                file: file.to_owned(),
                span: call.span,
                confidence: Confidence::Inferred,
                evidence: ev,
            });
        }
    }
    out
}

fn model_id(index: &ProjectIndex, m: ClassId) -> String {
    format!("model:{}:{}", index.location(m).0, index.class_name(m))
}

fn is_test_path(path: &str) -> bool {
    let file = path.rsplit('/').next().unwrap_or(path);
    path.split('/').any(|s| s == "tests" || s == "test")
        || file.starts_with("test_")
        || file.ends_with("_test.py")
}

/// Route template `/users/{id}` matches `/users/1` and `/users/{}`.
fn route_matches(method: &str, template: Option<&str>, call: &HttpCall) -> bool {
    let Some(template) = template else {
        return false;
    };
    if method != "api_route" && method != call.method {
        return false;
    }
    let url = call.url.split(['?', '#']).next().unwrap_or_default();
    let a: Vec<_> = template.trim_end_matches('/').split('/').collect();
    let b: Vec<_> = url.trim_end_matches('/').split('/').collect();
    a.len() == b.len()
        && a.iter()
            .zip(&b)
            .all(|(x, y)| x == y || (x.starts_with('{') && x.ends_with('}') && !y.is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(method: &str, url: &str) -> HttpCall {
        HttpCall {
            method: method.into(),
            url: url.into(),
            span: Span {
                start_line: 1,
                end_line: 1,
            },
        }
    }

    #[test]
    fn route_template_matching() {
        assert!(route_matches(
            "get",
            Some("/users/{id}"),
            &call("get", "/users/1")
        ));
        assert!(route_matches(
            "get",
            Some("/users/{id}"),
            &call("get", "/users/{}?x=1")
        ));
        assert!(!route_matches(
            "post",
            Some("/users/{id}"),
            &call("get", "/users/1")
        ));
        assert!(!route_matches(
            "get",
            Some("/users"),
            &call("get", "/users/1")
        ));
        assert!(!route_matches("get", None, &call("get", "/users")));
    }

    #[test]
    fn test_path_detection() {
        assert!(is_test_path("backend/tests/test_users.py"));
        assert!(is_test_path("users_test.py"));
        assert!(!is_test_path("backend/app/contest.py"));
    }
}
