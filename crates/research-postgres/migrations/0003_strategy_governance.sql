-- Exact M04 profile v1; mutable authority is append-only, outside profile identity.
CREATE TABLE edgeagent_research_profiles (
    caller_scope VARCHAR(64) NOT NULL,
    implementation VARCHAR(64) NOT NULL,
    configuration VARCHAR(64) NOT NULL,
    profile BYTEA NOT NULL CHECK (octet_length(profile) BETWEEN 1 AND 4096),
    PRIMARY KEY (caller_scope,implementation,configuration)
);
CREATE TABLE edgeagent_research_approvals (
    caller_scope VARCHAR(64) NOT NULL,
    implementation VARCHAR(64) NOT NULL,
    configuration VARCHAR(64) NOT NULL,
    revision SMALLINT NOT NULL CHECK (revision BETWEEN 1 AND 3),
    action VARCHAR(16) NOT NULL CHECK (
        (revision=1 AND action='installed') OR
        (revision=2 AND action='approved') OR
        (revision=3 AND action='retired')),
    actor TEXT NOT NULL,
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (caller_scope,implementation,configuration,revision),
    FOREIGN KEY (caller_scope,implementation,configuration)
        REFERENCES edgeagent_research_profiles(caller_scope,implementation,configuration)
);
CREATE TRIGGER edgeagent_research_profiles_immutable BEFORE UPDATE OR DELETE ON edgeagent_research_profiles
    FOR EACH ROW EXECUTE FUNCTION edgeagent_research_immutable();
CREATE TRIGGER edgeagent_research_approvals_immutable BEFORE UPDATE OR DELETE ON edgeagent_research_approvals
    FOR EACH ROW EXECUTE FUNCTION edgeagent_research_immutable();
