//! Python / Pydantic analyzer for RippleFix.
//!
//! Classification rules (all syntactic and deterministic):
//! * A class is a Pydantic model if a base is `BaseModel` imported from
//!   `pydantic` (including `as` aliases), `pydantic.BaseModel` with
//!   `import pydantic`, or another model class defined in the same file.
//! * Fields are annotated class-level assignments, excluding private
//!   (`_x`) names, `model_config`, and `ClassVar` annotations.
//! * A field wire name is resolved only from string-literal `alias=` /
//!   `serialization_alias=` arguments to `Field(...)`. A class configuring an
//!   `alias_generator` marks fields without a literal alias as unresolved.
//!
//! Known limitations: fields inherited from models in *other* files are not
//! merged, and alias generators inherited from base classes are not detected.

use ripplefix_core::model::{DataModel, Evidence, Field, FileSnapshot, ModelKind, Span};
use ripplefix_core::plugin::{AnalysisError, LanguageAnalyzer, PLUGIN_API_VERSION};
use std::collections::HashSet;
use std::path::Path;
use tree_sitter::{Node, Parser};

#[derive(Debug, Default, Clone, Copy)]
pub struct PydanticAnalyzer;

impl LanguageAnalyzer for PydanticAnalyzer {
    fn id(&self) -> &'static str {
        "python-pydantic"
    }

    fn api_version(&self) -> u32 {
        PLUGIN_API_VERSION
    }

    fn handles(&self, path: &Path) -> bool {
        matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("py" | "pyi")
        )
    }

    fn extract(&self, source: &str) -> Result<FileSnapshot, AnalysisError> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .map_err(|e| AnalysisError::Parser(e.to_string()))?;
        let tree = parser
            .parse(source, None)
            .ok_or_else(|| AnalysisError::Parser("parse cancelled".into()))?;
        let root = tree.root_node();
        if root.has_error() {
            let mut lines = Vec::new();
            collect_error_lines(root, &mut lines);
            return Err(AnalysisError::SyntaxError { lines });
        }
        let src = source.as_bytes();
        let imports = Imports::collect(root, src);

        let mut classes = Vec::new();
        collect_classes(root, src, "", &mut classes);

        // Fixpoint: classes inheriting from same-file models are models too.
        let mut model_names: HashSet<String> = HashSet::new();
        let mut reasons: Vec<Option<String>> = vec![None; classes.len()];
        loop {
            let mut grew = false;
            for (i, c) in classes.iter().enumerate() {
                if reasons[i].is_some() {
                    continue;
                }
                if let Some(r) = c
                    .bases
                    .iter()
                    .find_map(|b| imports.model_reason(b, &model_names))
                {
                    reasons[i] = Some(r);
                    model_names.insert(c.simple_name.clone());
                    grew = true;
                }
            }
            if !grew {
                break;
            }
        }

        let models = classes
            .into_iter()
            .zip(reasons)
            .filter_map(|(c, reason)| {
                let reason = reason?;
                Some(DataModel {
                    kind: ModelKind::PydanticModel,
                    evidence: Evidence::new(format!("class `{}` {reason}", c.qualified_name)),
                    qualified_name: c.qualified_name,
                    bases: c.bases,
                    fields: extract_fields(c.body, src, &imports),
                    span: span(c.node),
                })
            })
            .collect();
        Ok(FileSnapshot { models })
    }
}

struct ClassInfo<'t> {
    node: Node<'t>,
    body: Node<'t>,
    simple_name: String,
    qualified_name: String,
    bases: Vec<String>,
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

fn collect_error_lines(n: Node, out: &mut Vec<usize>) {
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
            collect_error_lines(c, out);
        }
    }
}

/// Recursively find classes; nesting inside functions is skipped because such
/// classes are not importable API contracts.
fn collect_classes<'t>(n: Node<'t>, src: &[u8], prefix: &str, out: &mut Vec<ClassInfo<'t>>) {
    let mut cur = n.walk();
    for child in n.named_children(&mut cur) {
        match child.kind() {
            "class_definition" => {
                let (Some(name), Some(body)) = (
                    child.child_by_field_name("name"),
                    child.child_by_field_name("body"),
                ) else {
                    continue;
                };
                let simple = text(name, src).to_owned();
                let qualified = if prefix.is_empty() {
                    simple.clone()
                } else {
                    format!("{prefix}.{simple}")
                };
                let bases = child
                    .child_by_field_name("superclasses")
                    .map(|args| {
                        let mut c = args.walk();
                        args.named_children(&mut c)
                            .filter(|a| a.kind() != "keyword_argument" && a.kind() != "comment")
                            .map(|a| normalize(a, src))
                            .collect()
                    })
                    .unwrap_or_default();
                collect_classes(body, src, &qualified, out);
                out.push(ClassInfo {
                    node: child,
                    body,
                    simple_name: simple,
                    qualified_name: qualified,
                    bases,
                });
            }
            "function_definition" | "lambda" => {}
            _ => collect_classes(child, src, prefix, out),
        }
    }
}

/// Names under which `pydantic`, `pydantic.BaseModel`, and `pydantic.Field`
/// are visible in a module.
#[derive(Default)]
struct Imports {
    base_model: HashSet<String>,
    field_fn: HashSet<String>,
    module: HashSet<String>,
}

impl Imports {
    fn collect(root: Node, src: &[u8]) -> Self {
        let mut imp = Imports::default();
        let mut cur = root.walk();
        for stmt in root.named_children(&mut cur) {
            match stmt.kind() {
                "import_from_statement" => {
                    let module = stmt
                        .child_by_field_name("module_name")
                        .map(|m| text(m, src));
                    if module != Some("pydantic") {
                        continue;
                    }
                    let mut c = stmt.walk();
                    for name in stmt.children_by_field_name("name", &mut c) {
                        let (orig, local) = imported_name(name, src);
                        match orig {
                            "BaseModel" => imp.base_model.insert(local.to_owned()),
                            "Field" => imp.field_fn.insert(local.to_owned()),
                            _ => false,
                        };
                    }
                }
                "import_statement" => {
                    let mut c = stmt.walk();
                    for name in stmt.children_by_field_name("name", &mut c) {
                        let (orig, local) = imported_name(name, src);
                        if orig == "pydantic" {
                            imp.module.insert(local.to_owned());
                        }
                    }
                }
                _ => {}
            }
        }
        imp
    }

    fn model_reason(&self, base: &str, same_file_models: &HashSet<String>) -> Option<String> {
        if self.base_model.contains(base) {
            return Some(format!("inherits `{base}` imported from `pydantic`"));
        }
        if let Some((module, "BaseModel")) = base.rsplit_once('.') {
            if self.module.contains(module) {
                return Some(format!("inherits `{base}`"));
            }
        }
        if same_file_models.contains(base) {
            return Some(format!(
                "inherits Pydantic model `{base}` defined in the same file"
            ));
        }
        None
    }

    fn is_field_call(&self, func: &str) -> bool {
        self.field_fn.contains(func)
            || func
                .rsplit_once('.')
                .is_some_and(|(m, f)| f == "Field" && self.module.contains(m))
    }
}

fn imported_name<'a>(n: Node, src: &'a [u8]) -> (&'a str, &'a str) {
    if n.kind() == "aliased_import" {
        let orig = n
            .child_by_field_name("name")
            .map(|x| text(x, src))
            .unwrap_or_default();
        let alias = n
            .child_by_field_name("alias")
            .map(|x| text(x, src))
            .unwrap_or(orig);
        (orig, alias)
    } else {
        let t = text(n, src);
        (t, t)
    }
}

fn extract_fields(body: Node, src: &[u8], imports: &Imports) -> Vec<Field> {
    let has_alias_generator = class_has_alias_generator(body, src);
    let mut fields = Vec::new();
    let mut cur = body.walk();
    for stmt in body.named_children(&mut cur) {
        if stmt.kind() != "expression_statement" {
            continue;
        }
        let Some(assign) = stmt.named_child(0).filter(|n| n.kind() == "assignment") else {
            continue;
        };
        let (Some(left), Some(ty)) = (
            assign.child_by_field_name("left"),
            assign.child_by_field_name("type"),
        ) else {
            continue;
        };
        if left.kind() != "identifier" {
            continue;
        }
        let name = text(left, src);
        let annotation = normalize(ty, src);
        if name.starts_with('_')
            || name == "model_config"
            || annotation.starts_with("ClassVar")
            || annotation.contains(".ClassVar")
        {
            continue;
        }
        let right = assign.child_by_field_name("right");
        let (alias, explicit_dynamic_alias) = right
            .map(|r| field_alias(r, src, imports))
            .unwrap_or((None, false));
        let alias_unresolved = explicit_dynamic_alias || (has_alias_generator && alias.is_none());
        fields.push(Field {
            name: name.to_owned(),
            annotation,
            default: right.map(|r| normalize(r, src)),
            alias,
            alias_unresolved,
            span: span(stmt),
        });
    }
    fields
}

/// Returns `(literal_alias, alias_is_dynamic)` for a `Field(...)` call.
fn field_alias(right: Node, src: &[u8], imports: &Imports) -> (Option<String>, bool) {
    if right.kind() != "call" {
        return (None, false);
    }
    let Some(func) = right.child_by_field_name("function") else {
        return (None, false);
    };
    if !imports.is_field_call(text(func, src)) {
        return (None, false);
    }
    let Some(args) = right.child_by_field_name("arguments") else {
        return (None, false);
    };
    let mut alias = None;
    let mut serialization = None;
    let mut cur = args.walk();
    for kw in args.named_children(&mut cur) {
        if kw.kind() == "dictionary_splat" {
            return (None, true); // **kwargs may set an alias we cannot see
        }
        if kw.kind() != "keyword_argument" {
            continue;
        }
        let key = kw
            .child_by_field_name("name")
            .map(|n| text(n, src))
            .unwrap_or_default();
        let Some(value) = kw.child_by_field_name("value") else {
            continue;
        };
        let slot = match key {
            "alias" => &mut alias,
            "serialization_alias" => &mut serialization,
            _ => continue,
        };
        *slot = Some(string_literal(value, src));
    }
    // serialization_alias takes precedence over alias for output.
    match serialization.or(alias) {
        None => (None, false),
        Some(Some(s)) => (Some(s), false),
        Some(None) => (None, true),
    }
}

/// Value of a plain string literal; `None` for f-strings, concatenations, names.
fn string_literal(n: Node, src: &[u8]) -> Option<String> {
    if n.kind() != "string" {
        return None;
    }
    let mut cur = n.walk();
    let mut content = String::new();
    for c in n.named_children(&mut cur) {
        match c.kind() {
            "string_start" | "string_end" => {}
            "string_content" => content.push_str(text(c, src)),
            _ => return None, // interpolation or escape sequence
        }
    }
    let prefix = text(n, src)
        .chars()
        .take_while(|ch| ch.is_ascii_alphabetic())
        .collect::<String>();
    if prefix.to_ascii_lowercase().contains('f') {
        return None;
    }
    Some(content)
}

fn class_has_alias_generator(body: Node, src: &[u8]) -> bool {
    let mut cur = body.walk();
    body.named_children(&mut cur).any(|stmt| {
        let t = text(stmt, src);
        let is_config = (stmt.kind() == "expression_statement"
            && t.trim_start().starts_with("model_config"))
            || (stmt.kind() == "class_definition"
                && stmt
                    .child_by_field_name("name")
                    .is_some_and(|n| text(n, src) == "Config"));
        is_config && t.contains("alias_generator")
    })
}

/// Token-level normalization: comments dropped, whitespace collapsed, so that
/// `Optional[ str ]` and `Optional[str]` compare equal. String literals are
/// kept verbatim.
fn normalize(n: Node, src: &[u8]) -> String {
    fn walk(n: Node, src: &[u8], out: &mut String) {
        if n.kind() == "comment" {
            return;
        }
        if n.child_count() == 0 || n.kind() == "string" {
            let t = text(n, src);
            let word = |c: char| c.is_alphanumeric() || c == '_' || c == '"' || c == '\'';
            if out.chars().last().is_some_and(word) && t.chars().next().is_some_and(word) {
                out.push(' ');
            }
            out.push_str(t);
            return;
        }
        let mut cur = n.walk();
        for c in n.children(&mut cur) {
            walk(c, src, out);
        }
    }
    let mut s = String::new();
    walk(n, src, &mut s);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extract(src: &str) -> FileSnapshot {
        PydanticAnalyzer.extract(src).expect("parse")
    }

    #[test]
    fn extracts_basic_model() {
        let s = extract(
            "from pydantic import BaseModel, Field\n\nclass User(BaseModel):\n    id: int\n    full_name: str = Field(alias='fullName')\n    tags: list[ str ] = []\n    _secret: str\n    def method(self): pass\n",
        );
        assert_eq!(s.models.len(), 1);
        let m = &s.models[0];
        assert_eq!(m.qualified_name, "User");
        let names: Vec<_> = m.fields.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["id", "full_name", "tags"]);
        assert_eq!(m.fields[1].wire_name(), Some("fullName"));
        assert_eq!(m.fields[2].annotation, "list[str]");
        assert_eq!(
            m.fields[0].span,
            Span {
                start_line: 4,
                end_line: 4
            }
        );
    }

    #[test]
    fn ignores_classes_without_pydantic_import() {
        let s = extract("class BaseModel: pass\nclass User(BaseModel):\n    id: int\n");
        assert!(s.models.is_empty());
    }

    #[test]
    fn handles_aliases_module_imports_and_inheritance() {
        let s = extract(
            "import pydantic as pd\nfrom pydantic import BaseModel as BM\nclass A(pd.BaseModel):\n    a: int\nclass B(BM):\n    b: int\nclass C(A):\n    c: int\n    class Inner(B):\n        x: int\n",
        );
        let names: Vec<_> = s.models.iter().map(|m| m.qualified_name.as_str()).collect();
        assert_eq!(names, ["A", "B", "C.Inner", "C"]);
    }

    #[test]
    fn serialization_alias_wins_and_dynamic_alias_is_unresolved() {
        let s = extract(
            "from pydantic import BaseModel, Field\nNAME='n'\nclass M(BaseModel):\n    a: int = Field(alias='x', serialization_alias='y')\n    b: int = Field(alias=NAME)\n    c: int = Field(alias=f'{NAME}')\n",
        );
        let f = &s.models[0].fields;
        assert_eq!(f[0].wire_name(), Some("y"));
        assert!(f[1].alias_unresolved);
        assert!(f[2].alias_unresolved);
    }

    #[test]
    fn alias_generator_marks_fields_unresolved() {
        let s = extract(
            "from pydantic import BaseModel, ConfigDict, Field\nfrom pydantic.alias_generators import to_camel\nclass M(BaseModel):\n    model_config = ConfigDict(alias_generator=to_camel)\n    first_name: str\n    explicit: str = Field(alias='EX')\n",
        );
        let f = &s.models[0].fields;
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].wire_name(), None);
        assert_eq!(f[1].wire_name(), Some("EX"));
    }

    #[test]
    fn skips_classvar_and_comments_do_not_matter() {
        let a = extract(
            "from pydantic import BaseModel\nfrom typing import ClassVar\nclass M(BaseModel):\n    k: ClassVar[int] = 1\n    a: int  # comment\n",
        );
        let b = extract(
            "from pydantic import BaseModel\nfrom typing import ClassVar\n\n\nclass M(BaseModel):\n    \"\"\"Docs.\"\"\"\n    k: ClassVar[int] = 1\n\n    a:   int\n",
        );
        assert_eq!(a.models[0].fields.len(), 1);
        assert!(a.models[0].same_shape(&b.models[0]));
    }

    #[test]
    fn syntax_errors_are_reported_not_guessed() {
        let err = PydanticAnalyzer
            .extract("from pydantic import BaseModel\nclass M(BaseModel:\n    a: int\n");
        assert!(matches!(err, Err(AnalysisError::SyntaxError { .. })));
    }
}
