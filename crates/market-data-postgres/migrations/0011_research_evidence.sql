CREATE TABLE edgeagent_research_evidence (
    caller_scope VARCHAR(64) NOT NULL,
    request_id VARCHAR(64) NOT NULL,
    request BYTEA NOT NULL CHECK (octet_length(request) BETWEEN 1 AND 131072),
    delivery BYTEA NOT NULL CHECK (octet_length(delivery) BETWEEN 1 AND 262144),
    acquisition_id VARCHAR(32) NOT NULL REFERENCES edgeagent_acquisitions(request_id) ON DELETE CASCADE,
    acquisition_digest VARCHAR(64) NOT NULL,
    admission BYTEA NOT NULL CHECK (octet_length(admission) BETWEEN 1 AND 8192),
    publication BYTEA CHECK (octet_length(publication) BETWEEN 1 AND 262144),
    PRIMARY KEY (caller_scope,request_id)
);
-- Opaque tombstones outlive owner erasure without storing content/authority digests.
CREATE TABLE edgeagent_research_evidence_denials (
    caller_scope VARCHAR(64) NOT NULL,
    request_id VARCHAR(64) NOT NULL,
    PRIMARY KEY (caller_scope,request_id)
);
CREATE FUNCTION edgeagent_research_evidence_immutable() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        IF NOT EXISTS(SELECT 1 FROM edgeagent_acquisition_denials WHERE request_id=OLD.acquisition_id) THEN
            RAISE EXCEPTION 'research evidence bindings are immutable' USING ERRCODE = '23514';
        END IF;
        -- Serialize retirement with intake/completion of this scoped request.
        PERFORM pg_advisory_xact_lock(hashtextextended('edgeagent.research-evidence/'||OLD.caller_scope||'/'||OLD.request_id,0));
        INSERT INTO edgeagent_research_evidence_denials(caller_scope,request_id)
            VALUES(OLD.caller_scope,OLD.request_id) ON CONFLICT DO NOTHING;
        DELETE FROM edgeagent_message_inbox
            WHERE consumer_name='market-data-research'
            AND convert_from(envelope,'UTF8')::jsonb #>> '{data,data,scope}'=OLD.caller_scope
            AND convert_from(envelope,'UTF8')::jsonb #>> '{data,data,request_id}'=OLD.request_id;
        DELETE FROM edgeagent_message_outbox
            WHERE message_source='urn:edgeagent:component:market-data'
            AND message_type='com.edgeagent.market-data.research-evidence-decided.v1'
            AND convert_from(envelope,'UTF8')::jsonb #>> '{data,request,scope}'=OLD.caller_scope
            AND convert_from(envelope,'UTF8')::jsonb #>> '{data,request,request_id}'=OLD.request_id;
        RETURN OLD;
    END IF;
    IF ROW(OLD.caller_scope,OLD.request_id,OLD.request,OLD.delivery,OLD.acquisition_id,OLD.acquisition_digest,OLD.admission)
       IS DISTINCT FROM ROW(NEW.caller_scope,NEW.request_id,NEW.request,NEW.delivery,NEW.acquisition_id,NEW.acquisition_digest,NEW.admission)
       OR OLD.publication IS NOT NULL OR NEW.publication IS NULL THEN
        RAISE EXCEPTION 'research evidence bindings are immutable' USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER edgeagent_research_evidence_immutable BEFORE UPDATE OR DELETE ON edgeagent_research_evidence
    FOR EACH ROW EXECUTE FUNCTION edgeagent_research_evidence_immutable();
