use super::model::{CrateRole, PackagePolicy, TemporaryException};

const NONE: &[&str] = &[];

pub(super) const PACKAGE_POLICIES: &[PackagePolicy] = &[
    PackagePolicy {
        name: "edgeagent-audit-projector",
        role: CrateRole::CompositionRoot,
        normal: &["edgeagent-contracts", "edgeagent-service-runtime"],
        development: NONE,
        build: NONE,
    },
    PackagePolicy {
        name: "edgeagent-contracts",
        role: CrateRole::Contract,
        normal: NONE,
        development: NONE,
        build: NONE,
    },
    PackagePolicy {
        name: "edgeagent-execution-simulator",
        role: CrateRole::CompositionRoot,
        normal: &["edgeagent-contracts", "edgeagent-service-runtime"],
        development: NONE,
        build: NONE,
    },
    PackagePolicy {
        name: "edgeagent-gateway",
        role: CrateRole::CompositionRoot,
        normal: &["edgeagent-contracts", "edgeagent-service-runtime"],
        development: NONE,
        build: NONE,
    },
    PackagePolicy {
        name: "edgeagent-inbox-handler",
        role: CrateRole::Application,
        normal: &[
            "edgeagent-contracts",
            "edgeagent-messaging",
            "edgeagent-telemetry",
        ],
        development: &[
            "edgeagent-inbox-postgres",
            "edgeagent-messaging-nats",
            "edgeagent-outbox-postgres",
            "edgeagent-outbox-relay",
        ],
        build: NONE,
    },
    PackagePolicy {
        name: "edgeagent-inbox-postgres",
        role: CrateRole::Adapter,
        normal: &["edgeagent-contracts", "edgeagent-messaging"],
        development: NONE,
        build: NONE,
    },
    PackagePolicy {
        name: "edgeagent-market-data",
        role: CrateRole::CompositionRoot,
        normal: &["edgeagent-contracts", "edgeagent-service-runtime"],
        development: NONE,
        build: NONE,
    },
    PackagePolicy {
        name: "edgeagent-messaging",
        role: CrateRole::Port,
        normal: &["edgeagent-contracts"],
        development: NONE,
        build: NONE,
    },
    PackagePolicy {
        name: "edgeagent-messaging-nats",
        role: CrateRole::Adapter,
        normal: &["edgeagent-contracts", "edgeagent-messaging"],
        development: NONE,
        build: NONE,
    },
    PackagePolicy {
        name: "edgeagent-outbox-postgres",
        role: CrateRole::Adapter,
        normal: &["edgeagent-contracts", "edgeagent-messaging"],
        development: NONE,
        build: NONE,
    },
    PackagePolicy {
        name: "edgeagent-outbox-relay",
        role: CrateRole::Application,
        normal: &[
            "edgeagent-contracts",
            "edgeagent-messaging",
            "edgeagent-telemetry",
        ],
        development: &["edgeagent-outbox-postgres"],
        build: NONE,
    },
    PackagePolicy {
        name: "edgeagent-research",
        role: CrateRole::CompositionRoot,
        normal: &["edgeagent-contracts", "edgeagent-service-runtime"],
        development: NONE,
        build: NONE,
    },
    PackagePolicy {
        name: "edgeagent-service-runtime",
        role: CrateRole::RuntimeSupport,
        normal: &["edgeagent-contracts"],
        development: NONE,
        build: NONE,
    },
    PackagePolicy {
        name: "edgeagent-telemetry",
        role: CrateRole::Observability,
        normal: &["edgeagent-contracts", "edgeagent-messaging"],
        development: NONE,
        build: NONE,
    },
    PackagePolicy {
        name: "edgeagent-xtask",
        role: CrateRole::Tooling,
        normal: NONE,
        development: NONE,
        build: NONE,
    },
];

pub(super) const TEMPORARY_EXCEPTIONS: &[TemporaryException] = &[];
