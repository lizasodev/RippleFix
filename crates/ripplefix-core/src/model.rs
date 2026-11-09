//! Language-neutral representation of the structures RippleFix understands.

use serde::Serialize;

/// 1-based, inclusive line range inside a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Span {
    pub start_line: usize,
    pub end_line: usize,
}

/// How certain RippleFix is about a reported fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// Proven directly from source syntax (e.g. a field literally disappeared).
    Confirmed,
    /// Derived by an explicit, documented heuristic. Never auto-applied silently.
    Inferred,
    /// RippleFix could not determine the answer (e.g. dynamic code).
    Unresolved,
}

/// A human-readable, traceable justification for a reported fact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Evidence {
    pub description: String,
    /// Path of the file the evidence refers to, relative to the repository root.
    pub file: Option<String>,
    pub span: Option<Span>,
}

impl Evidence {
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
            file: None,
            span: None,
        }
    }

    pub fn at(mut self, file: impl Into<String>, span: Span) -> Self {
        self.file = Some(file.into());
        self.span = Some(span);
        self
    }
}

/// Kind of data-model a plugin recognized.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelKind {
    PydanticModel,
}

/// A field declared on a data model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Field {
    /// Attribute name in source code.
    pub name: String,
    /// Type annotation with insignificant whitespace removed.
    pub annotation: String,
    /// Default value expression with insignificant whitespace removed, if any.
    pub default: Option<String>,
    /// Statically-known serialization alias, if declared as a string literal.
    pub alias: Option<String>,
    /// `true` when an alias exists but is not a literal (cannot be resolved statically).
    pub alias_unresolved: bool,
    pub span: Span,
}

impl Field {
    /// Name used on the wire (JSON key) when it is statically known.
    pub fn wire_name(&self) -> Option<&str> {
        if self.alias_unresolved {
            None
        } else {
            Some(self.alias.as_deref().unwrap_or(&self.name))
        }
    }

    /// Structural equality: everything except source position.
    pub fn same_shape(&self, other: &Field) -> bool {
        self.name == other.name
            && self.annotation == other.annotation
            && self.default == other.default
            && self.alias == other.alias
            && self.alias_unresolved == other.alias_unresolved
    }
}

/// A data model (e.g. a Pydantic `BaseModel` subclass).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DataModel {
    pub kind: ModelKind,
    /// Dotted name including enclosing classes, e.g. `Outer.Inner`.
    pub qualified_name: String,
    /// Base classes as written in source.
    pub bases: Vec<String>,
    pub fields: Vec<Field>,
    pub span: Span,
    /// Why this class was classified as a model.
    pub evidence: Evidence,
}

impl DataModel {
    pub fn field(&self, name: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.name == name)
    }

    pub fn same_shape(&self, other: &DataModel) -> bool {
        self.kind == other.kind
            && self.qualified_name == other.qualified_name
            && self.bases == other.bases
            && self.fields.len() == other.fields.len()
            && self
                .fields
                .iter()
                .zip(&other.fields)
                .all(|(a, b)| a.same_shape(b))
    }
}

/// Everything a plugin extracted from one version of one file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct FileSnapshot {
    pub models: Vec<DataModel>,
}

impl FileSnapshot {
    pub fn model(&self, qualified_name: &str) -> Option<&DataModel> {
        self.models
            .iter()
            .find(|m| m.qualified_name == qualified_name)
    }
}
