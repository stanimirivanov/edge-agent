ALTER TABLE edgeagent_research_approvals ADD COLUMN promotion BYTEA
    CHECK (promotion IS NULL OR (revision=2 AND octet_length(promotion) BETWEEN 1 AND 4096));
-- Pre-migration approvals keep explicit absence. PostgreSQL enforces this
-- constraint on all subsequent inserts, including inserts from old binaries.
ALTER TABLE edgeagent_research_approvals ADD CONSTRAINT edgeagent_research_promotion_required
    CHECK (revision<>2 OR promotion IS NOT NULL) NOT VALID;
