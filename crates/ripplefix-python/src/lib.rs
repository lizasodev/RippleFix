//! Python / Pydantic / FastAPI analyzer for RippleFix.
//!
//! Classification rules (all syntactic and deterministic):
//! * A class is a Pydantic model if a base is `BaseModel` imported from
//!   `pydantic` (including `as` aliases), `pydantic.BaseModel` with
//!   `import pydantic`, or another model class defined in the same file.
//!   Cross-file inheritance is resolved at project level by `ripplefix-impact`.
//! * Fields are annotated class-level assignments, excluding private
//!   (`_x`) names, `model_config`, and `ClassVar` annotations.
//! * A field wire name is resolved only from string-literal `alias=` /
//!   `serialization_alias=` arguments to `Field(...)`. A class configuring an
//!   `alias_generator` marks fields without a literal alias as unresolved.
//! * A FastAPI route is a function decorated with `<obj>.get|post|put|patch|
//!   delete|api_route(...)`; its response types come from `response_model=`
//!   or, if absent, from the return annotation.

use ripplefix_core::model::{DataModel, Evidence, Field, FileSnapshot, ModelKind, Span};
use ripplefix_core::plugin::{AnalysisError, LanguageAnalyzer, PLUGIN_API_VERSION};
use serde::Serialize;
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
        is_python(path)
    }

    fn extract(&self, source: &str) -> Result<FileSnapshot, AnalysisError> {
        Ok(parse_module(source)?.snapshot())
    }
}

pub fn is_python(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("py" | "pyi")
    )
}

/// A class definition (model or not) with its annotated fields.
#[derive(Debug, Clone, Serialize)]
pub struct PyClass {
    pub qualified_name: String,
    pub simple_name: String,
    pub bases: Vec<String>,
    pub fields: Vec<Field>,
    pub span: Span,
    /// Set when the class is a Pydantic model by same-file evidence.
    pub local_model_reason: Option<String>,
}

/// `from <module> import <name> as <local>` (or `import <module> as <local>`
/// with `name == None`). `level` counts leading dots of relative imports.
#[derive(Debug, Clone, Serialize)]
pub struct PyImport {
    pub local: String,
    pub module: String,
    pub level: usize,
    pub name: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponseSource {
    ResponseModel,
    ReturnAnnotation,
}

#[derive(Debug, Clone, Serialize)]
pub struct Route {
    pub method: String,
    pub path: Option<String>,
    pub function: String,
    /// Names referenced by the response type, e.g. `["list", "UserRead"]`.
    pub response_names: Vec<String>,
    pub response_source: ResponseSource,
    /// Names referenced by typed parameters (request bodies, query models).
    pub request_names: Vec<String>,
    pub span: Span,
}

/// A call like `client.get("/users/1")` (typically in tests).
#[derive(Debug, Clone, Serialize)]
pub struct HttpCall {
    pub method: String,
    pub url: String,
    pub span: Span,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ModuleInfo {
    pub classes: Vec<PyClass>,
    pub imports: Vec<PyImport>,
    pub routes: Vec<Route>,
    pub http_calls: Vec<HttpCall>,
    /// Plain string literal values with their location.
    pub strings: Vec<(String, Span)>,
}

impl ModuleInfo {
    /// Models identifiable from this file alone.
    pub fn snapshot(&self) -> FileSnapshot {
        FileSnapshot {
            models: self
                .classes
                .iter()
                .filter_map(|c| {
                    let reason = c.local_model_reason.as_ref()?;
                    Some(DataModel {
                        kind: ModelKind::PydanticModel,
                        evidence: Evidence::new(format!("class `{}` {reason}", c.qualified_name)),
                        qualified_name: c.qualified_name.clone(),
                        bases: c.bases.clone(),
                        fields: c.fields.clone(),
                        span: c.span,
                    })
                })
                .collect(),
        }
    }
}

const HTTP_METHODS: &[&str] = &["get", "post", "put", "patch", "delete", "head", "options"];

/// Parse a Python module. Fails on syntax errors rather than guessing.
pub fn parse_module(source: &str) -> Result<ModuleInfo, AnalysisError> {
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
    let pyd = PydanticNames::collect(root, src);

    let mut raw = Vec::new();
    collect_classes(root, src, "", &mut raw);

    // Fixpoint: classes inheriting from same-file models are models too.
    let mut model_names: HashSet<String> = HashSet::new();
    let mut reasons: Vec<Option<String>> = vec![None; raw.len()];
    loop {
        let mut grew = false;
        for (i, c) in raw.iter().enumerate() {
            if reasons[i].is_some() {
                continue;
            }
            if let Some(r) = c
                .bases
                .iter()
                .find_map(|b| pyd.model_reason(b, &model_names))
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

    let classes = raw
        .into_iter()
        .zip(reasons)
        .map(|(c, reason)| PyClass {
            fields: extract_fields(c.body, src, &pyd),
            span: span(c.node),
            qualified_name: c.qualified_name,
            simple_name: c.simple_name,
            bases: c.bases,
            local_model_reason: reason,
        })
        .collect();

    let mut info = ModuleInfo {
        classes,
        imports: collect_imports(root, src),
        ..Default::default()
    };
    walk_module(root, src, &mut info);
    Ok(info)
}

struct RawClass<'t> {
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
fn collect_classes<'t>(n: Node<'t>, src: &[u8], prefix: &str, out: &mut Vec<RawClass<'t>>) {
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
                out.push(RawClass {
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

fn collect_imports(root: Node, src: &[u8]) -> Vec<PyImport> {
    let mut out = Vec::new();
    let mut cur = root.walk();
    for stmt in root.named_children(&mut cur) {
        match stmt.kind() {
            "import_from_statement" => {
                let Some(m) = stmt.child_by_field_name("module_name") else {
                    continue;
                };
                let raw = text(m, src);
                let level = raw.chars().take_while(|c| *c == '.').count();
                let module = raw[level..].to_owned();
                let mut c = stmt.walk();
                for name in stmt.children_by_field_name("name", &mut c) {
                    let (orig, local) = imported_name(name, src);
                    out.push(PyImport {
                        local: local.to_owned(),
                        module: module.clone(),
                        level,
                        name: Some(orig.to_owned()),
                        span: span(stmt),
                    });
                }
            }
            "import_statement" => {
                let mut c = stmt.walk();
                for name in stmt.children_by_field_name("name", &mut c) {
                    let (orig, local) = imported_name(name, src);
                    out.push(PyImport {
                        local: local.to_owned(),
                        module: orig.to_owned(),
                        level: 0,
                        name: None,
                        span: span(stmt),
                    });
                }
            }
            _ => {}
        }
    }
    out
}

/// Names under which `pydantic`, `pydantic.BaseModel`, and `pydantic.Field`
/// are visible in a module.
#[derive(Default)]
struct PydanticNames {
    base_model: HashSet<String>,
    field_fn: HashSet<String>,
    module: HashSet<String>,
}

impl PydanticNames {
    fn collect(root: Node, src: &[u8]) -> Self {
        let mut p = PydanticNames::default();
        for imp in collect_imports(root, src) {
            match (imp.level, imp.module.as_str(), imp.name.as_deref()) {
                (0, "pydantic", Some("BaseModel")) => p.base_model.insert(imp.local),
                (0, "pydantic", Some("Field")) => p.field_fn.insert(imp.local),
                (0, "pydantic", None) => p.module.insert(imp.local),
                _ => false,
            };
        }
        p
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

fn extract_fields(body: Node, src: &[u8], pyd: &PydanticNames) -> Vec<Field> {
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
        let (alias, dynamic_alias) = right
            .map(|r| field_alias(r, src, pyd))
            .unwrap_or((None, false));
        fields.push(Field {
            name: name.to_owned(),
            annotation,
            default: right.and_then(|r| default_expr(r, src, pyd)),
            alias_unresolved: dynamic_alias || (has_alias_generator && alias.is_none()),
            alias,
            span: span(stmt),
        });
    }
    fields
}

/// Alias-related keyword arguments of `Field(...)`. They control the JSON key,
/// which is tracked separately, so they are not part of the default.
const ALIAS_KWARGS: &[&str] = &["alias", "serialization_alias", "validation_alias"];

/// The default/constraint part of a field assignment. For `Field(...)` calls,
/// alias arguments are dropped, and `Field()` with nothing left means required.
fn default_expr(right: Node, src: &[u8], pyd: &PydanticNames) -> Option<String> {
    let is_field = right.kind() == "call"
        && right
            .child_by_field_name("function")
            .is_some_and(|f| pyd.is_field_call(text(f, src)));
    if !is_field {
        return Some(normalize(right, src));
    }
    let args = right.child_by_field_name("arguments")?;
    let mut cur = args.walk();
    let kept: Vec<String> = args
        .named_children(&mut cur)
        .filter(|a| a.kind() != "comment")
        .filter(|a| {
            a.kind() != "keyword_argument"
                || !a
                    .child_by_field_name("name")
                    .is_some_and(|n| ALIAS_KWARGS.contains(&text(n, src)))
        })
        .map(|a| normalize(a, src))
        .collect();
    (!kept.is_empty()).then(|| format!("Field({})", kept.join(",")))
}

/// Returns `(literal_alias, alias_is_dynamic)` for a `Field(...)` call.
fn field_alias(right: Node, src: &[u8], pyd: &PydanticNames) -> (Option<String>, bool) {
    if right.kind() != "call" {
        return (None, false);
    }
    let Some(func) = right.child_by_field_name("function") else {
        return (None, false);
    };
    if !pyd.is_field_call(text(func, src)) {
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
    let prefix: String = text(n, src)
        .chars()
        .take_while(|ch| ch.is_ascii_alphabetic())
        .collect();
    if prefix.to_ascii_lowercase().contains('f') {
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

/// Collect routes, HTTP calls, and string literals across the whole module.
fn walk_module(n: Node, src: &[u8], info: &mut ModuleInfo) {
    match n.kind() {
        "decorated_definition" => {
            if let Some(r) = route(n, src) {
                info.routes.push(r);
            }
        }
        // Route decorators are declarations, not client calls.
        "decorator" => return,
        "call" => {
            if let Some(c) = http_call(n, src) {
                info.http_calls.push(c);
            }
        }
        "string" => {
            if let Some(s) = string_literal(n, src) {
                info.strings.push((s, span(n)));
            }
            return;
        }
        _ => {}
    }
    let mut cur = n.walk();
    for c in n.named_children(&mut cur) {
        walk_module(c, src, info);
    }
}

/// `obj.method` where method is an HTTP verb; returns the verb.
fn verb_of(func: Node, src: &[u8]) -> Option<String> {
    if func.kind() != "attribute" {
        return None;
    }
    let attr = text(func.child_by_field_name("attribute")?, src);
    (HTTP_METHODS.contains(&attr) || attr == "api_route").then(|| attr.to_owned())
}

fn first_string_arg(call: Node, src: &[u8]) -> Option<String> {
    let args = call.child_by_field_name("arguments")?;
    let mut cur = args.walk();
    let first = args.named_children(&mut cur).next()?;
    string_literal(first, src)
}

fn keyword_arg<'t>(call: Node<'t>, src: &[u8], key: &str) -> Option<Node<'t>> {
    let args = call.child_by_field_name("arguments")?;
    let mut cur = args.walk();
    let found = args.named_children(&mut cur).find(|a| {
        a.kind() == "keyword_argument"
            && a.child_by_field_name("name").map(|n| text(n, src)) == Some(key)
    })?;
    found.child_by_field_name("value")
}

fn route(decorated: Node, src: &[u8]) -> Option<Route> {
    let def = decorated.child_by_field_name("definition")?;
    if def.kind() != "function_definition" {
        return None;
    }
    let mut cur = decorated.walk();
    for dec in decorated.named_children(&mut cur) {
        if dec.kind() != "decorator" {
            continue;
        }
        let Some(call) = dec.named_child(0).filter(|c| c.kind() == "call") else {
            continue;
        };
        let Some(method) = call
            .child_by_field_name("function")
            .and_then(|f| verb_of(f, src))
        else {
            continue;
        };
        let (names, source) = match keyword_arg(call, src, "response_model") {
            Some(rm) => (referenced_names(rm, src), ResponseSource::ResponseModel),
            None => (
                def.child_by_field_name("return_type")
                    .map(|t| referenced_names(t, src))
                    .unwrap_or_default(),
                ResponseSource::ReturnAnnotation,
            ),
        };
        let mut request_names = Vec::new();
        if let Some(params) = def.child_by_field_name("parameters") {
            let mut pc = params.walk();
            for p in params.named_children(&mut pc) {
                if matches!(p.kind(), "typed_parameter" | "typed_default_parameter") {
                    if let Some(t) = p.child_by_field_name("type") {
                        request_names.extend(referenced_names(t, src));
                    }
                }
            }
        }
        return Some(Route {
            request_names,
            method,
            path: first_string_arg(call, src),
            function: text(def.child_by_field_name("name")?, src).to_owned(),
            response_names: names,
            response_source: source,
            span: span(dec),
        });
    }
    None
}

fn http_call(call: Node, src: &[u8]) -> Option<HttpCall> {
    let method = verb_of(call.child_by_field_name("function")?, src)?;
    let args = call.child_by_field_name("arguments")?;
    let mut cur = args.walk();
    let first = args.named_children(&mut cur).next()?;
    let url = string_literal(first, src).or_else(|| fstring_prefix(first, src))?;
    url.starts_with('/').then(|| HttpCall {
        method,
        url,
        span: span(call),
    })
}

/// For f-strings like `f"/users/{uid}"`, a template with `{}` placeholders.
fn fstring_prefix(n: Node, src: &[u8]) -> Option<String> {
    if n.kind() != "string" {
        return None;
    }
    let mut out = String::new();
    let mut cur = n.walk();
    for c in n.named_children(&mut cur) {
        match c.kind() {
            "string_start" | "string_end" => {}
            "string_content" => out.push_str(text(c, src)),
            "interpolation" => out.push_str("{}"),
            _ => return None,
        }
    }
    Some(out)
}

/// Identifiers and dotted names referenced by a type expression.
fn referenced_names(n: Node, src: &[u8]) -> Vec<String> {
    fn walk(n: Node, src: &[u8], out: &mut Vec<String>) {
        match n.kind() {
            "identifier" => out.push(text(n, src).to_owned()),
            "attribute" => out.push(normalize(n, src)),
            "string" => {}
            _ => {
                let mut cur = n.walk();
                for c in n.named_children(&mut cur) {
                    walk(c, src, out);
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(n, src, &mut out);
    out
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
    fn alias_arguments_are_not_part_of_the_default() {
        let s = extract(
            "from pydantic import BaseModel, Field
class M(BaseModel):
    a: str = Field(alias='x')
    b: str = Field('d', alias='y')
    c: str = Field(min_length=3)
",
        );
        let f = &s.models[0].fields;
        assert_eq!(f[0].default, None);
        assert_eq!(f[1].default.as_deref(), Some("Field('d')"));
        assert_eq!(f[2].default.as_deref(), Some("Field(min_length=3)"));
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

    #[test]
    fn extracts_routes_imports_and_http_calls() {
        let m = parse_module(
            "from .models import UserRead as UR\nimport app.db\n@router.get('/users/{uid}', response_model=list[UR])\ndef list_users(uid: int):\n    pass\n@app.post('/x')\nasync def mk() -> models.Thing:\n    pass\ndef test_it():\n    client.get(f'/users/{uid}')\n    client.post('/x', json={'full_name': 'a'})\n",
        )
        .unwrap();
        assert_eq!(m.imports[0].level, 1);
        assert_eq!(m.imports[0].module, "models");
        assert_eq!(m.imports[0].local, "UR");
        assert_eq!(m.imports[1].name, None);
        assert_eq!(m.routes.len(), 2);
        assert_eq!(m.routes[0].path.as_deref(), Some("/users/{uid}"));
        assert_eq!(m.routes[0].response_names, ["list", "UR"]);
        assert_eq!(m.routes[0].request_names, ["int"]);
        assert_eq!(m.routes[1].response_names, ["models.Thing"]);
        assert_eq!(
            m.routes[1].response_source,
            ResponseSource::ReturnAnnotation
        );
        let urls: Vec<_> = m.http_calls.iter().map(|c| c.url.as_str()).collect();
        assert_eq!(urls, ["/users/{}", "/x"]);
        assert!(m.strings.iter().any(|(s, _)| s == "full_name"));
    }
}
