CREATE TABLE edgeagent_research_experiment_ledger (
    caller_scope VARCHAR(64) NOT NULL,
    family VARCHAR(64) NOT NULL,
    ordinal INTEGER NOT NULL CHECK (ordinal BETWEEN 0 AND 65),
    record BYTEA NOT NULL CHECK (octet_length(record) BETWEEN 1 AND 262144),
    actor TEXT NOT NULL,
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (caller_scope, family, ordinal)
);
CREATE TRIGGER edgeagent_research_experiment_ledger_immutable BEFORE UPDATE OR DELETE
    ON edgeagent_research_experiment_ledger FOR EACH ROW EXECUTE FUNCTION edgeagent_research_immutable();
