-- Full private owner inputs; workers get narrow EXECUTE, never control DML.
CREATE TABLE edgeagent_stream_owner_inputs (
    session_id TEXT PRIMARY KEY REFERENCES edgeagent_stream_admissions(session_id),
    inputs BYTEA CHECK(octet_length(inputs) BETWEEN 1 AND 1048576),
    expires_ms BIGINT NOT NULL,
    retired BOOLEAN NOT NULL DEFAULT FALSE,
    verified BOOLEAN NOT NULL CHECK(verified),
    CHECK(retired=(inputs IS NULL))
);
CREATE TABLE edgeagent_stream_namespaces (
    namespace TEXT PRIMARY KEY CHECK(namespace ~ '^[0-9a-f]{16}$' AND namespace<>'0000000000000000'),
    scope BYTEA UNIQUE CHECK(octet_length(scope) BETWEEN 1 AND 1024),
    retired BOOLEAN NOT NULL DEFAULT FALSE,
    CHECK(retired=(scope IS NULL))
);
CREATE TABLE edgeagent_stream_namespace_members (
    session_id TEXT NOT NULL REFERENCES edgeagent_stream_owner_inputs(session_id),
    namespace TEXT NOT NULL REFERENCES edgeagent_stream_namespaces(namespace),
    PRIMARY KEY(session_id,namespace)
);
REVOKE ALL ON edgeagent_stream_owner_inputs,edgeagent_stream_namespaces,edgeagent_stream_namespace_members FROM PUBLIC;

CREATE FUNCTION edgeagent_stream_owner_inputs_control(target TEXT, action TEXT, body BYTEA, expiry BIGINT)
RETURNS BOOLEAN LANGUAGE SQL SECURITY DEFINER SET search_path=pg_catalog
BEGIN ATOMIC
    SELECT 1 FROM edgeagent_stream_accounts WHERE account_id=(SELECT account_id FROM edgeagent_stream_admissions WHERE session_id=target) FOR UPDATE;
    SELECT 1 FROM edgeagent_stream_admissions WHERE session_id=target FOR UPDATE;
    SELECT 1 FROM edgeagent_stream_sessions WHERE session_id=target FOR UPDATE;
    SELECT 1 FROM edgeagent_stream_retention WHERE session_id=target FOR UPDATE;
    -- Serialize control-plane claims, including conflicts on different namespaces'
    -- UNIQUE scope keys. No external I/O or implicit retries hold this lock.
    SELECT pg_advisory_xact_lock('edgeagent_stream_namespaces'::regclass::oid::bigint);
    INSERT INTO edgeagent_stream_namespaces(namespace,scope)
        SELECT c->>0,decode(c->>1,'hex') FROM edgeagent_stream_admissions a
        JOIN edgeagent_stream_accounts account USING(account_id),
        LATERAL jsonb_array_elements(CASE WHEN action='install' THEN convert_from(body,'UTF8')::jsonb->4 ELSE NULL END) c
        WHERE a.session_id=target AND session_user=a.approved_session AND session_user=account.approved_session
        AND action='install' ORDER BY c->>0 ON CONFLICT(namespace) DO NOTHING;
    WITH context AS (
        SELECT a.*,account.withdrawn AS account_withdrawn,account.approved_session AS account_owner,
            s.stopped,r.denied,r.writers_quiet,
            CASE WHEN action='install' THEN convert_from(body,'UTF8')::jsonb ELSE NULL END AS doc
        FROM edgeagent_stream_admissions a JOIN edgeagent_stream_accounts account USING(account_id)
        JOIN edgeagent_stream_sessions s USING(session_id) JOIN edgeagent_stream_retention r USING(session_id)
        WHERE a.session_id=target
    ) INSERT INTO edgeagent_stream_owner_inputs(session_id,inputs,expires_ms,verified)
        SELECT target,body,expiry,
            session_user=approved_session AND session_user=account_owner
            AND NOT withdrawn AND NOT account_withdrawn AND NOT stopped AND NOT denied AND NOT writers_quiet
            AND octet_length(body)<=1048576 AND jsonb_array_length(doc)=6
            AND doc->>0='edgeagent.stream-owner-inputs.v1' AND doc->>1=target
            AND doc->>2=encode(admission,'hex') AND (doc->>5)::bigint=expiry
            AND encode(sha256(decode(doc->>3,'hex')),'hex')=(convert_from(admission,'UTF8')::jsonb->>'configuration')
            AND expiry>floor(extract(epoch FROM clock_timestamp())*1000)::bigint
            AND expiry<=(convert_from(admission,'UTF8')::jsonb->'window'->>1)::bigint
            AND jsonb_array_length(doc->4) BETWEEN 1 AND 128
            AND (SELECT count(DISTINCT c->>0)=count(*) AND count(DISTINCT c->>1)=count(*) FROM jsonb_array_elements(doc->4) c)
            AND NOT EXISTS (SELECT 1 FROM jsonb_array_elements(doc->4) c
                LEFT JOIN edgeagent_stream_namespaces n ON n.namespace=c->>0
                WHERE n.namespace IS NULL OR n.retired OR n.scope IS DISTINCT FROM decode(c->>1,'hex'))
        FROM context WHERE action='install'
        ON CONFLICT(session_id) DO UPDATE SET verified=
            edgeagent_stream_owner_inputs.inputs=EXCLUDED.inputs
            AND edgeagent_stream_owner_inputs.expires_ms=EXCLUDED.expires_ms
            AND NOT edgeagent_stream_owner_inputs.retired AND EXCLUDED.verified;
    -- A false/NULL verified value violates a constraint, rolling back ALL namespace
    -- reservations even for direct SQL/autocommit callers, not only Rust callers.
    INSERT INTO edgeagent_stream_namespace_members(session_id,namespace)
        SELECT target,c->>0 FROM jsonb_array_elements(CASE WHEN action='install' THEN convert_from(body,'UTF8')::jsonb->4 ELSE NULL END) c
        JOIN edgeagent_stream_owner_inputs p ON p.session_id=target AND p.inputs=body AND NOT p.retired
        ON CONFLICT DO NOTHING;
    UPDATE edgeagent_stream_owner_inputs p SET inputs=NULL,retired=TRUE
        FROM edgeagent_stream_admissions a JOIN edgeagent_stream_retention r USING(session_id)
        WHERE action='retire' AND p.session_id=target AND a.session_id=target
        AND session_user=a.approved_session AND r.writers_quiet;
    DELETE FROM edgeagent_stream_namespace_members m USING edgeagent_stream_owner_inputs p
        WHERE action='retire' AND m.session_id=target AND p.session_id=target AND p.retired;
    UPDATE edgeagent_stream_namespaces n SET scope=NULL,retired=TRUE
        WHERE action='retire' AND NOT n.retired
        AND EXISTS(SELECT 1 FROM edgeagent_stream_owner_inputs p JOIN edgeagent_stream_admissions a USING(session_id)
            WHERE p.session_id=target AND p.retired AND session_user=a.approved_session)
        AND NOT EXISTS(SELECT 1 FROM edgeagent_stream_namespace_members m WHERE m.namespace=n.namespace);
    SELECT EXISTS(SELECT 1 FROM edgeagent_stream_owner_inputs p JOIN edgeagent_stream_admissions a USING(session_id)
        WHERE p.session_id=target AND session_user=a.approved_session
        AND ((action='install' AND NOT p.retired AND p.inputs=body AND p.expires_ms=expiry) OR (action='retire' AND p.retired)));
END;
REVOKE ALL ON FUNCTION edgeagent_stream_owner_inputs_control(TEXT,TEXT,BYTEA,BIGINT) FROM PUBLIC;

CREATE FUNCTION edgeagent_load_stream_inputs(target TEXT) RETURNS BYTEA
LANGUAGE SQL SECURITY DEFINER SET search_path=pg_catalog
BEGIN ATOMIC
    SELECT p.inputs FROM edgeagent_stream_owner_inputs p
    JOIN edgeagent_stream_admissions a USING(session_id) JOIN edgeagent_stream_accounts account USING(account_id)
    JOIN edgeagent_stream_sessions s USING(session_id) JOIN edgeagent_stream_retention r USING(session_id)
    WHERE p.session_id=target AND NOT p.retired AND NOT a.withdrawn AND NOT account.withdrawn
        AND NOT s.stopped AND NOT r.denied AND NOT r.writers_quiet
        AND p.expires_ms>floor(extract(epoch FROM clock_timestamp())*1000)::bigint
        AND session_user<>a.approved_session AND session_user<>account.approved_session
        AND NOT pg_has_role(session_user,a.approved_session,'MEMBER') AND NOT pg_has_role(session_user,account.approved_session,'MEMBER')
        AND NOT EXISTS(SELECT 1 FROM jsonb_array_elements(convert_from(p.inputs,'UTF8')::jsonb->4) claim
            LEFT JOIN edgeagent_stream_namespace_members member ON member.session_id=p.session_id AND member.namespace=claim->>0
            LEFT JOIN edgeagent_stream_namespaces n ON n.namespace=member.namespace
            WHERE member.namespace IS NULL OR n.retired OR n.scope IS DISTINCT FROM decode(claim->>1,'hex'))
        AND (SELECT count(*) FROM edgeagent_stream_namespace_members member WHERE member.session_id=p.session_id)
            =jsonb_array_length(convert_from(p.inputs,'UTF8')::jsonb->4)
        AND NOT EXISTS(SELECT 1 FROM pg_roles principal,pg_class c
            WHERE c.oid IN ('edgeagent_stream_owner_inputs'::regclass,'edgeagent_stream_namespaces'::regclass,'edgeagent_stream_namespace_members'::regclass)
            AND pg_has_role(session_user,principal.oid,'MEMBER')
            AND (has_schema_privilege(principal.oid,c.relnamespace,'CREATE')
                OR has_table_privilege(principal.oid,c.oid,'INSERT,UPDATE,DELETE,TRUNCATE,TRIGGER,REFERENCES')
                OR EXISTS(SELECT 1 FROM pg_attribute col WHERE col.attrelid=c.oid AND col.attnum>0 AND NOT col.attisdropped
                    AND has_column_privilege(principal.oid,c.oid,col.attnum,'INSERT,UPDATE,REFERENCES'))))
    FOR SHARE OF p,a,account,s,r;
END;
REVOKE ALL ON FUNCTION edgeagent_load_stream_inputs(TEXT) FROM PUBLIC;
