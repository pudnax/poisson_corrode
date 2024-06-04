// Simplified version of: https://github.com/rerun-io/rerun/blob/43aa22c426ba845ed9384db7b07d5ca0de85581f/crates/re_renderer/src/file_resolver.rs

use ahash::{AHashMap, AHashSet};
use clean_path::Clean;
use eyre::{bail, eyre, Context};
use std::{
    path::{Path, PathBuf},
    rc::Rc,
};

#[derive(Clone, Debug, PartialEq, Eq)]
struct ImportClause {
    path: PathBuf,
}

impl ImportClause {
    pub const PREFIX: &'static str = "#import ";
}

impl<P: Into<PathBuf>> From<P> for ImportClause {
    fn from(path: P) -> Self {
        Self { path: path.into() }
    }
}

impl std::str::FromStr for ImportClause {
    type Err = eyre::Error;

    fn from_str(clause_str: &str) -> Result<Self, Self::Err> {
        let s = clause_str.trim();

        if !s.starts_with(Self::PREFIX) {
            return Err(eyre!(
                "import clause must start with {:?}, got {s:?}",
                Self::PREFIX
            ));
        }

        let s = s.trim_start_matches(Self::PREFIX).trim();

        let splits = s
            .find('"')
            .and_then(|i0| s.rfind('"').map(|i1| (i0 + 1, i1)));

        if let Some((i0, i1)) = splits {
            let s = &s[i0..i1];

            if s.is_empty() {
                return Err(eyre!("import clause must contain a non-empty path"));
            }

            return s
                .parse()
                .with_context(|| "couldn't parse {s:?} as PathBuf")
                .map(|path| Self { path });
        }

        bail!("misformatted import clause: {clause_str:?}")
    }
}

impl std::fmt::Display for ImportClause {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("#import <{}>", self.path.to_string_lossy()))
    }
}

#[derive(Clone, Debug, Default)]
pub struct ResolvedFile {
    pub contents: String,
    pub imports: AHashSet<PathBuf>,
}

#[derive(Default)]
pub struct ImportResolver {
    search_path: Vec<PathBuf>,
}

impl ImportResolver {
    pub fn new(search_path: &[impl AsRef<Path>]) -> Self {
        Self {
            search_path: search_path
                .iter()
                .map(|p| p.as_ref().canonicalize().unwrap())
                .collect(),
        }
    }

    pub fn populate(&mut self, path: impl AsRef<Path>) -> eyre::Result<ResolvedFile> {
        let mut path_stack = Vec::new();
        let mut visited_stack = AHashSet::new();
        let mut resolved_files = AHashMap::default();

        fn populate_impl(
            this: &mut ImportResolver,
            path: impl AsRef<Path>,
            resolved_files: &mut AHashMap<PathBuf, Rc<ResolvedFile>>,
            path_stack: &mut Vec<PathBuf>,
            visited_stack: &mut AHashSet<PathBuf>,
        ) -> eyre::Result<Rc<ResolvedFile>> {
            let path = path.as_ref().clean();

            path_stack.push(path.clone());
            if !visited_stack.insert(path.clone()) {
                return Err(eyre!("import cycle detected: {path_stack:?}"));
            }

            if resolved_files.contains_key(&path) {
                path_stack.pop().unwrap();
                visited_stack.remove(&path);

                return Ok(Default::default());
            }

            let contents = std::fs::read_to_string(&path)?;

            let mut imports = AHashSet::new();

            let children: Result<Vec<_>, _> = contents
                .lines()
                .map(|line| {
                    if line.trim().starts_with(ImportClause::PREFIX) {
                        let clause = line.parse::<ImportClause>()?;
                        let cwd = path.join("..").clean();
                        let clause_path =
                            this.resolve_clause_path(cwd, &clause.path).ok_or_else(|| {
                                eyre!("couldn't resolve import clause path at {:?}", clause.path)
                            })?;
                        imports.insert(clause_path.clone());
                        populate_impl(this, clause_path, resolved_files, path_stack, visited_stack)
                    } else {
                        Ok(Rc::new(ResolvedFile {
                            contents: line.to_owned(),
                            ..Default::default()
                        }))
                    }
                })
                .collect();
            let children = children?;

            let interp = children.into_iter().fold(
                ResolvedFile {
                    imports,
                    ..Default::default()
                },
                |acc, child| ResolvedFile {
                    contents: match (acc.contents.is_empty(), child.contents.is_empty()) {
                        (true, _) => child.contents.clone(),
                        (_, true) => acc.contents,
                        _ => [acc.contents.as_str(), child.contents.as_str()].join("\n"),
                    },
                    imports: acc.imports.union(&child.imports).cloned().collect(),
                },
            );

            let resolved = Rc::new(interp);
            resolved_files.insert(path.clone(), Rc::clone(&resolved));

            path_stack.pop().unwrap();
            visited_stack.remove(&path);

            Ok(resolved)
        }

        populate_impl(
            self,
            path,
            &mut resolved_files,
            &mut path_stack,
            &mut visited_stack,
        )
        .map(|reslv| (*reslv).clone())
    }

    fn resolve_clause_path(
        &self,
        cwd: impl AsRef<Path>,
        path: impl AsRef<Path>,
    ) -> Option<PathBuf> {
        let path = path.as_ref().clean();

        if path.is_absolute() && path.exists() {
            return path.into();
        }

        {
            let path = cwd.as_ref().join(&path).clean();
            if path.exists() {
                return path.into();
            }
        }

        for dir in self.search_path.iter() {
            let dir = dir.join(&path).clean();
            if dir.exists() {
                return dir.into();
            }
        }

        None
    }
}
