-- AI operations are isolated from the original multiple-choice practice and chat tables.
-- A job is immutable once completed; one client request ID is one paid execution.
CREATE TABLE shenlun_jobs (
    id TEXT PRIMARY KEY,
    request_id TEXT NOT NULL UNIQUE,
    attempt_id TEXT NOT NULL REFERENCES shenlun_attempts(id),
    kind TEXT NOT NULL CHECK(kind IN ('transcribe','grade','independent_answer','rewrite','full_essay')),
    status TEXT NOT NULL CHECK(status IN ('running','complete','failed','stopped','unknown')),
    source_hash TEXT NOT NULL,
    attempt_revision INTEGER NOT NULL,
    skill_version TEXT NOT NULL,
    model TEXT NOT NULL DEFAULT 'codex-app-server',
    result_json TEXT,
    error TEXT,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
);
CREATE INDEX idx_shenlun_jobs_attempt ON shenlun_jobs(attempt_id, created_at_ms DESC);

-- Independent of the student's text: multiple drafts use the same grading rubric.
CREATE TABLE shenlun_rubrics (
    question_id TEXT NOT NULL REFERENCES questions(id),
    source_hash TEXT NOT NULL,
    skill_version TEXT NOT NULL,
    rubric_json TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL,
    PRIMARY KEY(question_id, source_hash, skill_version)
);

-- This flag marks later revisions after source/model answers have been revealed.
ALTER TABLE shenlun_attempts
    ADD COLUMN reference_viewed_at_ms INTEGER;
