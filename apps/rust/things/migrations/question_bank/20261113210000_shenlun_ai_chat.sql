-- Preserve every existing 申论 AI job while adding per-question text/image follow-up.
-- SQLite cannot ALTER an existing CHECK on kind; rebuild only this isolated table.
CREATE TABLE shenlun_jobs_next (
    id TEXT PRIMARY KEY,
    request_id TEXT NOT NULL UNIQUE,
    attempt_id TEXT NOT NULL REFERENCES shenlun_attempts(id),
    kind TEXT NOT NULL CHECK(kind IN ('transcribe','grade','chat','independent_answer','rewrite','full_essay')),
    status TEXT NOT NULL CHECK(status IN ('running','complete','failed','stopped','unknown')),
    source_hash TEXT NOT NULL,
    attempt_revision INTEGER NOT NULL,
    skill_version TEXT NOT NULL,
    model TEXT NOT NULL DEFAULT 'codex-app-server',
    result_json TEXT,
    error TEXT,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    request_fingerprint TEXT NOT NULL DEFAULT '',
    acknowledged_at_ms INTEGER,
    prompt_text TEXT NOT NULL DEFAULT ''
);
INSERT INTO shenlun_jobs_next (
    id,request_id,attempt_id,kind,status,source_hash,attempt_revision,skill_version,model,
    result_json,error,created_at_ms,updated_at_ms,request_fingerprint,acknowledged_at_ms,prompt_text
)
SELECT id,request_id,attempt_id,kind,status,source_hash,attempt_revision,skill_version,model,
    result_json,error,created_at_ms,updated_at_ms,request_fingerprint,acknowledged_at_ms,''
FROM shenlun_jobs;
DROP TABLE shenlun_jobs;
ALTER TABLE shenlun_jobs_next RENAME TO shenlun_jobs;
CREATE INDEX idx_shenlun_jobs_attempt ON shenlun_jobs(attempt_id, created_at_ms DESC);
CREATE UNIQUE INDEX idx_shenlun_jobs_one_running_per_attempt
ON shenlun_jobs(attempt_id) WHERE status='running';
