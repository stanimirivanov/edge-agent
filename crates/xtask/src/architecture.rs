mod model;
mod policy;

#[cfg(test)]
mod tests;

use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
    path::PathBuf,
};

use cargo_metadata::{DependencyKind, Metadata, MetadataCommand};
use model::{AppliedException, DependencyClass, DependencyEdge, PackagePolicy, WorkspaceGraph};
use policy::{PACKAGE_POLICIES, TEMPORARY_EXCEPTIONS};

const WORKSPACE_MANIFEST: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.toml");

#[derive(Debug)]
pub(crate) struct ArchitectureReport {
    package_count: usize,
    dependency_count: usize,
    temporary_exceptions: Vec<AppliedException>,
}

impl ArchitectureReport {
    pub(crate) const fn package_count(&self) -> usize {
        self.package_count
    }

    pub(crate) const fn dependency_count(&self) -> usize {
        self.dependency_count
    }

    pub(crate) fn temporary_exceptions(&self) -> &[AppliedException] {
        &self.temporary_exceptions
    }
}

#[derive(Debug)]
pub(crate) enum ArchitectureCheckError {
    Metadata(cargo_metadata::Error),
    Policy(Vec<String>),
}

impl fmt::Display for ArchitectureCheckError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Metadata(error) => write!(formatter, "could not read Cargo metadata: {error}"),
            Self::Policy(violations) => {
                for (index, violation) in violations.iter().enumerate() {
                    if index > 0 {
                        formatter.write_str("\n")?;
                    }
                    write!(formatter, "- {violation}")?;
                }
                Ok(())
            }
        }
    }
}

impl Error for ArchitectureCheckError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Metadata(error) => Some(error),
            Self::Policy(_) => None,
        }
    }
}

/// Checks every direct dependency between workspace packages.
pub(crate) fn check() -> Result<ArchitectureReport, ArchitectureCheckError> {
    let policy_violations = validate_policy();
    if !policy_violations.is_empty() {
        return Err(ArchitectureCheckError::Policy(policy_violations));
    }

    let metadata = load_metadata().map_err(ArchitectureCheckError::Metadata)?;
    let graph = graph_from_metadata(&metadata)?;
    validate_graph(&graph)
}

fn load_metadata() -> Result<Metadata, cargo_metadata::Error> {
    let mut command = MetadataCommand::new();
    command
        .manifest_path(PathBuf::from(WORKSPACE_MANIFEST))
        .no_deps()
        .other_options(vec!["--locked".to_owned()]);
    command.exec()
}

fn graph_from_metadata(metadata: &Metadata) -> Result<WorkspaceGraph, ArchitectureCheckError> {
    let workspace_packages = metadata.workspace_packages();
    let package_roots: BTreeMap<_, _> = workspace_packages
        .iter()
        .filter_map(|package| {
            package
                .manifest_path
                .parent()
                .map(|root| (root.as_str().to_owned(), package.name.as_str().to_owned()))
        })
        .collect();
    let packages = workspace_packages
        .iter()
        .map(|package| package.name.as_str().to_owned())
        .collect();
    let mut dependencies = BTreeSet::new();

    for package in workspace_packages {
        for dependency in &package.dependencies {
            let Some(dependency_name) = internal_dependency_name(
                dependency.path.as_ref().map(|path| path.as_str()),
                &package_roots,
            ) else {
                continue;
            };
            let class = dependency_class(dependency.kind).map_err(|violation| {
                ArchitectureCheckError::Policy(vec![format!(
                    "{violation} on {} -> {dependency_name}",
                    package.name
                )])
            })?;
            dependencies.insert(DependencyEdge::new(
                package.name.as_str(),
                dependency_name,
                class,
            ));
        }
    }

    Ok(WorkspaceGraph {
        packages,
        dependencies,
    })
}

fn internal_dependency_name<'a>(
    dependency_path: Option<&str>,
    package_roots: &'a BTreeMap<String, String>,
) -> Option<&'a str> {
    dependency_path
        .and_then(|path| package_roots.get(path))
        .map(String::as_str)
}

fn dependency_class(kind: DependencyKind) -> Result<DependencyClass, &'static str> {
    match kind {
        DependencyKind::Normal => Ok(DependencyClass::Normal),
        DependencyKind::Development => Ok(DependencyClass::Development),
        DependencyKind::Build => Ok(DependencyClass::Build),
        DependencyKind::Unknown => Err("unknown Cargo dependency kind"),
    }
}

fn validate_policy() -> Vec<String> {
    let mut violations = BTreeSet::new();
    let mut package_names = BTreeSet::new();
    let mut allowed_edges = BTreeSet::new();

    for package in PACKAGE_POLICIES {
        if !package_names.insert(package.name) {
            violations.insert(format!("duplicate package policy for `{}`", package.name));
        }
        for edge in package.dependencies() {
            if !allowed_edges.insert(edge.clone()) {
                violations.insert(format!("duplicate dependency rule: {edge}"));
            }
        }
    }

    for edge in &allowed_edges {
        if !package_names.contains(edge.dependency.as_str()) {
            violations.insert(format!(
                "dependency policy targets an unknown package: {edge}"
            ));
        }
    }

    let mut exception_edges = BTreeSet::new();
    for exception in TEMPORARY_EXCEPTIONS {
        let edge = exception.edge();
        if !package_names.contains(exception.package) {
            violations.insert(format!(
                "temporary exception source `{}` has no package policy",
                exception.package
            ));
        }
        if !package_names.contains(exception.dependency) {
            violations.insert(format!(
                "temporary exception target `{}` has no package policy",
                exception.dependency
            ));
        }
        if allowed_edges.contains(&edge) {
            violations.insert(format!(
                "temporary exception is also a permanent dependency: {edge}"
            ));
        }
        if !exception_edges.insert(edge.clone()) {
            violations.insert(format!("duplicate temporary exception: {edge}"));
        }
        if exception.tracking.trim().is_empty() || exception.reason.trim().is_empty() {
            violations.insert(format!(
                "temporary exception must have tracking and a reason: {edge}"
            ));
        }
    }

    let production_edges: BTreeSet<_> = allowed_edges
        .union(&exception_edges)
        .filter(|edge| edge.is_production())
        .cloned()
        .collect();
    if has_dependency_cycle(&package_names, &production_edges) {
        violations.insert("production dependency policy contains a cycle".to_owned());
    }

    violations.into_iter().collect()
}

fn validate_graph(graph: &WorkspaceGraph) -> Result<ArchitectureReport, ArchitectureCheckError> {
    let expected_packages: BTreeSet<_> = PACKAGE_POLICIES
        .iter()
        .map(|package| package.name.to_owned())
        .collect();
    let policy_by_name: BTreeMap<_, _> = PACKAGE_POLICIES
        .iter()
        .map(|package| (package.name, package))
        .collect();
    let allowed_dependencies: BTreeSet<_> = PACKAGE_POLICIES
        .iter()
        .flat_map(PackagePolicy::dependencies)
        .collect();
    let exception_dependencies: BTreeSet<_> = TEMPORARY_EXCEPTIONS
        .iter()
        .map(model::TemporaryException::edge)
        .collect();
    let expected_dependencies: BTreeSet<_> = allowed_dependencies
        .union(&exception_dependencies)
        .cloned()
        .collect();
    let mut violations = BTreeSet::new();

    for package in graph.packages.difference(&expected_packages) {
        violations.insert(format!(
            "workspace package `{package}` has no architecture policy"
        ));
    }
    for package in expected_packages.difference(&graph.packages) {
        let role = policy_by_name
            .get(package.as_str())
            .map_or_else(|| "unknown".to_owned(), |policy| policy.role.to_string());
        violations.insert(format!(
            "stale architecture policy for missing {role} package `{package}`"
        ));
    }
    for edge in graph.dependencies.difference(&expected_dependencies) {
        let role = policy_by_name.get(edge.package.as_str()).map_or_else(
            || "unclassified".to_owned(),
            |policy| policy.role.to_string(),
        );
        violations.insert(format!("unapproved dependency from {role} package: {edge}"));
    }
    for edge in allowed_dependencies.difference(&graph.dependencies) {
        violations.insert(format!("stale dependency rule: {edge}"));
    }
    for edge in exception_dependencies.difference(&graph.dependencies) {
        violations.insert(format!("stale temporary exception: {edge}"));
    }

    if !violations.is_empty() {
        return Err(ArchitectureCheckError::Policy(
            violations.into_iter().collect(),
        ));
    }

    Ok(ArchitectureReport {
        package_count: graph.packages.len(),
        dependency_count: graph.dependencies.len(),
        temporary_exceptions: TEMPORARY_EXCEPTIONS
            .iter()
            .map(AppliedException::from_policy)
            .collect(),
    })
}

fn has_dependency_cycle(packages: &BTreeSet<&str>, edges: &BTreeSet<DependencyEdge>) -> bool {
    let pairs: BTreeSet<_> = edges
        .iter()
        .map(|edge| (edge.package.as_str(), edge.dependency.as_str()))
        .collect();
    let mut incoming: BTreeMap<_, usize> = packages.iter().map(|package| (*package, 0)).collect();
    let mut outgoing: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();

    for (package, dependency) in pairs {
        outgoing.entry(package).or_default().insert(dependency);
        if let Some(count) = incoming.get_mut(dependency) {
            *count += 1;
        }
    }

    let mut ready: BTreeSet<_> = incoming
        .iter()
        .filter_map(|(package, count)| (*count == 0).then_some(*package))
        .collect();
    let mut visited = 0;
    while let Some(package) = ready.pop_first() {
        visited += 1;
        for dependency in outgoing.get(package).into_iter().flatten() {
            if let Some(count) = incoming.get_mut(dependency) {
                *count -= 1;
                if *count == 0 {
                    ready.insert(dependency);
                }
            }
        }
    }

    visited != packages.len()
}
