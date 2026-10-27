-- Transient scheduler health cannot reopen a restored session.
CREATE TABLE edgeagent_stream_recovery (
    session_id TEXT PRIMARY KEY REFERENCES edgeagent_stream_admissions(session_id),
    revision BIGINT NOT NULL DEFAULT 0 CHECK(revision>=0),
    blocked BOOLEAN NOT NULL DEFAULT TRUE,
    incident SMALLINT CHECK(incident BETWEEN 1 AND 4),
    CHECK(incident IS NULL OR blocked)
);
INSERT INTO edgeagent_stream_recovery(session_id) SELECT session_id FROM edgeagent_stream_admissions;
REVOKE ALL ON edgeagent_stream_recovery FROM PUBLIC;

-- Early-bound SQL body; account/admission/session locks share publication order.
-- No worker receives control DML. No operation clears an incident or account slot.
CREATE FUNCTION edgeagent_stream_recovery_control(target TEXT, expected BIGINT, action TEXT, reason SMALLINT)
RETURNS TEXT LANGUAGE SQL SECURITY DEFINER SET search_path=pg_catalog
BEGIN ATOMIC
    SELECT 1 FROM edgeagent_stream_accounts WHERE account_id=(
        SELECT account_id FROM edgeagent_stream_admissions WHERE session_id=target) FOR UPDATE;
    SELECT 1 FROM edgeagent_stream_admissions WHERE session_id=target FOR UPDATE;
    SELECT 1 FROM edgeagent_stream_sessions WHERE session_id=target FOR UPDATE;
    INSERT INTO edgeagent_stream_recovery(session_id)
        SELECT session_id FROM edgeagent_stream_admissions WHERE session_id=target
        ON CONFLICT DO NOTHING;
    WITH authorized AS MATERIALIZED (
        SELECT r.session_id FROM edgeagent_stream_recovery r
        JOIN edgeagent_stream_admissions a USING(session_id)
        WHERE r.session_id=target AND (
            (action='approve' AND session_user=a.approved_session)
            OR (action<>'approve' AND session_user<>a.approved_session
                AND NOT pg_has_role(session_user,a.approved_session,'MEMBER')
                AND NOT EXISTS (
                    SELECT 1 FROM pg_roles p WHERE pg_has_role(session_user,p.oid,'MEMBER')
                    AND (has_table_privilege(p.oid,r.tableoid,'INSERT,UPDATE,DELETE,TRUNCATE,TRIGGER,REFERENCES')
                        OR EXISTS (SELECT 1 FROM pg_attribute c WHERE c.attrelid=r.tableoid
                            AND c.attnum>0 AND NOT c.attisdropped
                            AND has_column_privilege(p.oid,c.attrelid,c.attnum,'INSERT,UPDATE,REFERENCES')))
                ))
        ) FOR UPDATE OF r
    ), changed AS (
        UPDATE edgeagent_stream_recovery r SET
            revision=CASE WHEN action IN ('begin','incident') THEN r.revision+1 ELSE r.revision END,
            blocked=CASE WHEN action='approve' THEN FALSE WHEN action IN ('begin','incident') THEN TRUE ELSE r.blocked END,
            incident=CASE WHEN action='incident' THEN reason ELSE r.incident END
        FROM authorized z WHERE r.session_id=z.session_id
        AND r.revision=expected AND r.incident IS NULL
        AND (action='approve' OR r.revision<9223372036854775807)
        AND (action<>'incident' OR reason BETWEEN 1 AND 4)
        AND action IN ('begin','incident','approve')
        AND (action<>'approve' OR (
            r.revision>0 AND r.incident IS NULL
            AND NOT EXISTS (SELECT 1 FROM edgeagent_stream_publications p
                WHERE p.session_id=target AND NOT p.published AND NOT p.retired)
            AND EXISTS (SELECT 1 FROM edgeagent_stream_admissions a JOIN edgeagent_stream_sessions s USING(session_id)
                WHERE a.session_id=target AND NOT a.withdrawn AND NOT s.stopped)))
        RETURNING r.revision,r.blocked,r.incident
    ), observed AS (
        -- Checks lock the barrier but do not generate a no-op UPDATE/WAL record.
        SELECT r.revision,r.blocked,r.incident FROM edgeagent_stream_recovery r
        JOIN authorized z USING(session_id)
        WHERE action='inspect' OR (r.revision=expected AND r.revision>0 AND r.incident IS NULL
            AND ((action='recover' AND r.blocked) OR (action='check' AND NOT r.blocked
                AND NOT EXISTS (SELECT 1 FROM edgeagent_stream_publications p
                    WHERE p.session_id=target AND NOT p.published AND NOT p.retired)
                AND EXISTS (SELECT 1 FROM edgeagent_stream_admissions a JOIN edgeagent_stream_sessions s USING(session_id)
                    WHERE a.session_id=target AND NOT a.withdrawn AND NOT s.stopped))))
    ) SELECT revision::text||':'||blocked::text||':'||COALESCE(incident::text,'0') FROM changed
      UNION ALL SELECT revision::text||':'||blocked::text||':'||COALESCE(incident::text,'0') FROM observed;
END;
REVOKE ALL ON FUNCTION edgeagent_stream_recovery_control(TEXT,BIGINT,TEXT,SMALLINT) FROM PUBLIC;
