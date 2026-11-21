ALTER TABLE edgeagent_explanation_attempts
    DROP CONSTRAINT edgeagent_explanation_attempts_check,
    ADD CONSTRAINT edgeagent_explanation_attempts_deadline_bound
        CHECK (deadline_ms > reserved_at_ms AND deadline_ms - reserved_at_ms <= 600000);
