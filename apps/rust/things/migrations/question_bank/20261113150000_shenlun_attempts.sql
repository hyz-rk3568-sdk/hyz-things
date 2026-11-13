-- Isolated 申论 subjective attempts. Do not modify the A-Z practice tables.
CREATE TABLE shenlun_attempts (
    id TEXT PRIMARY KEY,
    paper_id TEXT NOT NULL REFERENCES papers(id),
    question_id TEXT NOT NULL REFERENCES questions(id),
    source_hash TEXT NOT NULL,
    answer_text TEXT NOT NULL DEFAULT '',
    source_kind TEXT NOT NULL DEFAULT 'typed'
        CHECK(source_kind IN ('typed', 'vision-confirmed')),
    status TEXT NOT NULL DEFAULT 'draft'
        CHECK(status IN ('draft', 'submitted')),
    revision INTEGER NOT NULL DEFAULT 0 CHECK(revision >= 0),
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    submitted_at_ms INTEGER,
    CHECK(length(answer_text) <= 50000)
);
CREATE INDEX idx_shenlun_question_attempt ON shenlun_attempts(question_id, updated_at_ms DESC);

-- Submitted revisions are immutable. Reworking a question creates a new attempt.
CREATE TABLE shenlun_attempt_revisions (
    attempt_id TEXT NOT NULL REFERENCES shenlun_attempts(id),
    revision INTEGER NOT NULL,
    answer_text TEXT NOT NULL,
    source_kind TEXT NOT NULL CHECK(source_kind IN ('typed', 'vision-confirmed')),
    submitted_at_ms INTEGER NOT NULL,
    PRIMARY KEY(attempt_id, revision)
);
