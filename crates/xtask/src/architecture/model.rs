use std::{collections::BTreeSet, fmt};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum DependencyClass {
    Normal,
    Development,
    Build,
}

impl fmt::Display for DependencyClass {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Normal => "normal",
            Self::Development => "development",
            Self::Build => "build",
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CrateRole {
    Contract,
    Port,
    Application,
    Adapter,
    Observability,
    RuntimeSupport,
    CompositionRoot,
    Tooling,
}

impl fmt::Display for CrateRole {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Contract => "contract",
            Self::Port => "port",
            Self::Application => "application",
            Self::Adapter => "adapter",
            Self::Observability => "observability",
            Self::RuntimeSupport => "runtime support",
            Self::CompositionRoot => "composition root",
            Self::Tooling => "tooling",
        })
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct DependencyEdge {
    pub(super) package: String,
    pub(super) dependency: String,
    pub(super) class: DependencyClass,
}

impl DependencyEdge {
    pub(super) fn new(
        package: impl Into<String>,
        dependency: impl Into<String>,
        class: DependencyClass,
    ) -> Self {
        Self {
            package: package.into(),
            dependency: dependency.into(),
            class,
        }
    }

    pub(super) fn is_production(&self) -> bool {
        self.class != DependencyClass::Development
    }
}

impl fmt::Display for DependencyEdge {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} -[{}]-> {}",
            self.package, self.class, self.dependency
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct WorkspaceGraph {
    pub(super) packages: BTreeSet<String>,
    pub(super) dependencies: BTreeSet<DependencyEdge>,
}

pub(super) struct PackagePolicy {
    pub(super) name: &'static str,
    pub(super) role: CrateRole,
    pub(super) normal: &'static [&'static str],
    pub(super) development: &'static [&'static str],
    pub(super) build: &'static [&'static str],
}

impl PackagePolicy {
    pub(super) fn dependencies(&self) -> impl Iterator<Item = DependencyEdge> + '_ {
        [
            (DependencyClass::Normal, self.normal),
            (DependencyClass::Development, self.development),
            (DependencyClass::Build, self.build),
        ]
        .into_iter()
        .flat_map(move |(class, dependencies)| {
            dependencies
                .iter()
                .map(move |dependency| DependencyEdge::new(self.name, *dependency, class))
        })
    }
}

pub(super) struct TemporaryException {
    pub(super) package: &'static str,
    pub(super) dependency: &'static str,
    pub(super) class: DependencyClass,
    pub(super) tracking: &'static str,
    pub(super) reason: &'static str,
}

impl TemporaryException {
    pub(super) fn edge(&self) -> DependencyEdge {
        DependencyEdge::new(self.package, self.dependency, self.class)
    }
}

#[derive(Debug)]
pub(crate) struct AppliedException {
    edge: DependencyEdge,
    tracking: &'static str,
    reason: &'static str,
}

impl AppliedException {
    pub(super) fn from_policy(exception: &TemporaryException) -> Self {
        Self {
            edge: exception.edge(),
            tracking: exception.tracking,
            reason: exception.reason,
        }
    }
}

impl fmt::Display for AppliedException {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} ({}) - {}",
            self.edge, self.tracking, self.reason
        )
    }
}
