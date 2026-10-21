-- Expand only. Historical P08 receipts remain immutable; execution requires an
-- actual retained admission journal. Never synthesize one for a legacy order.
ALTER TABLE simulation_order ADD COLUMN admission_journal BYTEA
    CHECK (octet_length(admission_journal) BETWEEN 77 AND 1048576);
CREATE TABLE simulation_execution_config (
    account TEXT PRIMARY KEY REFERENCES simulation_account(account),
    binding BYTEA NOT NULL CHECK (octet_length(binding)=32),
    canonical BYTEA NOT NULL CHECK (octet_length(canonical) BETWEEN 1 AND 1048576),
    profile BYTEA NOT NULL CHECK (octet_length(profile) BETWEEN 1 AND 4096),
    slots BIGINT NOT NULL CHECK (slots BETWEEN 1 AND 4096)
);
CREATE TABLE simulation_execution_order (
    account TEXT NOT NULL, order_id TEXT NOT NULL,
    journal BYTEA NOT NULL CHECK (octet_length(journal) BETWEEN 77 AND 1048576),
    exact_cents TEXT NOT NULL CHECK (exact_cents ~ '^[0-9]{1,39}$'),
    filled_units BIGINT NOT NULL CHECK (filled_units>=0),
    gross_cents BIGINT NOT NULL CHECK (gross_cents>=0),
    fees_cents BIGINT NOT NULL CHECK (fees_cents>=0),
    PRIMARY KEY(account,order_id),
    FOREIGN KEY(account,order_id) REFERENCES simulation_order(account,order_id)
);
CREATE TABLE simulation_position_cost (
    account TEXT NOT NULL, strategy TEXT NOT NULL, instrument TEXT NOT NULL, unit TEXT NOT NULL,
    cost_cents BIGINT NOT NULL CHECK (cost_cents>=0),
    PRIMARY KEY(account,strategy,instrument,unit),
    FOREIGN KEY(account,strategy,instrument,unit) REFERENCES simulation_inventory(account,strategy,instrument,unit)
);
-- Run-wide, not account-wide. Shared displayed volume cannot be spent by a
-- second account/strategy/replica. Account lock THEN one liquidity row lock.
CREATE TABLE simulation_liquidity (
    run BYTEA NOT NULL CHECK (octet_length(run)=32),
    budget BYTEA NOT NULL CHECK (octet_length(budget)=32),
    content BYTEA NOT NULL CHECK (octet_length(content)=32),
    capacity BIGINT NOT NULL CHECK (capacity>=0),
    consumed BIGINT NOT NULL CHECK (consumed BETWEEN 0 AND capacity),
    PRIMARY KEY(run,budget)
);
CREATE TABLE simulation_execution_receipt (
    account TEXT NOT NULL REFERENCES simulation_account(account),
    sequence BIGINT NOT NULL CHECK (sequence>0),
    request BYTEA NOT NULL CHECK (octet_length(request) BETWEEN 1 AND 4096),
    outcome BYTEA NOT NULL CHECK (octet_length(outcome) BETWEEN 1 AND 16384),
    cash_delta BIGINT NOT NULL, position_delta BIGINT NOT NULL,
    realized_delta BIGINT NOT NULL, fee_delta BIGINT NOT NULL CHECK (fee_delta>=0),
    CHECK (cash_delta::NUMERIC + position_delta + realized_delta + fee_delta=0),
    PRIMARY KEY(account,sequence)
);
