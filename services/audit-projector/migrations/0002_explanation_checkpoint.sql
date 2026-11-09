-- Allow the full reducer bound (5 * 131072 + 1024 + 2 * 16384).
ALTER TABLE edgeagent_research_views
    DROP CONSTRAINT edgeagent_research_views_checkpoint_check,
    ADD CONSTRAINT edgeagent_research_views_checkpoint_check
        CHECK (octet_length(checkpoint) BETWEEN 32 AND 689152);
