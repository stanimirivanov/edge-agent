-- Owner-private denial identities contain no licensed content fingerprints.
-- Refuse before any mutation when legacy content lacks an ownership inventory.
DO $$ BEGIN
    IF EXISTS(SELECT 1 FROM edgeagent_evidence_heads) OR EXISTS(SELECT 1 FROM edgeagent_observation_versions)
    THEN RAISE EXCEPTION 'retention migration requires inventoried legacy publication metadata'; END IF;
END $$;
CREATE TABLE edgeagent_evidence_scopes (
    scope TEXT PRIMARY KEY,
    revoked BOOLEAN NOT NULL DEFAULT FALSE
);
CREATE SEQUENCE edgeagent_evidence_cleanup_order;
CREATE TABLE edgeagent_acquisition_denials (
    request_id TEXT PRIMARY KEY CHECK (request_id ~ '^[0-9a-f]{32}$'),
    cleaned BOOLEAN NOT NULL DEFAULT FALSE,
    cleanup_order BIGINT NOT NULL DEFAULT nextval('edgeagent_evidence_cleanup_order')
);
CREATE INDEX edgeagent_acquisition_cleanup_queue ON edgeagent_acquisition_denials(cleanup_order) WHERE NOT cleaned;
ALTER TABLE edgeagent_acquisition_accounts ADD COLUMN blocked BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE edgeagent_acquisition_accounts ADD COLUMN block_generation BIGINT NOT NULL DEFAULT 0 CHECK (block_generation >= 0);
ALTER TABLE edgeagent_acquisitions ADD COLUMN retention_scope TEXT REFERENCES edgeagent_evidence_scopes(scope);
ALTER TABLE edgeagent_acquisitions ADD COLUMN delete_ms BIGINT;
ALTER TABLE edgeagent_acquisitions ADD COLUMN orphan_ms BIGINT;
-- Before activation, legacy records must be inventoried rather than silently forgotten.
INSERT INTO edgeagent_evidence_scopes(scope)
SELECT DISTINCT concat(j->>'storage','/',j->>'audience','/',split_part(j->>'rights',':',1))
FROM (SELECT convert_from(admission,'UTF8')::jsonb AS j FROM edgeagent_acquisitions) a;
UPDATE edgeagent_acquisitions SET
    retention_scope=concat(j->>'storage','/',j->>'audience','/',split_part(j->>'rights',':',1)),
    delete_ms=(j->>'delete_ms')::bigint,
    orphan_ms=least((j->>'delete_ms')::bigint,(j->>'deadline_ms')::bigint)
FROM (SELECT request_id,convert_from(admission,'UTF8')::jsonb AS j FROM edgeagent_acquisitions) a
WHERE a.request_id=edgeagent_acquisitions.request_id;
ALTER TABLE edgeagent_acquisitions ALTER COLUMN retention_scope SET NOT NULL;
ALTER TABLE edgeagent_acquisitions ALTER COLUMN delete_ms SET NOT NULL;
ALTER TABLE edgeagent_acquisitions ALTER COLUMN orphan_ms SET NOT NULL;
CREATE INDEX edgeagent_acquisitions_retention ON edgeagent_acquisitions(delete_ms,request_id);
-- Kept after command erasure only until quiescent, confirmed physical deletion.
CREATE TABLE edgeagent_evidence_objects (
    object_key TEXT PRIMARY KEY,
    request_id TEXT NOT NULL CHECK (request_id ~ '^[0-9a-f]{32}$'),
    descriptor BYTEA NOT NULL CHECK (octet_length(descriptor) BETWEEN 1 AND 2048),
    -- A damaged descriptor must never redirect maintenance to another immutable key.
    CHECK (coalesce(object_key =
        (convert_from(descriptor,'UTF8')::jsonb ->> 'storage') || '/' ||
        (convert_from(descriptor,'UTF8')::jsonb ->> 'audience') || '/' ||
        (convert_from(descriptor,'UTF8')::jsonb #>> '{rights,id}') || '/' ||
        (convert_from(descriptor,'UTF8')::jsonb #>> '{rights,revision}') || '/' ||
        (convert_from(descriptor,'UTF8')::jsonb ->> 'sha256'), FALSE))
);
CREATE INDEX edgeagent_evidence_objects_request ON edgeagent_evidence_objects(request_id);
CREATE TABLE edgeagent_evidence_ownership (
    request_id TEXT PRIMARY KEY REFERENCES edgeagent_acquisitions(request_id) ON DELETE CASCADE,
    series_key TEXT NOT NULL,
    observations TEXT[] NOT NULL CHECK (cardinality(observations) <= 252)
);
ALTER TABLE edgeagent_evidence_heads ADD COLUMN owner_request TEXT;
CREATE INDEX edgeagent_evidence_ownership_series ON edgeagent_evidence_ownership(series_key);
CREATE INDEX edgeagent_evidence_ownership_observations ON edgeagent_evidence_ownership USING gin(observations);
