-- Immutable synthetic postlude, appended to one frozen account/execution plan.
CREATE TABLE simulation_settlement_config (
    account TEXT PRIMARY KEY REFERENCES simulation_account(account),
    binding BYTEA NOT NULL CHECK(octet_length(binding)=32),
    canonical BYTEA NOT NULL CHECK(octet_length(canonical) BETWEEN 1 AND 1048576)
);
CREATE TABLE simulation_settlement_receipt (
    account TEXT NOT NULL REFERENCES simulation_account(account),
    sequence BIGINT NOT NULL CHECK(sequence>0),
    request BYTEA NOT NULL CHECK(octet_length(request) BETWEEN 1 AND 4096),
    outcome BYTEA NOT NULL CHECK(octet_length(outcome) BETWEEN 1 AND 16384),
    cash_delta BIGINT NOT NULL, position_delta BIGINT NOT NULL,
    realized_delta BIGINT NOT NULL, fee_delta BIGINT NOT NULL CHECK(fee_delta>=0),
    CHECK(cash_delta::NUMERIC+position_delta+realized_delta+fee_delta=0),
    PRIMARY KEY(account,sequence)
);
-- A seal survives later zero-effect commands; account/run ownership is immutable.
CREATE TABLE simulation_settled_instrument (
    account TEXT NOT NULL REFERENCES simulation_account(account),
    instrument TEXT NOT NULL CHECK(instrument ~ '^[0-9a-f]{32}$'),
    policy BYTEA NOT NULL CHECK(octet_length(policy)=32),
    evidence BYTEA NOT NULL CHECK(octet_length(evidence)=32),
    sequence BIGINT NOT NULL,
    PRIMARY KEY(account,instrument),
    FOREIGN KEY(account,sequence) REFERENCES simulation_settlement_receipt(account,sequence)
);
