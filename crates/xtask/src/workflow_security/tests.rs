use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use super::{WORKFLOWS, check, check_source, parse};

fn source(name: &str) -> &'static str {
    match name {
        "dependency-audit.yml" => {
            include_str!("../../../../.github/workflows/dependency-audit.yml")
        }
        "repository-quality.yml" => {
            include_str!("../../../../.github/workflows/repository-quality.yml")
        }
        "m08-durable-execution.yml" => {
            include_str!("../../../../.github/workflows/m08-durable-execution.yml")
        }
        "release-images.yml" => include_str!("../../../../.github/workflows/release-images.yml"),
        _ => "",
    }
}

fn reject(name: &str, before: &str, after: &str, reason: &str) {
    let original = source(name);
    assert!(
        original.contains(before),
        "fixture mutation must alter executable YAML"
    );
    let changed = original.replacen(before, after, 1);
    let problems = check_source(name, &changed);
    assert!(
        problems.iter().any(|problem| problem.contains(reason)),
        "expected {reason}, got {problems:?}"
    );
}

#[test]
fn all_reviewed_workflows_satisfy_the_semantic_contract() {
    for name in WORKFLOWS {
        assert_eq!(
            check_source(name, source(name)),
            Vec::<String>::new(),
            "{name}"
        );
    }
}

#[test]
fn root_and_job_permissions_cannot_expand_or_move_release_authority() {
    reject(
        "repository-quality.yml",
        "  contents: read",
        "  contents: write",
        "workflow.permissions",
    );
    reject(
        "repository-quality.yml",
        "permissions:\n  contents: read",
        "permissions: write-all",
        "workflow.permissions",
    );
    reject(
        "repository-quality.yml",
        "  verify:\n",
        "  verify:\n    permissions:\n      contents: read\n      id-token: write\n",
        "jobs.verify.permissions",
    );
    reject(
        "release-images.yml",
        "  plan:\n",
        "  plan:\n    permissions:\n      contents: read\n      packages: write\n      id-token: write\n",
        "jobs.plan.permissions",
    );
    reject(
        "release-images.yml",
        "      artifact-metadata: write",
        "      artifact-metadata: write\n      contents-extra: write",
        "jobs.publish.permissions",
    );
}

#[test]
fn untrusted_privileged_and_manual_triggers_and_path_skips_are_rejected() {
    reject(
        "repository-quality.yml",
        "  pull_request:",
        "  pull_request_target:",
        "workflow.on",
    );
    reject(
        "m08-durable-execution.yml",
        "  pull_request:",
        "  pull_request:\n  workflow_run:",
        "workflow.on",
    );
    reject(
        "release-images.yml",
        "  push:\n",
        "  workflow_dispatch:\n  push:\n",
        "workflow.on",
    );
    reject(
        "release-images.yml",
        "  push:\n",
        "  pull_request:\n  push:\n",
        "workflow.on",
    );
    reject(
        "repository-quality.yml",
        "    branches: [main]",
        "    branches: [main]\n    paths: ['crates/**']",
        "workflow.on",
    );
}

#[test]
fn every_checkout_and_every_remote_action_is_checked() {
    let name = "repository-quality.yml";
    let pin = "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1";
    reject(name, pin, "actions/checkout@v7", "exact full commit SHA");
    reject(
        name,
        pin,
        "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b",
        "exact full commit SHA",
    );
    reject(
        name,
        pin,
        "actions/checkout@0000000000000000000000000000000000000000",
        "exact full commit SHA",
    );
    reject(
        name,
        pin,
        "./.github/actions/checkout",
        "local, container, reusable",
    );
    let changed = source(name).replace("persist-credentials: false", "persist-credentials: true");
    let problems = check_source(name, &changed);
    assert_eq!(
        problems
            .iter()
            .filter(|problem| problem.contains("checkout must disable"))
            .count(),
        6
    );
    reject(
        name,
        "persist-credentials: false",
        "persist-credentials: false\n          token: ${{ secrets.CUSTOM_TOKEN }}",
        "checkout must disable",
    );
    reject(
        name,
        "persist-credentials: false",
        "persist-credentials: false\n          ref: ${{ github.event.pull_request.head.sha }}",
        "checkout must disable",
    );
    reject(
        name,
        "persist-credentials: false",
        "persist-credentials: false\n          allow-unsafe-pr-checkout: true",
        "checkout must disable",
    );
}

#[test]
fn pinned_text_in_comments_does_not_authorize_a_mutable_action() {
    reject(
        "repository-quality.yml",
        "uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1",
        "uses: actions/checkout@v7 # uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1",
        "exact full commit SHA",
    );
}

#[test]
fn skips_failure_suppression_and_command_strings_cannot_fake_a_gate() {
    reject(
        "repository-quality.yml",
        "  security:\n",
        "  security:\n    if: false\n",
        "unreviewed keys",
    );
    reject(
        "repository-quality.yml",
        "  security:\n",
        "  security:\n    continue-on-error: true\n",
        "unreviewed keys",
    );
    reject(
        "repository-quality.yml",
        "        run: cargo xtask verify\n",
        "        if: false\n        run: cargo xtask verify\n",
        "must not be conditional",
    );
    reject(
        "repository-quality.yml",
        "        run: cargo xtask verify\n",
        "        continue-on-error: true\n        run: cargo xtask verify\n",
        "unreviewed keys",
    );
    reject(
        "repository-quality.yml",
        "run: cargo xtask verify\n",
        "run: cargo xtask verify || true\n",
        "protected command",
    );
    reject(
        "repository-quality.yml",
        "run: cargo xtask verify\n",
        "run: echo 'cargo xtask verify'\n",
        "protected command",
    );
    reject(
        "repository-quality.yml",
        "run: cargo xtask verify\n",
        "run: cargo xtask verify --check repository-policy # cargo xtask verify\n",
        "protected command",
    );
}

#[test]
fn mandatory_security_gate_dependencies_and_full_history_cannot_be_removed() {
    reject(
        "repository-quality.yml",
        "needs: [architecture, security]",
        "needs: architecture",
        "dependency order",
    );
    reject(
        "repository-quality.yml",
        "fetch-depth: 0",
        "fetch-depth: 1",
        "complete reachable-history",
    );
    reject(
        "repository-quality.yml",
        "id: secret-scan",
        "id: optional-secret-scan",
        "protected IDs",
    );
    reject(
        "repository-quality.yml",
        "run: cargo xtask verify --check secret-scan\n",
        "run: cargo xtask verify --check secret-scan || true\n",
        "protected command",
    );
    reject(
        "m08-durable-execution.yml",
        "run: cargo xtask workflow-security",
        "run: echo 'cargo xtask workflow-security'",
        "protected command",
    );
}

#[test]
fn self_hosted_runners_unbounded_jobs_and_matrix_injection_are_rejected() {
    reject(
        "repository-quality.yml",
        "runs-on: ubuntu-latest",
        "runs-on: self-hosted",
        "GitHub-hosted runner",
    );
    reject(
        "repository-quality.yml",
        "timeout-minutes: 5",
        "timeout-minutes: 999",
        "literal integer",
    );
    reject(
        "repository-quality.yml",
        "os: [ubuntu-latest, windows-latest]",
        "os: [ubuntu-latest, self-hosted]",
        "finite OS matrix",
    );
    reject(
        "m08-durable-execution.yml",
        "          - flawless-proof",
        "          - flawless-proof\n          - 'x; arbitrary-shell'",
        "finite OS matrix",
    );
}

#[test]
fn shell_interpolation_and_credential_boundaries_are_not_free_form() {
    reject(
        "repository-quality.yml",
        "run: cargo xtask repository",
        "run: echo '${{ github.event.pull_request.title }}'",
        "unsafe or unreviewed expression",
    );
    reject(
        "repository-quality.yml",
        "run: cargo xtask repository",
        "run: echo '${{ github.head_ref }}'",
        "unsafe or unreviewed expression",
    );
    reject(
        "repository-quality.yml",
        "run: cargo xtask repository",
        "run: echo '${{ secrets.PRIVATE_KEY }}'",
        "credential expressions",
    );
    reject(
        "repository-quality.yml",
        "  contents: read",
        "  contents: read\nenv:\n  TOKEN: ${{ github.token }}",
        "credential expressions",
    );
    reject(
        "release-images.yml",
        "password: ${{ secrets.GITHUB_TOKEN }}",
        "password: ${{ secrets.RELEASE_PAT }}",
        "credential expressions",
    );
    reject(
        "release-images.yml",
        "GH_TOKEN: ${{ github.token }}",
        "GH_TOKEN: ${{ secrets.RELEASE_PAT }}",
        "credential expressions",
    );
    let sentinel = "NEVER_PRINT_CREDENTIAL_SENTINEL";
    let changed = source("repository-quality.yml").replace(
        "  contents: read",
        &format!("  contents: read\nenv:\n  TOKEN: '${{{{ secrets.{sentinel} }}}}'"),
    );
    assert!(
        !check_source("repository-quality.yml", &changed)
            .join("\n")
            .contains(sentinel)
    );
}

#[test]
fn diagnostics_and_cleanup_are_exact_exceptions_not_universal_skip_permission() {
    reject(
        "repository-quality.yml",
        "if: always()",
        "if: success()",
        "must not be conditional",
    );
    reject(
        "repository-quality.yml",
        "if: failure()",
        "if: always()",
        "must not be conditional",
    );
    reject(
        "repository-quality.yml",
        "down --volumes",
        "down --volumes || true",
        "must not be conditional",
    );
}

#[test]
fn environment_and_interpreter_overrides_cannot_redirect_literal_commands() {
    for key in ["PATH", "PYTHONPATH", "CARGO_ALIAS_XTASK", "GITLEAKS_CONFIG"] {
        reject(
            "repository-quality.yml",
            "        run: cargo xtask verify\n",
            &format!(
                "        env:\n          {key}: untrusted-redirect\n        run: cargo xtask verify\n"
            ),
            "redirection overrides",
        );
    }
    reject(
        "repository-quality.yml",
        "  verify:\n",
        "  verify:\n    env:\n      CARGO_ALIAS_XTASK: untrusted-redirect\n",
        "redirection overrides",
    );
    reject(
        "repository-quality.yml",
        "permissions:\n",
        "env:\n  PYTHONPATH: untrusted-redirect\npermissions:\n",
        "redirection overrides",
    );
    reject(
        "repository-quality.yml",
        "        run: cargo xtask verify\n",
        "        shell: custom-interpreter\n        run: cargo xtask verify\n",
        "unreviewed keys",
    );
    reject(
        "repository-quality.yml",
        "        run: cargo xtask verify\n",
        "        working-directory: elsewhere\n        run: cargo xtask verify\n",
        "unreviewed keys",
    );
    reject(
        "repository-quality.yml",
        "  verify:\n",
        "  verify:\n    defaults:\n      run:\n        working-directory: elsewhere\n",
        "unreviewed keys",
    );
}

#[test]
fn release_capabilities_remain_bound_to_build_digests_and_verified_evidence() {
    reject(
        "release-images.yml",
        "subject-digest: ${{ steps.build.outputs.digest }}",
        "subject-digest: sha256:untrusted",
        "protected build digest",
    );
    reject(
        "release-images.yml",
        "push-to-registry: true",
        "push-to-registry: false # push-to-registry: true",
        "attach evidence",
    );
    reject(
        "release-images.yml",
        "run: cosign sign --yes",
        "run: echo cosign sign --yes",
        "protected release command",
    );
    reject(
        "release-images.yml",
        "          --certificate-oidc-issuer \"${OIDC_ISSUER}\"",
        "          --certificate-oidc-issuer \"untrusted-issuer\"",
        "protected release command",
    );
    reject(
        "release-images.yml",
        "DIGEST: ${{ steps.build.outputs.digest }}",
        "DIGEST: digest-not-produced-by-build",
        "exact reviewed environment",
    );
    reject(
        "release-images.yml",
        "          push: true",
        "          push: false # push: true",
        "publication/evidence policy",
    );
    reject(
        "release-images.yml",
        "--predicate-type https://slsa.dev/provenance/v1",
        "--predicate-type untrusted-predicate",
        "protected release command",
    );
    reject(
        "release-images.yml",
        "matrix: ${{ fromJSON(needs.plan.outputs.matrix) }}",
        "matrix: ${{ fromJSON('{}') }}",
        "protected planner job",
    );
}

#[test]
fn comments_and_quoted_strings_cannot_replace_executable_supply_chain_evidence() {
    reject(
        "repository-quality.yml",
        "run: python scripts/verify_supply_chain.py --artifacts artifacts/sbom",
        "run: echo 'python scripts/verify_supply_chain.py --artifacts artifacts/sbom'",
        "artifact validation commands",
    );
    reject(
        "repository-quality.yml",
        "          command: check",
        "          command: '--help' # command: check",
        "actual locked policy check",
    );
    reject(
        "repository-quality.yml",
        "          if-no-files-found: error",
        "          if-no-files-found: warn # if-no-files-found: error",
        "fail-closed missing-artifact",
    );
    reject(
        "m08-durable-execution.yml",
        "          arguments: --locked --workspace --config experiments/deny.toml",
        "          arguments: '--help' # arguments: --locked --workspace --config experiments/deny.toml",
        "actual locked policy check",
    );
}

#[test]
fn scheduled_audit_requires_daily_detection_and_has_no_privileged_or_input_trigger() {
    let name = "dependency-audit.yml";
    reject(
        name,
        "  schedule:\n    - cron: '27 3 * * *'\n",
        "",
        "workflow.on",
    );
    reject(name, "27 3 * * *", "27 3 * * 1", "workflow.on");
    reject(
        name,
        "  workflow_dispatch:\n",
        "  workflow_dispatch:\n    inputs:\n      command:\n        type: string\n",
        "workflow.on",
    );
    for trigger in [
        "push",
        "pull_request",
        "pull_request_target",
        "workflow_run",
    ] {
        reject(
            name,
            "  workflow_dispatch:\n",
            &format!("  {trigger}:\n"),
            "workflow.on",
        );
    }
    let no_manual = source(name).replace("  workflow_dispatch:\n", "");
    assert!(check_source(name, &no_manual).is_empty());
}

#[test]
fn scheduled_audit_matrix_cannot_skip_duplicate_or_redirect_lock_roots() {
    let name = "dependency-audit.yml";
    reject(
        name,
        "          - workspace: flawless-proof\n            manifest: experiments/flawless-proof/Cargo.toml\n            policy: experiments/deny.toml\n",
        "",
        "finite OS matrix",
    );
    reject(
        name,
        "manifest: experiments/dbos-proof/Cargo.toml",
        "manifest: experiments/temporal-proof/Cargo.toml",
        "finite OS matrix",
    );
    reject(
        name,
        "policy: deny.toml",
        "policy: experiments/deny.toml",
        "finite OS matrix",
    );
    reject(
        name,
        "workspace: production",
        "workspace: dbos-proof",
        "finite OS matrix",
    );
    reject(
        name,
        "fail-fast: false",
        "fail-fast: true",
        "finite OS matrix",
    );
    reject(
        name,
        "      matrix:\n",
        "      matrix:\n        exclude: []\n",
        "finite OS matrix",
    );
}

#[test]
fn scheduled_audit_retains_readonly_permissions_credentials_pins_and_inputs() {
    let name = "dependency-audit.yml";
    reject(
        name,
        "  contents: read",
        "  contents: write",
        "workflow.permissions",
    );
    reject(
        name,
        "  audit:\n",
        "  audit:\n    permissions:\n      contents: read\n      packages: write\n",
        "jobs.audit.permissions",
    );
    reject(
        name,
        "persist-credentials: false",
        "persist-credentials: true",
        "checkout must disable",
    );
    reject(
        name,
        "EmbarkStudios/cargo-deny-action@3c6349835b2b7b196a839186cb8b78e02f7b5f25",
        "EmbarkStudios/cargo-deny-action@v2",
        "exact full commit SHA",
    );
    reject(
        name,
        "          command: check",
        "          command: check\n          token: ${{ secrets.GITHUB_TOKEN }}",
        "exact reviewed input",
    );
    reject(
        name,
        "manifest-path: ${{ matrix.manifest }}",
        "manifest-path: Cargo.toml",
        "exact reviewed input",
    );
    reject(
        name,
        "--locked --workspace --config ${{ matrix.policy }}",
        "--workspace --config ${{ matrix.policy }}",
        "actual locked policy check",
    );
    reject(
        name,
        "--locked --workspace --config ${{ matrix.policy }}",
        "--locked --workspace --config experiments/deny.toml",
        "actual locked policy check",
    );
    reject(
        name,
        "command: check",
        "command: check advisories",
        "actual locked policy check",
    );
}

#[test]
fn scheduled_audit_gates_cannot_be_removed_skipped_suppressed_or_reordered()
-> Result<(), Box<dyn std::error::Error>> {
    let name = "dependency-audit.yml";
    reject(name, "    needs: architecture\n", "", "dependency order");
    reject(
        name,
        "  audit:\n",
        "  audit:\n    if: false\n",
        "unreviewed keys",
    );
    reject(
        name,
        "      - name: Audit locked dependencies and licenses\n",
        "      - name: Audit locked dependencies and licenses\n        if: false\n",
        "must not be conditional",
    );
    reject(
        name,
        "      - name: Audit locked dependencies and licenses\n",
        "      - name: Audit locked dependencies and licenses\n        continue-on-error: true\n",
        "unreviewed keys",
    );
    reject(
        name,
        "run: cargo xtask workflow-security",
        "run: echo 'cargo xtask workflow-security'",
        "protected command",
    );
    reject(
        name,
        "run: cargo xtask verify --check repository-policy --check architecture-graph",
        "run: cargo xtask verify --check repository-policy --check architecture-graph || true",
        "protected command",
    );
    reject(
        name,
        "    timeout-minutes: 15",
        "    timeout-minutes: 60",
        "bounded, read-only bootstrap",
    );

    let original = source(name);
    let audit_index = original
        .find("      - name: Audit locked dependencies and licenses\n")
        .ok_or("audit fixture missing")?;
    let audit = &original[audit_index..];
    let removed = original.replace(audit, "");
    assert!(
        check_source(name, &removed)
            .iter()
            .any(|problem| problem.contains("actual locked policy check"))
    );
    let duplicated = format!("{original}{audit}");
    assert!(
        check_source(name, &duplicated)
            .iter()
            .any(|problem| problem.contains("actual locked policy check"))
    );
    let checkout_index = original[..audit_index]
        .rfind("      - name: Check out repository\n")
        .ok_or("audit checkout fixture missing")?;
    let checkout = &original[checkout_index..audit_index];
    let reordered = format!("{}{audit}{checkout}", &original[..checkout_index]);
    assert!(
        check_source(name, &reordered)
            .iter()
            .any(|problem| problem.contains("checkout must precede"))
    );
    Ok(())
}

#[test]
fn scheduled_audit_cannot_become_a_build_or_publication_workflow() {
    let name = "dependency-audit.yml";
    reject(
        name,
        "run: rustup toolchain install 1.98.1 --profile minimal",
        "run: cargo build --workspace",
        "only the reviewed security bootstrap",
    );
    reject(
        name,
        "  audit:\n",
        "  audit:\n    outputs:\n      result: arbitrary\n",
        "bounded, read-only bootstrap",
    );
    reject(
        name,
        "      - name: Audit locked dependencies and licenses\n",
        "      - name: Build images\n        run: docker build .\n      - name: Audit locked dependencies and licenses\n",
        "only the reviewed security bootstrap",
    );
    reject(
        name,
        "EmbarkStudios/cargo-deny-action@3c6349835b2b7b196a839186cb8b78e02f7b5f25",
        "docker/login-action@dbcb813823bdd20940b903addbd779551569679f",
        "exact full commit SHA",
    );
}

#[test]
fn serialized_credential_contexts_cannot_hide_in_pinned_action_inputs() {
    for expression in [
        "${{ toJSON(secrets) }}",
        "${{ toJSON(github) }}",
        "${{ github[format('to{0}', 'ken')] }}",
        "${{ format('}}{0}', toJSON(secrets)) }}",
    ] {
        reject(
            "repository-quality.yml",
            "          python-version: \"3.13\"",
            &format!("          python-version: \"3.13\"\n          token: {expression}"),
            "exact reviewed input",
        );
        reject(
            "repository-quality.yml",
            "name: Repository quality",
            &format!("name: \"{expression}\""),
            "credential expressions",
        );
    }
    reject(
        "repository-quality.yml",
        "          python-version: \"3.13\"",
        "          python-version: ${{ toJSON(secrets) }}",
        "exact reviewed input",
    );
}

#[test]
fn malformed_duplicate_ambiguous_and_resource_unbounded_yaml_fails_closed() {
    for source in [
        "on: {}\non: {}\n",
        "permissions: {contents: read, contents: write}\n",
        "x: &policy {contents: read}\npermissions: *policy\n",
        "permissions: !custom {contents: read}\n",
        "permissions: {!!str contents: read}\n",
        "permissions: {<<: {contents: read}}\n",
        "---\non: {}\n---\non: {}\n",
        "[workflow, is, not, a, mapping]",
        "jobs: [",
        "",
    ] {
        assert!(
            parse::workflow(source).is_err(),
            "ambiguous fixture must fail closed"
        );
    }
    assert!(parse::workflow(&format!("x: {}", "[".repeat(65))).is_err());
    assert!(parse::workflow(&" ".repeat(512 * 1024 + 1)).is_err());
    assert!(parse::workflow("'on': {push: {branches: [main]}, pull_request: null}").is_ok());
}

#[test]
fn yaml_parse_diagnostics_never_echo_untrusted_values() {
    let sentinel = "PRIVATE_VALUE_SENTINEL";
    let source = format!("{sentinel}: 1\n{sentinel}: 2\n");
    let problems = check_source("repository-quality.yml", &source);
    assert!(!problems.is_empty());
    assert!(!problems.join("\n").contains(sentinel));
}

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "edgeagent-workflow-security-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root)?;
        fs::create_dir_all(root.join(".github/workflows"))?;
        for name in WORKFLOWS {
            fs::write(root.join(".github/workflows").join(name), source(name))?;
        }
        fs::write(
            root.join(".github/dependabot.yml"),
            include_str!("../../../../.github/dependabot.yml"),
        )?;
        fs::write(root.join("Cargo.toml"), "[workspace]\n")?;
        fs::write(root.join("Cargo.lock"), "version = 4\n")?;
        for proof in [
            "event-spine-proof",
            "durable-execution",
            "dbos-proof",
            "temporal-proof",
            "flawless-proof",
        ] {
            let directory = root.join("experiments").join(proof);
            fs::create_dir_all(&directory)?;
            fs::write(directory.join("Cargo.toml"), "[workspace]\n")?;
            fs::write(directory.join("Cargo.lock"), "version = 4\n")?;
        }
        Ok(Self(root))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Only remove the exact, exclusively created fixture directory, never a workspace root.
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn missing_and_additional_workflows_cannot_escape_inventory()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    assert_eq!(check(&fixture.0), Ok(()));
    fs::write(
        fixture.0.join(".github/workflows/unreviewed.yaml"),
        "on: pull_request\n",
    )?;
    let problems = check(&fixture.0)
        .err()
        .ok_or("new workflow unexpectedly authorized")?;
    assert!(
        problems
            .iter()
            .any(|problem| problem.contains("explicit security policy"))
    );
    fs::remove_file(
        fixture
            .0
            .join(".github/workflows/m08-durable-execution.yml"),
    )?;
    let problems = check(&fixture.0)
        .err()
        .ok_or("missing workflow unexpectedly authorized")?;
    assert!(
        problems
            .iter()
            .any(|problem| problem.contains("required reviewed workflow is missing"))
    );
    Ok(())
}
