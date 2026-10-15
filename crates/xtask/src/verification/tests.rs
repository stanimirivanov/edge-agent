use std::{collections::VecDeque, io};

use super::{
    Check, CommandSpec, Discovery, Program, Requirement, inventory,
    runner::{self, Execution, Executor, Options, Status},
};

fn fixture() -> Check {
    Check {
        id: "fixture",
        tier: "T3",
        profiles: &["local-platform"],
        command: CommandSpec {
            program: Program::Cargo,
            args: vec![
                "test",
                "--test",
                "proof",
                "--",
                "--ignored",
                "--exact",
                "required_test",
            ],
        },
        prerequisites: &[],
        discovery: Some(Discovery {
            args: vec!["test", "--test", "proof", "--", "--ignored", "--list"],
            expected: vec!["required_test"],
            ignored: true,
        }),
        evidence: "fixture only",
    }
}

struct Fake {
    commands: Vec<(Vec<&'static str>, bool)>,
    outputs: VecDeque<io::Result<Execution>>,
    prerequisite: Result<(), String>,
}

impl Fake {
    fn new(outputs: Vec<io::Result<Execution>>) -> Self {
        Self {
            commands: Vec::new(),
            outputs: outputs.into(),
            prerequisite: Ok(()),
        }
    }
}

impl Executor for Fake {
    fn prerequisite(&mut self, _: Requirement) -> Result<(), String> {
        self.prerequisite.clone()
    }
    fn execute(&mut self, command: &CommandSpec, capture: bool) -> io::Result<Execution> {
        self.commands.push((command.args.clone(), capture));
        self.outputs
            .pop_front()
            .ok_or_else(|| io::Error::other("unexpected execution"))?
    }
}

fn output(success: bool, stdout: &str) -> io::Result<Execution> {
    Ok(Execution {
        success,
        stdout: stdout.as_bytes().to_vec(),
    })
}

#[test]
fn inventory_and_default_acceptance_are_complete_and_credential_free() -> Result<(), String> {
    let checks = inventory::checks();
    inventory::validate(&checks)?;
    let options = Options::parse(&[])?;
    let selected = options.select(&checks)?;
    assert_eq!(
        selected.iter().map(|c| c.id).collect::<Vec<_>>(),
        [
            "text-format",
            "rust-format",
            "repository-bytes",
            "repository-policy",
            "architecture-declarations",
            "workflow-security",
            "secret-policy",
            "architecture-graph",
            "image-contracts",
            "local-stack-contracts",
            "supply-chain-contracts",
            "release-contracts",
            "locked-graph",
            "rust-check",
            "rust-clippy",
            "sqlx-query-policy",
            "harness-tests",
            "ignored-inventory",
            "rust-tests",
            "rust-doctests",
            "messaging-error-contracts",
            "contracts-preserve-order",
        ]
    );
    assert!(
        selected
            .iter()
            .all(|c| c.tier != "T3" && c.prerequisites.is_empty())
    );
    assert!(selected.iter().any(|c| c.command.args.contains(&"--doc")));
    assert!(
        selected
            .iter()
            .any(|c| c.command.args.contains(&"serde_json/preserve_order"))
    );
    Ok(())
}

#[test]
fn dependency_audit_covers_each_isolated_lock_without_runtime_or_sbom_work() -> Result<(), String> {
    let checks = inventory::checks();
    let options = Options::parse(&["dependency-audit".to_owned()])?;
    let selected = options.select(&checks)?;
    assert_eq!(
        selected.iter().map(|check| check.id).collect::<Vec<_>>(),
        [
            "workflow-security",
            "supply-chain-contracts",
            "dependency-policy",
            "m08-event-spine-dependency-policy",
            "m08-duroxide-dependency-policy",
            "m08-dbos-dependency-policy",
            "m08-temporal-dependency-policy",
            "m08-flawless-dependency-policy",
        ]
    );
    let audits: Vec<_> = selected
        .into_iter()
        .filter(|check| check.id.ends_with("dependency-policy"))
        .collect();
    assert_eq!(audits.len(), 6);
    for audit in audits {
        assert_eq!(audit.command.program, Program::Cargo);
        assert_eq!(audit.command.args.first(), Some(&"deny"));
        assert!(audit.command.args.contains(&"--locked"));
        assert!(matches!(
            audit.prerequisites,
            [Requirement::Executable("cargo-deny")]
        ));
        assert!(audit.discovery.is_none());
        if audit.id != "dependency-policy" {
            assert!(audit.command.args.contains(&"--workspace"));
            assert!(audit.command.args.contains(&"experiments/deny.toml"));
            assert!(audit.command.args.contains(&"--manifest-path"));
        }
    }
    Ok(())
}

#[test]
fn security_profile_requires_actual_scanner_protocol_and_default_does_not_install_it()
-> Result<(), String> {
    let checks = inventory::checks();
    let options = Options::parse(&["ci-security".to_owned()])?;
    let selected = options.select(&checks)?;
    assert_eq!(
        selected.iter().map(|check| check.id).collect::<Vec<_>>(),
        [
            "workflow-security",
            "secret-policy",
            "secret-scanner-contracts",
            "secret-scan"
        ]
    );
    for check in selected.iter().filter(|check| check.tier == "T3") {
        assert!(matches!(
            check.prerequisites,
            [Requirement::CommandProbe("gitleaks", ["version"])]
        ));
        assert_eq!(check.command.program, Program::Python);
        assert_eq!(
            check.command.args.first(),
            Some(&"scripts/verify_secrets.py")
        );
    }
    Ok(())
}

#[test]
fn unknown_empty_ambiguous_and_duplicate_selections_fail_closed() -> Result<(), String> {
    let checks = inventory::checks();
    for args in [
        vec!["unknown"],
        vec!["--python"],
        vec!["--check", ""],
        vec!["verify", "--check", "rust-tests"],
        vec!["--other"],
    ] {
        assert!(Options::parse(&args.into_iter().map(str::to_owned).collect::<Vec<_>>()).is_err());
    }
    for args in [
        vec!["--check", "renamed-test"],
        vec!["--check", "rust-tests", "--check", "rust-tests"],
    ] {
        assert!(
            Options::parse(&args.into_iter().map(str::to_owned).collect::<Vec<_>>())?
                .select(&checks)
                .is_err()
        );
    }
    Ok(())
}

#[test]
fn libtest_census_rejects_zero_renamed_added_duplicate_and_malformed_output() -> Result<(), String>
{
    let discovery = fixture().discovery.ok_or("fixture missing discovery")?;
    runner::verify_discovery(
        &discovery,
        b"required_test: test\r\n\r\n1 test, 0 benchmarks\r\n",
    )?;
    for stdout in [
        b"0 tests, 0 benchmarks".as_slice(),
        b"renamed_test: test",
        b"required_test: test\nnew_ignored: test",
        b"required_test: test\nrequired_test: test",
        b"required_test: benchmark",
        b"required_test: test\xff",
    ] {
        assert!(runner::verify_discovery(&discovery, stdout).is_err());
    }
    let not_ignored = Discovery {
        ignored: false,
        ..discovery
    };
    assert!(runner::verify_discovery(&not_ignored, b"required_test: test").is_err());
    Ok(())
}

#[test]
fn successful_discovery_precedes_actual_proof_execution() {
    let check = fixture();
    let mut executor = Fake::new(vec![
        output(true, "required_test: test\n"),
        output(true, ""),
    ]);
    let outcomes = runner::execute(&[&check], &mut executor);
    assert_eq!(outcomes[0].status, Status::Passed);
    assert_eq!(executor.commands.len(), 2);
    assert!(executor.commands[0].1);
    assert!(!executor.commands[1].1);
    assert!(executor.commands[0].0.contains(&"--list"));
    assert!(executor.commands[1].0.contains(&"--exact"));
}

#[test]
fn missing_prerequisite_is_not_run_and_blocks_later_effects() {
    let mut check = fixture();
    check.prerequisites = &[Requirement::Environment("SYNTHETIC_SERVICE_URL")];
    let next = fixture();
    let mut executor = Fake::new(Vec::new());
    executor.prerequisite = Err("requires SYNTHETIC_SERVICE_URL".to_owned());
    let outcomes = runner::execute(&[&check, &next], &mut executor);
    assert_eq!(
        outcomes[0].status,
        Status::NotRun("requires SYNTHETIC_SERVICE_URL".to_owned())
    );
    assert!(matches!(outcomes[1].status, Status::NotRun(_)));
    assert!(executor.commands.is_empty());
}

#[test]
fn discovery_failure_cannot_launch_proof_or_later_check() {
    for response in [
        output(true, "0 tests"),
        output(false, "required_test: test"),
        Err(io::Error::new(io::ErrorKind::NotFound, "no cargo")),
    ] {
        let check = fixture();
        let next = fixture();
        let mut executor = Fake::new(vec![response]);
        let outcomes = runner::execute(&[&check, &next], &mut executor);
        assert_ne!(outcomes[0].status, Status::Passed);
        assert!(matches!(outcomes[1].status, Status::NotRun(_)));
        assert_eq!(executor.commands.len(), 1);
    }
}

#[test]
fn actual_command_failure_and_spawn_failure_never_pass() {
    for response in [
        output(false, ""),
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "missing executable",
        )),
    ] {
        let mut check = fixture();
        check.discovery = None;
        let mut executor = Fake::new(vec![response]);
        let outcomes = runner::execute(&[&check], &mut executor);
        assert_ne!(outcomes[0].status, Status::Passed);
    }
}

#[test]
fn census_runs_once_and_claims_only_discovery() {
    let mut check = fixture();
    check.id = "ignored-inventory";
    let mut executor = Fake::new(vec![output(true, "required_test: test")]);
    let outcomes = runner::execute(&[&check], &mut executor);
    assert_eq!(outcomes[0].status, Status::Passed);
    assert_eq!(executor.commands.len(), 1);
}

#[test]
fn malformed_inventory_is_rejected() {
    let checks = inventory::checks();
    let mut duplicate = checks.clone();
    duplicate.push(checks[0].clone());
    assert!(inventory::validate(&duplicate).is_err());
    let mut empty_discovery = checks.clone();
    empty_discovery[0].discovery = Some(Discovery {
        args: vec!["test", "--list"],
        expected: Vec::new(),
        ignored: false,
    });
    assert!(inventory::validate(&empty_discovery).is_err());
}

#[test]
fn selector_target_and_list_only_mutations_cannot_borrow_a_valid_census() -> Result<(), String> {
    let checks = inventory::checks();
    let index = checks
        .iter()
        .position(|check| check.id == "nats-publish")
        .ok_or("proof missing")?;
    let mut typo = checks.clone();
    let argument = typo[index]
        .command
        .args
        .last_mut()
        .ok_or("filter missing")?;
    *argument = "renamed-test-selects-zero";
    assert!(inventory::validate(&typo).is_err());
    let mut target = checks.clone();
    let target_index = target[index]
        .command
        .args
        .iter()
        .position(|arg| *arg == "jetstream_publish")
        .ok_or("target missing")?;
    target[index].command.args[target_index] = "different_target";
    assert!(inventory::validate(&target).is_err());
    let mut list_only = checks.clone();
    list_only[index].command.args = list_only[index]
        .discovery
        .as_ref()
        .ok_or("discovery missing")?
        .args
        .clone();
    assert!(inventory::validate(&list_only).is_err());
    let mut duplicate_list = checks.clone();
    duplicate_list[index]
        .discovery
        .as_mut()
        .ok_or("discovery missing")?
        .args
        .push("--list");
    assert!(inventory::validate(&duplicate_list).is_err());
    Ok(())
}

#[test]
fn default_service_dependency_and_census_command_drift_fail_closed() -> Result<(), String> {
    let checks = inventory::checks();
    let mut service_default = checks.clone();
    service_default[0].tier = "T3";
    assert!(inventory::validate(&service_default).is_err());
    service_default[0].tier = "T2";
    service_default[0].prerequisites = &[Requirement::Environment("PRIVATE_DATABASE_URL")];
    assert!(inventory::validate(&service_default).is_err());
    let mut census = checks;
    let index = census
        .iter()
        .position(|c| c.id == "ignored-inventory")
        .ok_or("census missing")?;
    census[index].command.program = Program::Python;
    assert!(inventory::validate(&census).is_err());
    Ok(())
}

#[test]
fn overrides_are_single_argv_values_and_ordinary_children_force_offline_sqlx()
-> Result<(), Box<dyn std::error::Error>> {
    let options = Options::parse(&[
        "--python".to_owned(),
        "C:/Python Tools/python.exe".to_owned(),
        "--sbom-output".to_owned(),
        "artifacts/custom path".to_owned(),
    ])?;
    let command = CommandSpec {
        program: Program::Python,
        args: vec![
            "scripts/generate_sboms.py",
            "rust",
            "--output",
            "artifacts/sbom",
        ],
    };
    let child = runner::subprocess(&command, &options)?;
    assert_eq!(child.get_program(), "C:/Python Tools/python.exe");
    assert_eq!(
        child.get_args().collect::<Vec<_>>(),
        [
            "scripts/generate_sboms.py",
            "rust",
            "--output",
            "artifacts/custom path"
        ]
        .map(std::ffi::OsStr::new)
    );
    assert!(
        child.get_envs().any(
            |(key, value)| key == "SQLX_OFFLINE" && value == Some(std::ffi::OsStr::new("true"))
        )
    );
    let metadata = CommandSpec {
        program: Program::Xtask,
        args: vec!["sqlx-metadata"],
    };
    let child = runner::subprocess(&metadata, &options)?;
    assert!(!child.get_envs().any(|(key, _)| key == "SQLX_OFFLINE"));
    Ok(())
}
