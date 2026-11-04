-- Additive AI review storage. Kept outside the versioned migrator so a
-- previous ELF can still open the database after an OTA rollback.
CREATE TABLE IF NOT EXISTS question_ai_reviews (
    session_id TEXT NOT NULL REFERENCES practice_sessions(id),
    question_id TEXT NOT NULL,
    source_hash TEXT NOT NULL,
    thread_id TEXT,
    status TEXT NOT NULL DEFAULT 'idle' CHECK(status IN ('idle','running','complete','stopped','failed')),
    error TEXT,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY(session_id, question_id)
);
CREATE TABLE IF NOT EXISTS question_ai_messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    question_id TEXT NOT NULL,
    role TEXT NOT NULL CHECK(role IN ('user','assistant')),
    content TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    FOREIGN KEY(session_id, question_id) REFERENCES question_ai_reviews(session_id, question_id)
);
CREATE INDEX IF NOT EXISTS idx_question_ai_messages_review
    ON question_ai_messages(session_id,question_id,id);
