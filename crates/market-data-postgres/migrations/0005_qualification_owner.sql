-- Created by the migration owner, never by a worker. Deployment separately grants
-- SELECT to workers and DML to independently authenticated approval operators.
-- FK retirement erases private profile/review bytes with the original admission.
CREATE TABLE edgeagent_qualification_grants (
    request_id TEXT PRIMARY KEY REFERENCES edgeagent_acquisitions(request_id) ON DELETE CASCADE,
    profile BYTEA NOT NULL CHECK (octet_length(profile) BETWEEN 1 AND 8192),
    reviews BYTEA NOT NULL CHECK (octet_length(reviews) BETWEEN 1 AND 4096),
    approved BOOLEAN NOT NULL DEFAULT FALSE,
    approved_by NAME NOT NULL DEFAULT current_user,
    approved_session NAME NOT NULL DEFAULT session_user,
    approved_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp()
);
REVOKE ALL ON edgeagent_qualification_grants FROM PUBLIC;

-- Row locks normally require UPDATE privileges. Expose only a bounded locking
-- read instead. SQL-standard body binds relation OIDs at definition time; a fixed
-- runtime search path prevents temp/public substitution under SECURITY DEFINER.
CREATE FUNCTION edgeagent_lock_qualification_grant(target TEXT)
RETURNS TABLE(profile BYTEA, reviews BYTEA, approved BOOLEAN, independent BOOLEAN)
LANGUAGE SQL SECURITY DEFINER SET search_path = pg_catalog
BEGIN ATOMIC
    SELECT g.profile,g.reviews,g.approved,
        g.approved_by<>session_user AND g.approved_session<>session_user
        AND NOT pg_has_role(session_user,g.approved_by,'MEMBER')
    FROM edgeagent_qualification_grants AS g
    WHERE g.request_id=target FOR SHARE;
END;
REVOKE ALL ON FUNCTION edgeagent_lock_qualification_grant(TEXT) FROM PUBLIC;

-- INVOKER rights: current_user/session_user remain the authenticated callers.
-- Early-bound relation identity also prevents an implicit temp table from
-- turning a harmless CREATE privilege into apparent approval authority.
CREATE FUNCTION edgeagent_qualification_role()
RETURNS TABLE(owner_allowed BOOLEAN, worker_allowed BOOLEAN)
LANGUAGE SQL SECURITY INVOKER SET search_path = pg_catalog
BEGIN ATOMIC
    SELECT has_table_privilege(current_user,c.oid,'INSERT')
               AND has_table_privilege(current_user,c.oid,'UPDATE'),
           NOT (has_schema_privilege(c.relnamespace,'CREATE') OR EXISTS (
               SELECT 1 FROM pg_roles r
               WHERE (pg_has_role(r.oid,'MEMBER') OR pg_has_role(session_user,r.oid,'MEMBER'))
               AND (r.rolsuper OR r.rolcreaterole OR r.rolcreatedb OR r.rolbypassrls
                   OR has_schema_privilege(r.oid,c.relnamespace,'CREATE')
                   OR has_table_privilege(r.oid,c.oid,'INSERT,UPDATE,DELETE,TRUNCATE,TRIGGER,REFERENCES'))
           ))
    FROM pg_class c WHERE c.oid='edgeagent_qualification_grants'::regclass;
END;
REVOKE ALL ON FUNCTION edgeagent_qualification_role() FROM PUBLIC;
