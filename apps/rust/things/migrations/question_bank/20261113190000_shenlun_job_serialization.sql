-- At most one active Codex job per submitted attempt; retries reuse the request ID.
CREATE UNIQUE INDEX idx_shenlun_jobs_one_running_per_attempt
ON shenlun_jobs(attempt_id) WHERE status='running';
