CREATE TABLE edgeagent_gateway_requests (
    caller_scope VARCHAR(64) NOT NULL,
    request_id VARCHAR(64) NOT NULL,
    request BYTEA NOT NULL CHECK (octet_length(request) BETWEEN 1 AND 131072),
    command BYTEA NOT NULL CHECK (octet_length(command) BETWEEN 1 AND 262144),
    PRIMARY KEY (caller_scope, request_id)
);

CREATE FUNCTION edgeagent_gateway_immutable() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'gateway admissions are immutable' USING ERRCODE = '23514';
END;
$$;
CREATE TRIGGER edgeagent_gateway_immutable
    BEFORE UPDATE OR DELETE ON edgeagent_gateway_requests
    FOR EACH ROW EXECUTE FUNCTION edgeagent_gateway_immutable();
