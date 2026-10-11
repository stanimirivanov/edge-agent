use std::{
    fs, io,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

use super::*;

static FIXTURE_ID: AtomicUsize = AtomicUsize::new(0);

#[test]
fn cargo_normalizes_renamed_optional_inactive_workspace_normal_dependency()
-> Result<(), Box<dyn Error>> {
    let edge = DependencyEdge::new(
        "edgeagent-messaging",
        "edgeagent-contracts",
        DependencyClass::Normal,
    );
    let graph = fixture_graph(&edge)?;

    assert_eq!(graph, declared_graph());
    validate_graph(&graph)?;
    Ok(())
}

#[test]
fn cargo_normalizes_renamed_inactive_workspace_development_dependency() -> Result<(), Box<dyn Error>>
{
    let edge = DependencyEdge::new(
        "edgeagent-inbox-handler",
        "edgeagent-inbox-postgres",
        DependencyClass::Development,
    );
    let graph = fixture_graph(&edge)?;

    assert_eq!(graph, declared_graph());
    validate_graph(&graph)?;
    Ok(())
}

#[test]
fn cargo_target_normal_dependency_cannot_reuse_development_allowance() -> Result<(), Box<dyn Error>>
{
    let edge = DependencyEdge::new(
        "edgeagent-inbox-handler",
        "edgeagent-inbox-postgres",
        DependencyClass::Normal,
    );

    assert_hidden_edge_is_rejected(
        &edge,
        "unapproved dependency from application package: \
         edgeagent-inbox-handler -[normal]-> edgeagent-inbox-postgres",
    )
}

#[test]
fn cargo_target_build_dependency_cannot_reuse_development_allowance() -> Result<(), Box<dyn Error>>
{
    let edge = DependencyEdge::new(
        "edgeagent-inbox-handler",
        "edgeagent-inbox-postgres",
        DependencyClass::Build,
    );

    assert_hidden_edge_is_rejected(
        &edge,
        "unapproved dependency from application package: \
         edgeagent-inbox-handler -[build]-> edgeagent-inbox-postgres",
    )
}

#[test]
fn cargo_unapproved_inactive_workspace_development_dependency_is_rejected()
-> Result<(), Box<dyn Error>> {
    let edge = DependencyEdge::new(
        "edgeagent-messaging",
        "edgeagent-telemetry",
        DependencyClass::Development,
    );

    assert_hidden_edge_is_rejected(
        &edge,
        "unapproved dependency from port package: \
         edgeagent-messaging -[development]-> edgeagent-telemetry",
    )
}

#[test]
fn cargo_unapproved_optional_inactive_workspace_normal_dependency_is_rejected()
-> Result<(), Box<dyn Error>> {
    let edge = DependencyEdge::new(
        "edgeagent-contracts",
        "edgeagent-telemetry",
        DependencyClass::Normal,
    );

    assert_hidden_edge_is_rejected(
        &edge,
        "unapproved dependency from contract package: \
         edgeagent-contracts -[normal]-> edgeagent-telemetry",
    )
}

fn assert_hidden_edge_is_rejected(
    edge: &DependencyEdge,
    expected_diagnostic: &str,
) -> Result<(), Box<dyn Error>> {
    let graph = fixture_graph(edge)?;
    let mut expected_graph = declared_graph();
    expected_graph.dependencies.insert(edge.clone());

    assert_eq!(graph, expected_graph);
    assert_eq!(
        policy_violations(validate_graph(&graph))?,
        vec![expected_diagnostic.to_owned()]
    );
    Ok(())
}

fn fixture_graph(edge: &DependencyEdge) -> Result<WorkspaceGraph, Box<dyn Error>> {
    let fixture = CargoWorkspaceFixture::new(edge)?;
    let metadata = MetadataCommand::new()
        .current_dir(&fixture.root)
        .manifest_path(fixture.root.join("Cargo.toml"))
        .no_deps()
        .other_options(vec!["--offline".to_owned(), "--locked".to_owned()])
        .exec()?;
    let package = metadata
        .workspace_packages()
        .into_iter()
        .find(|package| package.name.as_str() == edge.package)
        .ok_or_else(|| io::Error::other("fixture source package missing from Cargo metadata"))?;
    let dependency = package
        .dependencies
        .iter()
        .find(|dependency| {
            dependency.rename.as_deref() == Some("aliased_internal")
                && dependency_class(dependency.kind) == Ok(edge.class)
        })
        .ok_or_else(|| io::Error::other("fixture alias missing from Cargo metadata"))?;

    // Assert Cargo really read the intended declaration rather than constructing
    // another handcrafted graph that accidentally drops its target or kind.
    assert_eq!(dependency.name, edge.dependency);
    assert_eq!(dependency_class(dependency.kind)?, edge.class);
    assert_eq!(
        dependency.optional,
        edge.class != DependencyClass::Development
    );
    assert_eq!(
        dependency.target.as_ref().map(ToString::to_string),
        Some("cfg(any())".to_owned())
    );
    assert!(dependency.path.is_some());

    Ok(graph_from_metadata(&metadata)?)
}

struct CargoWorkspaceFixture {
    root: PathBuf,
}

impl CargoWorkspaceFixture {
    fn new(changed_edge: &DependencyEdge) -> io::Result<Self> {
        let fixture = Self::create()?;
        let members = PACKAGE_POLICIES
            .iter()
            .map(|policy| format!("\"{}\"", policy.name))
            .collect::<Vec<_>>()
            .join(", ");
        fs::write(
            fixture.root.join("Cargo.toml"),
            format!(
                "[workspace]\nresolver = \"3\"\nmembers = [{members}]\n\n\
                 [workspace.dependencies]\n\
                 aliased_internal = {{ package = \"{dependency}\", path = \"{dependency}\" }}\n",
                dependency = changed_edge.dependency,
            ),
        )?;

        for policy in PACKAGE_POLICIES {
            let directory = fixture.root.join(policy.name);
            fs::create_dir_all(directory.join("src"))?;
            let mut manifest = format!(
                "[package]\nname = \"{}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
                policy.name,
            );
            for edge in policy.dependencies() {
                if edge != *changed_edge {
                    if policy.name == changed_edge.package
                        && edge.dependency == changed_edge.dependency
                    {
                        // Keep the legitimate dev edge under the same alias so
                        // Cargo can expose distinct kinds without duplicate names.
                        manifest.push_str(&format!(
                            "\n[{}.aliased_internal]\nworkspace = true\n",
                            dependency_table(edge.class),
                        ));
                    } else {
                        manifest.push_str(&format!(
                            "\n[{}.{}]\npath = \"../{}\"\n",
                            dependency_table(edge.class),
                            edge.dependency,
                            edge.dependency,
                        ));
                    }
                }
            }
            if policy.name == changed_edge.package {
                manifest.push_str(&format!(
                    "\n[target.'cfg(any())'.{}.aliased_internal]\nworkspace = true\n",
                    dependency_table(changed_edge.class),
                ));
                // Cargo rejects optional development dependencies; the other
                // kinds deliberately exercise an unactivated optional feature.
                if changed_edge.class != DependencyClass::Development {
                    manifest.push_str("optional = true\n");
                }
            }
            fs::write(directory.join("Cargo.toml"), manifest)?;
            fs::write(
                directory.join("src/lib.rs"),
                "// Synthetic metadata fixture.\n",
            )?;
        }
        Ok(fixture)
    }

    fn create() -> io::Result<Self> {
        // create_dir, not create_dir_all, establishes ownership before cleanup.
        // A stale directory from another process is never reused or removed.
        for _ in 0..64 {
            let id = FIXTURE_ID.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "edgeagent-cargo-architecture-{}-{id}",
                std::process::id(),
            ));
            match fs::create_dir(&root) {
                Ok(()) => return Ok(Self { root }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::other(
            "could not create an isolated Cargo fixture",
        ))
    }
}

impl Drop for CargoWorkspaceFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

const fn dependency_table(class: DependencyClass) -> &'static str {
    match class {
        DependencyClass::Normal => "dependencies",
        DependencyClass::Development => "dev-dependencies",
        DependencyClass::Build => "build-dependencies",
    }
}
