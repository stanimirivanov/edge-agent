use std::error::Error;

use super::*;

#[test]
fn current_workspace_matches_the_declared_policy() -> Result<(), Box<dyn Error>> {
    let metadata = load_metadata()?;
    let graph = graph_from_metadata(&metadata)?;

    validate_graph(&graph)?;
    assert!(
        graph
            .dependencies
            .iter()
            .all(|edge| graph.packages.contains(&edge.dependency)),
        "only internal workspace dependencies belong in the architecture graph"
    );
    Ok(())
}

#[test]
fn renamed_dependency_alias_resolves_to_the_workspace_package() {
    let package_roots = BTreeMap::from([(
        "C:/workspace/crates/contracts".to_owned(),
        "edgeagent-contracts".to_owned(),
    )]);
    let manifest_alias = "contracts-under-an-alias";

    let resolved = internal_dependency_name(Some("C:/workspace/crates/contracts"), &package_roots);

    assert_ne!(resolved, Some(manifest_alias));
    assert_eq!(resolved, Some("edgeagent-contracts"));
}

#[test]
fn unapproved_production_dependency_is_rejected() -> Result<(), String> {
    let mut graph = declared_graph();
    graph.dependencies.insert(DependencyEdge::new(
        "edgeagent-contracts",
        "edgeagent-telemetry",
        DependencyClass::Normal,
    ));

    let violations = policy_violations(validate_graph(&graph))?;

    assert_eq!(
        violations,
        vec![
            "unapproved dependency from contract package: \
             edgeagent-contracts -[normal]-> edgeagent-telemetry"
                .to_owned()
        ]
    );
    Ok(())
}

#[test]
fn development_dependency_does_not_authorize_a_production_edge() -> Result<(), String> {
    let mut graph = declared_graph();
    graph.dependencies.insert(DependencyEdge::new(
        "edgeagent-inbox-handler",
        "edgeagent-inbox-postgres",
        DependencyClass::Normal,
    ));

    let violations = policy_violations(validate_graph(&graph))?;

    assert_eq!(
        violations,
        vec![
            "unapproved dependency from application package: \
             edgeagent-inbox-handler -[normal]-> edgeagent-inbox-postgres"
                .to_owned()
        ]
    );
    Ok(())
}

#[test]
fn unapproved_development_dependency_is_rejected() -> Result<(), String> {
    let mut graph = declared_graph();
    graph.dependencies.insert(DependencyEdge::new(
        "edgeagent-messaging",
        "edgeagent-telemetry",
        DependencyClass::Development,
    ));

    let violations = policy_violations(validate_graph(&graph))?;

    assert_eq!(
        violations,
        vec![
            "unapproved dependency from port package: \
             edgeagent-messaging -[development]-> edgeagent-telemetry"
                .to_owned()
        ]
    );
    Ok(())
}

#[test]
fn unclassified_workspace_package_is_rejected() -> Result<(), String> {
    let mut graph = declared_graph();
    graph.packages.insert("edgeagent-new-capability".to_owned());

    let violations = policy_violations(validate_graph(&graph))?;

    assert_eq!(
        violations,
        vec!["workspace package `edgeagent-new-capability` has no architecture policy".to_owned()]
    );
    Ok(())
}

#[test]
fn removed_dependency_makes_its_permanent_rule_stale() -> Result<(), String> {
    let mut graph = declared_graph();
    graph.dependencies.remove(&DependencyEdge::new(
        "edgeagent-gateway",
        "edgeagent-service-runtime",
        DependencyClass::Normal,
    ));

    let violations = policy_violations(validate_graph(&graph))?;

    assert_eq!(
        violations,
        vec![
            "stale dependency rule: \
             edgeagent-gateway -[normal]-> edgeagent-service-runtime"
                .to_owned()
        ]
    );
    Ok(())
}

#[test]
fn stale_package_policy_is_rejected_with_its_role() -> Result<(), String> {
    let mut graph = declared_graph();
    graph.packages.remove("edgeagent-service-runtime");

    let violations = policy_violations(validate_graph(&graph))?;

    assert_eq!(
        violations,
        vec![
            "stale architecture policy for missing runtime support package \
             `edgeagent-service-runtime`"
                .to_owned()
        ]
    );
    Ok(())
}

#[test]
fn declared_policy_is_consistent_and_acyclic() {
    assert_eq!(validate_policy(), Vec::<String>::new());
}

#[test]
fn production_cycle_includes_build_edges() {
    let packages = BTreeSet::from(["a", "b", "c"]);
    let edges = BTreeSet::from([
        DependencyEdge::new("a", "b", DependencyClass::Normal),
        DependencyEdge::new("b", "c", DependencyClass::Build),
        DependencyEdge::new("c", "a", DependencyClass::Normal),
    ]);

    assert!(has_dependency_cycle(&packages, &edges));
}

#[test]
fn development_edges_do_not_form_a_production_cycle() {
    let packages = BTreeSet::from(["a", "b"]);
    let all_edges = BTreeSet::from([
        DependencyEdge::new("a", "b", DependencyClass::Normal),
        DependencyEdge::new("b", "a", DependencyClass::Development),
    ]);
    let production_edges = all_edges
        .into_iter()
        .filter(DependencyEdge::is_production)
        .collect();

    assert!(!has_dependency_cycle(&packages, &production_edges));
}

fn declared_graph() -> WorkspaceGraph {
    let packages = PACKAGE_POLICIES
        .iter()
        .map(|package| package.name.to_owned())
        .collect();
    let dependencies = PACKAGE_POLICIES
        .iter()
        .flat_map(PackagePolicy::dependencies)
        .chain(
            TEMPORARY_EXCEPTIONS
                .iter()
                .map(super::model::TemporaryException::edge),
        )
        .collect();

    WorkspaceGraph {
        packages,
        dependencies,
    }
}

fn policy_violations(
    result: Result<ArchitectureReport, ArchitectureCheckError>,
) -> Result<Vec<String>, String> {
    match result {
        Err(ArchitectureCheckError::Policy(violations)) => Ok(violations),
        Err(ArchitectureCheckError::Metadata(error)) => {
            Err(format!("unexpected metadata failure: {error}"))
        }
        Ok(_) => Err("graph should violate policy".to_owned()),
    }
}
