-- MarketData-private ownership. No provider I/O occurs inside these transactions.
CREATE TABLE edgeagent_acquisition_accounts (
    account_id TEXT PRIMARY KEY CHECK (account_id ~ '^[0-9a-f]{32}$'),
    budget BYTEA NOT NULL CHECK (octet_length(budget) BETWEEN 1 AND 8192)
);
CREATE TABLE edgeagent_acquisitions (
    request_id TEXT PRIMARY KEY CHECK (request_id ~ '^[0-9a-f]{32}$'),
    account_id TEXT NOT NULL REFERENCES edgeagent_acquisition_accounts(account_id),
    digest TEXT NOT NULL CHECK (digest ~ '^[0-9a-f]{64}$'),
    admission BYTEA NOT NULL CHECK (octet_length(admission) BETWEEN 1 AND 8192),
    budget BYTEA NOT NULL CHECK (octet_length(budget) BETWEEN 1 AND 8192)
);
