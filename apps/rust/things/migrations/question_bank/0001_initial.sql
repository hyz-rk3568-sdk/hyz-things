-- Question Bank has an independent SQLite database. Do not put this migration
-- in the portal/Flashcard migration directory or rebuild user practice tables.
-- CREATE IF NOT EXISTS preserves older databases containing papers/identities.
CREATE TABLE IF NOT EXISTS papers (
    id TEXT PRIMARY KEY,
    metadata TEXT NOT NULL,
    active INTEGER NOT NULL DEFAULT 1
);
CREATE TABLE IF NOT EXISTS identities (
    id TEXT PRIMARY KEY,
    paper_id TEXT NOT NULL REFERENCES papers(id)
);
CREATE TABLE IF NOT EXISTS questions (
    id TEXT PRIMARY KEY,
    paper_id TEXT NOT NULL REFERENCES papers(id),
    number INTEGER NOT NULL,
    source_hash TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_bank_questions_paper_number
    ON questions(paper_id, number);
CREATE TABLE IF NOT EXISTS practice_sessions (
    id TEXT PRIMARY KEY,
    status TEXT NOT NULL,
    revision INTEGER NOT NULL,
    data TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS practice_items (
    session_id TEXT NOT NULL REFERENCES practice_sessions(id),
    ordinal INTEGER NOT NULL,
    question_id TEXT NOT NULL,
    paper_id TEXT NOT NULL,
    number INTEGER NOT NULL,
    source_hash TEXT NOT NULL,
    answer TEXT NOT NULL,
    grade TEXT,
    PRIMARY KEY (session_id, ordinal)
);
CREATE TABLE IF NOT EXISTS practice_attempts (
    session_id TEXT NOT NULL,
    question_id TEXT NOT NULL,
    answer TEXT NOT NULL,
    grade TEXT NOT NULL,
    submitted_at INTEGER NOT NULL,
    PRIMARY KEY (session_id, question_id)
);
CREATE TABLE IF NOT EXISTS question_progress (
    question_id TEXT PRIMARY KEY,
    pending INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS favorites (
    question_id TEXT PRIMARY KEY,
    favorite INTEGER NOT NULL
);
