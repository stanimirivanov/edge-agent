-- Offline-first stream resources; no vendor activation or changes to daily call credits.
CREATE TABLE edgeagent_stream_accounts (
    account_id TEXT PRIMARY KEY CHECK(account_id ~ '^[0-9a-f]{32}$'),
    revision BIGINT NOT NULL CHECK(revision BETWEEN 1 AND 4294967295),
    connections INTEGER NOT NULL CHECK(connections BETWEEN 1 AND 65535),
    start_limit BIGINT NOT NULL CHECK(start_limit BETWEEN 1 AND 4294967295),
    session_ms BIGINT NOT NULL CHECK(session_ms BETWEEN 1 AND 4294967295),
    spacing_ms BIGINT NOT NULL CHECK(spacing_ms BETWEEN 1 AND 4294967295),
    expires_ms BIGINT NOT NULL,
    opening_starts BIGINT NOT NULL CHECK(opening_starts>=0 AND opening_starts<=start_limit),
    charged_starts BIGINT NOT NULL CHECK(charged_starts>=opening_starts AND charged_starts<=start_limit),
    generation BIGINT NOT NULL DEFAULT 0 CHECK(generation>=0),
    last_start_ms BIGINT,
    withdrawn BOOLEAN NOT NULL DEFAULT FALSE,
    approved_session NAME NOT NULL DEFAULT session_user
);
CREATE TABLE edgeagent_stream_admissions (
    session_id TEXT PRIMARY KEY CHECK(session_id ~ '^[0-9a-f]{32}$'),
    account_id TEXT NOT NULL REFERENCES edgeagent_stream_accounts(account_id),
    admission BYTEA NOT NULL CHECK(octet_length(admission) BETWEEN 1 AND 65536),
    withdrawn BOOLEAN NOT NULL DEFAULT FALSE,
    approved_session NAME NOT NULL DEFAULT session_user
);
CREATE TABLE edgeagent_stream_sessions (
    session_id TEXT PRIMARY KEY REFERENCES edgeagent_stream_admissions(session_id),
    starts BIGINT NOT NULL DEFAULT 0 CHECK(starts BETWEEN 0 AND 4294967295),
    frames BIGINT NOT NULL DEFAULT 0 CHECK(frames BETWEEN 0 AND 4294967295),
    bytes BIGINT NOT NULL DEFAULT 0 CHECK(bytes>=0),
    last_frame_bytes BIGINT CHECK(last_frame_bytes BETWEEN 0 AND 4294967295),
    stopped BOOLEAN NOT NULL DEFAULT FALSE,
    generation BIGINT NOT NULL DEFAULT 0 CHECK(generation>=0),
    active BOOLEAN NOT NULL DEFAULT FALSE,
    CHECK((frames=0)=(last_frame_bytes IS NULL)),
    CHECK(NOT active OR generation>0)
);
CREATE INDEX edgeagent_stream_account_sessions ON edgeagent_stream_admissions(account_id);
REVOKE ALL ON edgeagent_stream_accounts,edgeagent_stream_admissions,edgeagent_stream_sessions FROM PUBLIC;

-- Narrow early-bound read/lock: a temporary table cannot manufacture approval.
CREATE FUNCTION edgeagent_lock_stream_admission(target TEXT)
RETURNS TABLE(account_id TEXT,admission BYTEA,withdrawn BOOLEAN,independent BOOLEAN)
LANGUAGE SQL SECURITY DEFINER SET search_path=pg_catalog
BEGIN ATOMIC
    SELECT s.account_id,s.admission,s.withdrawn,
        s.approved_session<>session_user AND a.approved_session<>session_user
        AND NOT pg_has_role(session_user,s.approved_session,'MEMBER')
        AND NOT pg_has_role(session_user,a.approved_session,'MEMBER')
    FROM edgeagent_stream_admissions s JOIN edgeagent_stream_accounts a USING(account_id)
    WHERE s.session_id=target FOR SHARE OF s;
END;
REVOKE ALL ON FUNCTION edgeagent_lock_stream_admission(TEXT) FROM PUBLIC;

-- Current and session LOGIN must lack every owner-control write path, including
-- NOINHERIT membership. Ordinary counter DML remains trusted application access.
CREATE FUNCTION edgeagent_stream_worker_allowed() RETURNS BOOLEAN
LANGUAGE SQL SECURITY INVOKER SET search_path=pg_catalog
BEGIN ATOMIC
    SELECT q.worker_allowed AND NOT EXISTS (
        SELECT 1 FROM pg_roles r
        WHERE (pg_has_role(r.oid,'MEMBER') OR pg_has_role(session_user,r.oid,'MEMBER'))
          AND (has_table_privilege(r.oid,s.oid,'INSERT,UPDATE,DELETE,TRUNCATE,TRIGGER,REFERENCES')
            OR has_table_privilege(r.oid,a.oid,'INSERT,DELETE,TRUNCATE,TRIGGER,REFERENCES')
            OR EXISTS (SELECT 1 FROM pg_attribute p WHERE p.attrelid IN (s.oid,a.oid)
                AND p.attnum>0 AND NOT p.attisdropped
                AND (has_column_privilege(r.oid,p.attrelid,p.attnum,'INSERT,REFERENCES')
                    OR ((p.attrelid=s.oid OR p.attname NOT IN ('charged_starts','generation','last_start_ms'))
                        AND has_column_privilege(r.oid,p.attrelid,p.attnum,'UPDATE')))))
    ) FROM edgeagent_qualification_role() q,pg_class s,pg_class a
    WHERE s.oid='edgeagent_stream_admissions'::regclass AND a.oid='edgeagent_stream_accounts'::regclass;
END;
REVOKE ALL ON FUNCTION edgeagent_stream_worker_allowed() FROM PUBLIC;
