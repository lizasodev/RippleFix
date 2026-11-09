//! Whole-project index: parsed files, import resolution, and model hierarchy.

use ripplefix_core::{Evidence, Span};
use ripplefix_python::{ModuleInfo, Route, is_python, parse_module};
use ripplefix_typescript::{TsFile, is_typescript, parse_path};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// A repository file to index. `path` is repository-relative with `/`.
pub struct SourceFile {
    pub path: String,
    pub text: String,
}

/// A class in the index: (file index, class index within the file).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ClassId(usize, usize);

enum Parsed {
    Python(ModuleInfo),
    TypeScript(TsFile),
    Failed(String),
}

pub struct ProjectIndex {
    files: Vec<(String, Parsed)>,
    by_path: HashMap<String, usize>,
    /// Pydantic models and their resolved model bases (with evidence).
    models: HashMap<ClassId, Vec<(ClassId, Vec<Evidence>)>>,
}

const MAX_DEPTH: usize = 8;

impl ProjectIndex {
    /// Parse every Python and TypeScript file and resolve the model hierarchy.
    pub fn build(sources: Vec<SourceFile>) -> Self {
        let files: Vec<(String, Parsed)> = sources
            .into_iter()
            .filter_map(|s| {
                let p = Path::new(&s.path);
                let parsed = if is_python(p) {
                    parse_module(&s.text).map(Parsed::Python)
                } else if is_typescript(p) {
                    parse_path(p, &s.text).map(Parsed::TypeScript)
                } else {
                    return None;
                };
                Some((
                    s.path,
                    parsed.unwrap_or_else(|e| Parsed::Failed(e.to_string())),
                ))
            })
            .collect();
        let by_path = files
            .iter()
            .enumerate()
            .map(|(i, (p, _))| (p.clone(), i))
            .collect();
        let mut idx = ProjectIndex {
            files,
            by_path,
            models: HashMap::new(),
        };
        idx.resolve_models();
        idx
    }

    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    pub fn failures(&self) -> Vec<(String, String)> {
        self.files
            .iter()
            .filter_map(|(p, f)| match f {
                Parsed::Failed(e) => Some((p.clone(), e.clone())),
                _ => None,
            })
            .collect()
    }

    pub fn python(&self) -> impl Iterator<Item = (&str, &ModuleInfo)> {
        self.files.iter().filter_map(|(p, f)| match f {
            Parsed::Python(m) => Some((p.as_str(), m)),
            _ => None,
        })
    }

    pub fn typescript(&self) -> impl Iterator<Item = (&str, &TsFile)> {
        self.files.iter().filter_map(|(p, f)| match f {
            Parsed::TypeScript(t) => Some((p.as_str(), t)),
            _ => None,
        })
    }

    pub fn routes(&self) -> impl Iterator<Item = (&str, &Vec<Route>)> {
        self.python().map(|(p, m)| (p, &m.routes))
    }

    fn module(&self, file: usize) -> Option<&ModuleInfo> {
        match &self.files.get(file)?.1 {
            Parsed::Python(m) => Some(m),
            _ => None,
        }
    }

    fn class(&self, c: ClassId) -> &ripplefix_python::PyClass {
        &self
            .module(c.0)
            .expect("ClassId points at a Python file")
            .classes[c.1]
    }

    pub fn class_name(&self, c: ClassId) -> &str {
        &self.class(c).qualified_name
    }

    pub fn location(&self, c: ClassId) -> (&str, Span) {
        (&self.files[c.0].0, self.class(c).span)
    }

    pub fn declares_field(&self, c: ClassId, name: &str) -> bool {
        self.class(c).fields.iter().any(|f| f.name == name)
    }

    pub fn find_model(&self, path: &str, qualified_name: &str) -> Option<ClassId> {
        let fi = *self.by_path.get(path)?;
        let ci = self
            .module(fi)?
            .classes
            .iter()
            .position(|c| c.qualified_name == qualified_name)?;
        let id = ClassId(fi, ci);
        self.models.contains_key(&id).then_some(id)
    }

    /// Resolve a module path from an import in `from_file` to a file index.
    /// Returns `None` when the module is external or ambiguous.
    fn resolve_module(&self, from_file: &str, module: &str, level: usize) -> Option<usize> {
        let parts: Vec<&str> = module.split('.').filter(|s| !s.is_empty()).collect();
        let candidates = |base: &str| -> Vec<String> {
            let mut rel = base.to_owned();
            for p in &parts {
                if !rel.is_empty() {
                    rel.push('/');
                }
                rel.push_str(p);
            }
            let init = if rel.is_empty() {
                "__init__.py".to_owned()
            } else {
                format!("{rel}/__init__.py")
            };
            vec![format!("{rel}.py"), init]
        };
        if level > 0 {
            let mut dir: Vec<&str> = from_file.split('/').collect();
            dir.pop(); // file name
            for _ in 1..level {
                dir.pop()?;
            }
            return candidates(&dir.join("/"))
                .into_iter()
                .find_map(|c| self.by_path.get(&c).copied());
        }
        // Absolute import: the module may live under any source root.
        let wanted = candidates("");
        let mut hits = self.files.iter().enumerate().filter(|(_, (p, _))| {
            wanted
                .iter()
                .any(|w| p == w || p.ends_with(&format!("/{w}")))
        });
        let first = hits.next()?.0;
        hits.next().is_none().then_some(first)
    }

    /// Resolve `name` as seen from `file` to a class, following imports.
    pub fn resolve_name(&self, file: &str, name: &str) -> Option<(ClassId, Vec<Evidence>)> {
        let fi = *self.by_path.get(file)?;
        self.resolve_in(fi, name, 0)
    }

    fn resolve_in(&self, fi: usize, name: &str, depth: usize) -> Option<(ClassId, Vec<Evidence>)> {
        if depth > MAX_DEPTH {
            return None;
        }
        let m = self.module(fi)?;
        let path = &self.files[fi].0;
        if let Some(ci) = m.classes.iter().position(|c| c.qualified_name == name) {
            let ev = Evidence::new(format!("`{name}` defined here")).at(path, m.classes[ci].span);
            return Some((ClassId(fi, ci), vec![ev]));
        }
        let follow = |target: usize, inner: &str, imp: &ripplefix_python::PyImport| {
            let (id, mut ev) = self.resolve_in(target, inner, depth + 1)?;
            ev.insert(
                0,
                Evidence::new(format!("`{}` imported here", imp.local)).at(path, imp.span),
            );
            Some((id, ev))
        };
        let (head, rest) = match name.split_once('.') {
            Some((h, r)) => (h, Some(r)),
            None => (name, None),
        };
        for imp in m.imports.iter().rev() {
            match (&imp.name, rest) {
                // from M import X [as name]
                (Some(orig), None) if imp.local == name => {
                    if let Some(t) = self.resolve_module(path, &imp.module, imp.level) {
                        return follow(t, orig, imp);
                    }
                    return None;
                }
                // from pkg import submodule; submodule.X
                (Some(orig), Some(r)) if imp.local == head => {
                    let sub = format!("{}.{orig}", imp.module);
                    let t = self.resolve_module(path, &sub, imp.level)?;
                    return follow(t, r, imp);
                }
                // import a.b [as c]; c.X / a.b.X
                (None, Some(_)) => {
                    if let Some(attr) = name.strip_prefix(&format!("{}.", imp.local)) {
                        let t = self.resolve_module(path, &imp.module, 0)?;
                        return follow(t, attr, imp);
                    }
                }
                _ => {}
            }
        }
        None
    }

    fn resolve_models(&mut self) {
        let mut models: HashMap<ClassId, Vec<(ClassId, Vec<Evidence>)>> = HashMap::new();
        let all: Vec<ClassId> = (0..self.files.len())
            .flat_map(|fi| {
                let n = self.module(fi).map_or(0, |m| m.classes.len());
                (0..n).map(move |ci| ClassId(fi, ci))
            })
            .collect();
        loop {
            let mut grew = false;
            for &c in &all {
                if models.contains_key(&c) {
                    continue;
                }
                let class = self.class(c);
                let path = self.files[c.0].0.clone();
                let mut bases = Vec::new();
                for b in &class.bases {
                    if let Some((target, ev)) = self.resolve_in(c.0, b, 0) {
                        if models.contains_key(&target) {
                            bases.push((target, ev));
                        }
                    }
                }
                if class.local_model_reason.is_some() || !bases.is_empty() {
                    tracing::debug!(class = %class.qualified_name, file = %path, "pydantic model");
                    models.insert(c, bases);
                    grew = true;
                }
            }
            if !grew {
                break;
            }
        }
        self.models = models;
    }

    /// Transitive subclasses of `root` with the evidence chain for each.
    pub fn subclasses(&self, root: ClassId) -> Vec<(ClassId, Vec<Evidence>)> {
        let mut out: Vec<(ClassId, Vec<Evidence>)> = Vec::new();
        let mut seen = HashSet::from([root]);
        let mut frontier = vec![(root, Vec::new())];
        while let Some((cur, chain)) = frontier.pop() {
            let mut children: Vec<_> = self
                .models
                .iter()
                .filter_map(|(c, bases)| {
                    bases
                        .iter()
                        .find(|(b, _)| *b == cur)
                        .map(|(_, ev)| (*c, ev))
                })
                .collect();
            children.sort_by_key(|(c, _)| *c);
            for (child, ev) in children {
                if !seen.insert(child) {
                    continue;
                }
                let (file, span) = self.location(child);
                let mut ch = chain.clone();
                ch.push(
                    Evidence::new(format!(
                        "`{}` inherits `{}`",
                        self.class_name(child),
                        self.class_name(cur)
                    ))
                    .at(file, span),
                );
                ch.extend(ev.iter().cloned());
                out.push((child, ch.clone()));
                frontier.push((child, ch));
            }
        }
        out.sort_by_key(|(c, _)| *c);
        out
    }

    /// JSON keys of a model including inherited fields. The flag is `false`
    /// when some key is not statically known.
    pub fn wire_keys(&self, c: ClassId) -> (Vec<String>, bool) {
        let mut fields: Vec<(String, Option<String>)> = Vec::new();
        self.collect_fields(c, &mut fields, 0);
        let complete = fields.iter().all(|(_, k)| k.is_some());
        (
            fields.into_iter().filter_map(|(_, k)| k).collect(),
            complete,
        )
    }

    fn collect_fields(&self, c: ClassId, out: &mut Vec<(String, Option<String>)>, depth: usize) {
        if depth > MAX_DEPTH {
            return;
        }
        if let Some(bases) = self.models.get(&c) {
            for (b, _) in bases {
                self.collect_fields(*b, out, depth + 1);
            }
        }
        for f in &self.class(c).fields {
            let key = f.wire_name().map(str::to_owned);
            match out.iter_mut().find(|(n, _)| *n == f.name) {
                Some(slot) => slot.1 = key,
                None => out.push((f.name.clone(), key)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(files: &[(&str, &str)]) -> ProjectIndex {
        ProjectIndex::build(
            files
                .iter()
                .map(|(p, t)| SourceFile {
                    path: (*p).into(),
                    text: (*t).into(),
                })
                .collect(),
        )
    }

    #[test]
    fn resolves_cross_file_inheritance_and_routes() {
        let idx = index(&[
            (
                "svc/app/base.py",
                "from pydantic import BaseModel\nclass Base(BaseModel):\n    id: int\n    name: str\n",
            ),
            (
                "svc/app/models.py",
                "from .base import Base\nclass Child(Base):\n    extra: str\n",
            ),
            (
                "svc/app/api.py",
                "from app import models\n@r.get('/c')\ndef get() -> models.Child: ...\n",
            ),
        ]);
        let base = idx.find_model("svc/app/base.py", "Base").unwrap();
        let child = idx
            .find_model("svc/app/models.py", "Child")
            .expect("cross-file model");
        let subs = idx.subclasses(base);
        assert_eq!(subs.len(), 1);
        assert_eq!(subs[0].0, child);
        assert_eq!(
            idx.wire_keys(child),
            (vec!["id".into(), "name".into(), "extra".into()], true)
        );
        let (resolved, ev) = idx.resolve_name("svc/app/api.py", "models.Child").unwrap();
        assert_eq!(resolved, child);
        assert_eq!(ev[0].file.as_deref(), Some("svc/app/api.py"));
    }

    #[test]
    fn ambiguous_absolute_imports_do_not_resolve() {
        let idx = index(&[
            (
                "a/models.py",
                "from pydantic import BaseModel\nclass M(BaseModel):\n    x: int\n",
            ),
            (
                "b/models.py",
                "from pydantic import BaseModel\nclass M(BaseModel):\n    x: int\n",
            ),
            ("c/use.py", "from models import M\n"),
        ]);
        assert!(idx.resolve_name("c/use.py", "M").is_none());
    }

    #[test]
    fn syntax_errors_are_recorded() {
        let idx = index(&[("x.py", "def (:\n"), ("y.ts", "interface {")]);
        assert_eq!(idx.failures().len(), 2);
    }
}
