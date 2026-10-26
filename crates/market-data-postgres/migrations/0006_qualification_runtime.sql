-- Owner-reviewed complete input; never a worker-supplied approval document.
CREATE TABLE edgeagent_qualification_inputs (
    request_id TEXT PRIMARY KEY REFERENCES edgeagent_acquisitions(request_id) ON DELETE CASCADE,
    document BYTEA NOT NULL CHECK (octet_length(document) BETWEEN 1 AND 262144)
);
REVOKE ALL ON edgeagent_qualification_inputs FROM PUBLIC;

-- Early-bound relation OID prevents a worker TEMP relation from substituting input.
CREATE FUNCTION edgeagent_qualification_input(target TEXT)
RETURNS BYTEA LANGUAGE SQL SECURITY DEFINER SET search_path=pg_catalog
BEGIN ATOMIC
    SELECT document FROM edgeagent_qualification_inputs WHERE request_id=target;
END;
REVOKE ALL ON FUNCTION edgeagent_qualification_input(TEXT) FROM PUBLIC;

CREATE TABLE edgeagent_precise_preparations (
    request_id TEXT PRIMARY KEY REFERENCES edgeagent_acquisitions(request_id) ON DELETE CASCADE,
    digest TEXT NOT NULL,
    artifact BYTEA NOT NULL CHECK (octet_length(artifact) BETWEEN 1 AND 2097152)
);
REVOKE ALL ON edgeagent_precise_preparations FROM PUBLIC;

-- No timeout or automatic takeover. A dead process leaves an unresolved fence.
CREATE TABLE edgeagent_qualification_workers (
    account_id TEXT PRIMARY KEY REFERENCES edgeagent_acquisition_accounts(account_id),
    generation BIGINT NOT NULL CHECK (generation>0),
    request_id TEXT NOT NULL,
    active BOOLEAN NOT NULL
);
REVOKE ALL ON edgeagent_qualification_workers FROM PUBLIC;

-- Survives admission retirement; exact inventory and worker generation are reviewed.
CREATE TABLE edgeagent_qualification_cleanup (
    request_id TEXT PRIMARY KEY REFERENCES edgeagent_acquisition_denials(request_id),
    inventory BYTEA NOT NULL CHECK (octet_length(inventory) BETWEEN 1 AND 32768),
    evidence TEXT NOT NULL,
    approved_by NAME NOT NULL DEFAULT current_user,
    approved_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp()
);
REVOKE ALL ON edgeagent_qualification_cleanup FROM PUBLIC;

CREATE FUNCTION edgeagent_qualification_input_role()
RETURNS BOOLEAN LANGUAGE SQL SECURITY INVOKER SET search_path=pg_catalog
BEGIN ATOMIC
    SELECT NOT EXISTS (
        SELECT 1 FROM pg_roles r WHERE
        (pg_has_role(r.oid,'MEMBER') OR pg_has_role(session_user,r.oid,'MEMBER'))
        AND (has_table_privilege(r.oid,'edgeagent_qualification_inputs'::regclass,'INSERT,UPDATE,DELETE,TRUNCATE,TRIGGER,REFERENCES')
          OR has_table_privilege(r.oid,'edgeagent_qualification_cleanup'::regclass,'INSERT,UPDATE,DELETE,TRUNCATE,TRIGGER,REFERENCES'))
    );
END;
REVOKE ALL ON FUNCTION edgeagent_qualification_input_role() FROM PUBLIC;
