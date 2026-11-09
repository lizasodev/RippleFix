//! TypeScript consumer analyzer for RippleFix.
//!
//! Extracts, syntactically and without type checking:
//! * object-shaped types: `interface X { ... }` and `type X = { ... }` with
//!   their property names;
//! * property accesses (`v.prop`, `v?.prop`, `v["prop"]`) and object-literal
//!   keys, with locations;
//! * type names referenced in the file.
//!
//! Because there is no type checker, links from accesses to types are never
//! reported as confirmed by consumers of this crate.

use ripplefix_core::model::Span;
use ripplefix_core::plugin::AnalysisError;
use serde::Serialize;
use std::collections::BTreeSet;
use std::path::Path;
use tree_sitter::{Node, Parser};

#[derive(Debug, Clone, Serialize)]
pub struct TsProperty {
    pub name: String,
    pub optional: bool,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize)]
pub struct TsType {
    pub name: String,
    pub properties: Vec<TsProperty>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageKind {
    PropertyAccess,
    ObjectKey,
}

#[derive(Debug, Clone, Serialize)]
pub struct TsUsage {
    pub name: String,
    pub kind: UsageKind,
    pub span: Span,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct TsFile {
    pub types: Vec<TsType>,
    pub usages: Vec<TsUsage>,
    /// Every type name referenced (e.g. in annotations, `as` casts, generics).
    pub type_refs: BTreeSet<String>,
}

pub fn is_typescript(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("ts" | "tsx" | "mts" | "cts")
    )
}

/// Parse a TypeScript (or TSX, when `tsx` is true) file.
pub fn parse(source: &str, tsx: bool) -> Result<TsFile, AnalysisError> {
    let mut parser = Parser::new();
    let lang = if tsx {
        tree_sitter_typescript::LANGUAGE_TSX
    } else {
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT
    };
    parser
        .set_language(&lang.into())
        .map_err(|e| AnalysisError::Parser(e.to_string()))?;
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| AnalysisError::Parser("parse cancelled".into()))?;
    let root = tree.root_node();
    if root.has_error() {
        let mut lines = Vec::new();
        error_lines(root, &mut lines);
        return Err(AnalysisError::SyntaxError { lines });
    }
    let mut out = TsFile::default();
    walk(root, source.as_bytes(), &mut out);
    Ok(out)
}

pub fn parse_path(path: &Path, source: &str) -> Result<TsFile, AnalysisError> {
    parse(source, path.extension().is_some_and(|e| e == "tsx"))
}

fn text<'a>(n: Node, src: &'a [u8]) -> &'a str {
    n.utf8_text(src).unwrap_or_default()
}

fn span(n: Node) -> Span {
    Span {
        start_line: n.start_position().row + 1,
        end_line: n.end_position().row + 1,
    }
}

fn error_lines(n: Node, out: &mut Vec<usize>) {
    if out.len() >= 10 {
        return;
    }
    if n.is_error() || n.is_missing() {
        out.push(n.start_position().row + 1);
        return;
    }
    let mut cur = n.walk();
    for c in n.children(&mut cur) {
        if c.has_error() {
            error_lines(c, out);
        }
    }
}

/// Text of a property name node (identifier or plain string literal).
fn prop_name(n: Node, src: &[u8]) -> Option<String> {
    match n.kind() {
        "property_identifier" | "identifier" => Some(text(n, src).to_owned()),
        "string" => {
            let mut cur = n.walk();
            let parts: Vec<_> = n.named_children(&mut cur).collect();
            match parts.as_slice() {
                [] => Some(String::new()),
                [frag] if frag.kind() == "string_fragment" => Some(text(*frag, src).to_owned()),
                _ => None,
            }
        }
        _ => None,
    }
}

fn object_props(body: Node, src: &[u8]) -> Vec<TsProperty> {
    let mut cur = body.walk();
    body.named_children(&mut cur)
        .filter(|c| c.kind() == "property_signature")
        .filter_map(|p| {
            let name = prop_name(p.child_by_field_name("name")?, src)?;
            let mut c = p.walk();
            let optional = p.children(&mut c).any(|x| x.kind() == "?");
            Some(TsProperty {
                name,
                optional,
                span: span(p),
            })
        })
        .collect()
}

fn walk(n: Node, src: &[u8], out: &mut TsFile) {
    match n.kind() {
        "interface_declaration" => {
            if let (Some(name), Some(body)) =
                (n.child_by_field_name("name"), n.child_by_field_name("body"))
            {
                out.types.push(TsType {
                    name: text(name, src).to_owned(),
                    properties: object_props(body, src),
                    span: span(n),
                });
            }
        }
        "type_alias_declaration" => {
            if let (Some(name), Some(value)) = (
                n.child_by_field_name("name"),
                n.child_by_field_name("value"),
            ) {
                if value.kind() == "object_type" {
                    out.types.push(TsType {
                        name: text(name, src).to_owned(),
                        properties: object_props(value, src),
                        span: span(n),
                    });
                }
            }
        }
        "member_expression" => {
            if let Some(p) = n.child_by_field_name("property") {
                if p.kind() == "property_identifier" {
                    out.usages.push(TsUsage {
                        name: text(p, src).to_owned(),
                        kind: UsageKind::PropertyAccess,
                        span: span(p),
                    });
                }
            }
        }
        "subscript_expression" => {
            if let Some(name) = n
                .child_by_field_name("index")
                .and_then(|i| (i.kind() == "string").then(|| prop_name(i, src)).flatten())
            {
                out.usages.push(TsUsage {
                    name,
                    kind: UsageKind::PropertyAccess,
                    span: span(n),
                });
            }
        }
        "pair" => {
            if let Some(name) = n.child_by_field_name("key").and_then(|k| prop_name(k, src)) {
                out.usages.push(TsUsage {
                    name,
                    kind: UsageKind::ObjectKey,
                    span: span(n),
                });
            }
        }
        "shorthand_property_identifier" => {
            out.usages.push(TsUsage {
                name: text(n, src).to_owned(),
                kind: UsageKind::ObjectKey,
                span: span(n),
            });
        }
        "type_identifier" => {
            out.type_refs.insert(text(n, src).to_owned());
        }
        _ => {}
    }
    let mut cur = n.walk();
    for c in n.named_children(&mut cur) {
        walk(c, src, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_types_usages_and_refs() {
        let f = parse(
            "export interface UserRead {\n  id: number;\n  full_name: string;\n  'created-at'?: string;\n}\ntype Small = { a: number };\ntype Alias = string;\nconst u = x as UserRead;\nconsole.log(u.full_name, u?.id, u['created-at']);\nconst o = { full_name: 'x', id };\n",
            false,
        )
        .unwrap();
        assert_eq!(f.types.len(), 2);
        let names: Vec<_> = f.types[0]
            .properties
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(names, ["id", "full_name", "created-at"]);
        assert!(f.types[0].properties[2].optional);
        assert!(f.type_refs.contains("UserRead"));
        let accesses: Vec<_> = f
            .usages
            .iter()
            .filter(|u| u.kind == UsageKind::PropertyAccess)
            .map(|u| u.name.as_str())
            .collect();
        assert_eq!(accesses, ["log", "full_name", "id", "created-at"]);
        let keys: Vec<_> = f
            .usages
            .iter()
            .filter(|u| u.kind == UsageKind::ObjectKey)
            .map(|u| (u.name.as_str(), u.span.start_line))
            .collect();
        assert_eq!(keys, [("full_name", 10), ("id", 10)]);
    }

    #[test]
    fn tsx_is_supported_and_errors_are_reported() {
        assert!(parse("const a = <div>{u.full_name}</div>;", true).is_ok());
        assert!(matches!(
            parse("interface X { a: ", false),
            Err(AnalysisError::SyntaxError { .. })
        ));
    }
}
