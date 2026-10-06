WITH candidate AS (
    SELECT consumer_name,
           delivery_key,
           transport_subject,
           payload,
           failure_code,
           first_delivery_attempt,
           last_delivery_attempt,
           quarantined_at,
           last_observed_at
    FROM edgeagent_message_quarantine
    WHERE consumer_name = $1 AND delivery_key = $2
    FOR UPDATE
),
audit AS (
    INSERT INTO edgeagent_message_quarantine_replay_audit (
        replay_request_id,
        consumer_name,
        delivery_key,
        requested_by,
        reason,
        prior_failure_code,
        prior_first_delivery_attempt,
        prior_last_delivery_attempt,
        prior_quarantined_at,
        prior_last_observed_at
    )
    SELECT $3,
           candidate.consumer_name,
           candidate.delivery_key,
           $4,
           $5,
           candidate.failure_code,
           candidate.first_delivery_attempt,
           candidate.last_delivery_attempt,
           candidate.quarantined_at,
           candidate.last_observed_at
    FROM candidate
    ON CONFLICT (replay_request_id) DO NOTHING
    RETURNING consumer_name, delivery_key
)
SELECT candidate.transport_subject AS "transport_subject!",
       candidate.payload AS "payload!"
FROM candidate
JOIN audit USING (consumer_name, delivery_key)
