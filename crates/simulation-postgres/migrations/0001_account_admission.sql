-- Apply only in ExecutionSimulator's private schema, alongside inbox/outbox.
CREATE TABLE IF NOT EXISTS simulation_account (
    account TEXT PRIMARY KEY CHECK (account ~ '^[0-9a-f]{32}$'),
    configuration BYTEA NOT NULL CHECK (octet_length(configuration) = 32),
    revision BIGINT NOT NULL DEFAULT 0 CHECK (revision >= 0),
    input_revision BIGINT NOT NULL DEFAULT 0 CHECK (input_revision >= 0),
    cash_cents BIGINT NOT NULL CHECK (cash_cents >= 0),
    risk_checkpoint BYTEA NOT NULL CHECK (octet_length(risk_checkpoint) = 158),
    run_checkpoint BYTEA NOT NULL CHECK (octet_length(run_checkpoint) = 108)
);
CREATE TABLE IF NOT EXISTS simulation_account_grant (
    account TEXT NOT NULL REFERENCES simulation_account(account),
    principal TEXT NOT NULL CHECK (principal ~ '^[0-9a-f]{32}$'),
    PRIMARY KEY (account, principal)
);
CREATE TABLE IF NOT EXISTS simulation_valuation (
    account TEXT NOT NULL REFERENCES simulation_account(account),
    strategy TEXT NOT NULL CHECK (strategy ~ '^[0-9a-f]{32}$'),
    gross_cents BIGINT NOT NULL CHECK (gross_cents >= 0),
    loss_cents BIGINT NOT NULL CHECK (loss_cents >= 0),
    valued_seconds BIGINT NOT NULL,
    valued_nanos BIGINT NOT NULL CHECK (valued_nanos BETWEEN 0 AND 999999999),
    artifact BYTEA NOT NULL CHECK (octet_length(artifact) = 32),
    PRIMARY KEY (account, strategy)
);
CREATE TABLE IF NOT EXISTS simulation_inventory (
    account TEXT NOT NULL REFERENCES simulation_account(account),
    strategy TEXT NOT NULL,
    instrument TEXT NOT NULL CHECK (instrument ~ '^[0-9a-f]{32}$'),
    unit TEXT NOT NULL CHECK (unit IN ('shares', 'contracts')),
    quantity BIGINT NOT NULL CHECK (quantity >= 0),
    PRIMARY KEY (account, strategy, instrument, unit),
    FOREIGN KEY (account, strategy) REFERENCES simulation_valuation(account, strategy)
);
CREATE TABLE IF NOT EXISTS simulation_quote (
    account TEXT NOT NULL REFERENCES simulation_account(account),
    fingerprint TEXT NOT NULL CHECK (fingerprint ~ '^[0-9a-f]{64}$'),
    input_revision BIGINT NOT NULL CHECK (input_revision > 0),
    canonical BYTEA NOT NULL CHECK (octet_length(canonical) BETWEEN 1 AND 4096),
    profile TEXT NOT NULL CHECK (profile = 'synthetic-reservation-v1'),
    PRIMARY KEY (account, fingerprint),
    UNIQUE (account, input_revision)
);
CREATE TABLE IF NOT EXISTS simulation_order (
    account TEXT NOT NULL REFERENCES simulation_account(account),
    order_id TEXT NOT NULL CHECK (order_id ~ '^[0-9a-f]{64}$'),
    quote TEXT NOT NULL,
    request BYTEA NOT NULL CHECK (octet_length(request) BETWEEN 1 AND 4096),
    outcome BYTEA NOT NULL CHECK (octet_length(outcome) BETWEEN 1 AND 16384),
    account_revision BIGINT NOT NULL CHECK (account_revision > 0),
    order_revision BIGINT NOT NULL CHECK (order_revision = 1),
    PRIMARY KEY (account, order_id),
    UNIQUE (account, account_revision),
    FOREIGN KEY (account, quote) REFERENCES simulation_quote(account, fingerprint)
);
CREATE TABLE IF NOT EXISTS simulation_reservation (
    account TEXT NOT NULL,
    order_id TEXT NOT NULL,
    strategy TEXT NOT NULL,
    instrument TEXT NOT NULL CHECK (instrument ~ '^[0-9a-f]{32}$'),
    unit TEXT NOT NULL CHECK (unit IN ('shares', 'contracts')),
    cash_cents BIGINT NOT NULL CHECK (cash_cents >= 0),
    quantity BIGINT NOT NULL CHECK (quantity >= 0),
    state TEXT NOT NULL CHECK (state = 'held'),
    PRIMARY KEY (account, order_id),
    FOREIGN KEY (account, order_id) REFERENCES simulation_order(account, order_id),
    FOREIGN KEY (account, strategy) REFERENCES simulation_valuation(account, strategy)
);
CREATE INDEX IF NOT EXISTS simulation_reservation_strategy ON simulation_reservation(account, strategy);
