-- NULL deliberately fences unresolved pre-migration generations. No capacity refund.
ALTER TABLE edgeagent_stream_sessions ADD COLUMN generation_start BIGINT CHECK(generation_start>0);
CREATE TABLE edgeagent_stream_retention (
    session_id TEXT PRIMARY KEY REFERENCES edgeagent_stream_admissions(session_id),
    denied BOOLEAN NOT NULL DEFAULT FALSE,
    writers_quiet BOOLEAN NOT NULL DEFAULT FALSE,
    approved_session NAME NOT NULL DEFAULT session_user
);
INSERT INTO edgeagent_stream_retention(session_id) SELECT session_id FROM edgeagent_stream_admissions;
CREATE TABLE edgeagent_stream_publications (
    publication_id TEXT PRIMARY KEY CHECK(publication_id ~ '^[0-9a-f]{32}$'),
    session_id TEXT NOT NULL REFERENCES edgeagent_stream_retention(session_id),
    instrument TEXT CHECK(instrument ~ '^[0-9a-f]{32}$'),
    preparation BYTEA CHECK(octet_length(preparation) BETWEEN 1 AND 20975616),
    descriptor TEXT NOT NULL CHECK(octet_length(descriptor)<=4096),
    delete_ms BIGINT NOT NULL,
    published BOOLEAN NOT NULL DEFAULT FALSE,
    retired BOOLEAN NOT NULL DEFAULT FALSE,
    event BYTEA CHECK(octet_length(event)<=262144),
    history_start BOOLEAN,
    history_retired BOOLEAN,
    reconnect BOOLEAN,
    skipped BOOLEAN,
    CHECK(NOT retired OR (preparation IS NULL AND event IS NULL AND instrument IS NULL)),
    CHECK(descriptor<>'' OR retired),
    CHECK(retired OR (published=(event IS NOT NULL)))
);
CREATE UNIQUE INDEX edgeagent_stream_one_preparation ON edgeagent_stream_publications(session_id,instrument)
    WHERE NOT published AND NOT retired;
CREATE INDEX edgeagent_stream_pending_cleanup ON edgeagent_stream_publications(session_id,publication_id)
    WHERE retired AND descriptor<>'';
CREATE TABLE edgeagent_stream_heads (
    session_id TEXT NOT NULL REFERENCES edgeagent_stream_retention(session_id),
    instrument TEXT NOT NULL CHECK(instrument ~ '^[0-9a-f]{32}$'),
    publication_id TEXT NOT NULL REFERENCES edgeagent_stream_publications(publication_id),
    -- Retired history keeps its capture position, never its licensed fingerprint.
    digest TEXT CHECK(digest ~ '^[0-9a-f]{64}$'),
    frame BIGINT NOT NULL CHECK(frame>0),
    generation BIGINT NOT NULL CHECK(generation>0),
    epoch BIGINT NOT NULL CHECK(epoch>0),
    captured_ms BIGINT NOT NULL,
    PRIMARY KEY(session_id,instrument)
);
REVOKE ALL ON edgeagent_stream_retention,edgeagent_stream_publications,edgeagent_stream_heads FROM PUBLIC;

-- Owner controls are early-bound and cannot be shadowed by worker temp tables.
CREATE FUNCTION edgeagent_lock_stream_retention(target TEXT)
RETURNS TABLE(denied BOOLEAN,writers_quiet BOOLEAN,independent BOOLEAN)
LANGUAGE SQL SECURITY DEFINER SET search_path=pg_catalog
BEGIN ATOMIC
    SELECT r.denied,r.writers_quiet,r.approved_session<>session_user
        AND NOT pg_has_role(session_user,r.approved_session,'MEMBER')
        AND NOT EXISTS (
            SELECT 1 FROM pg_roles p WHERE
                -- SECURITY DEFINER changes current_user to the owner. Inspect the
                -- authenticated LOGIN's memberships, not the definer's privileges.
                pg_has_role(session_user,p.oid,'MEMBER')
                AND (has_table_privilege(p.oid,r.tableoid,'INSERT,UPDATE,DELETE,TRUNCATE,TRIGGER,REFERENCES')
                    OR EXISTS(SELECT 1 FROM pg_attribute a WHERE a.attrelid=r.tableoid AND a.attnum>0
                        AND NOT a.attisdropped AND has_column_privilege(p.oid,a.attrelid,a.attnum,'INSERT,UPDATE,REFERENCES')))
        )
    FROM edgeagent_stream_retention r WHERE session_id=target FOR SHARE;
END;
REVOKE ALL ON FUNCTION edgeagent_lock_stream_retention(TEXT) FROM PUBLIC;
