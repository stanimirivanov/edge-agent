-- Owner-retained export log. Runtime receives INSERT/SELECT, never UPDATE/DELETE.
-- No backfill: a current balance cannot reconstruct historical funding/risk.
CREATE TABLE simulation_account_audit (
    account TEXT NOT NULL REFERENCES simulation_account(account),
    revision BIGINT NOT NULL CHECK(revision BETWEEN 0 AND 8192),
    payload BYTEA NOT NULL CHECK(octet_length(payload) BETWEEN 1 AND 1048576),
    PRIMARY KEY(account,revision)
);
