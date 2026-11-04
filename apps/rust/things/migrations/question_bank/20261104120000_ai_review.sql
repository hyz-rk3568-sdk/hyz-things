-- Persistent question-specific Codex conversations, separate from Flashcards and source Markdown.
CREATE TABLE question_ai_reviews (
    session_id TEXT NOT NULL REFERENCES practice_sessions(id),
    question_id TEXT NOT NULL,
    source_hash TEXT NOT NULL,
    thread_id TEXT,
    status TEXT NOT NULL DEFAULT 'idle' CHECK(status IN ('idle','running','complete','stopped','failed')),
    error TEXT,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY(session_id, question_id)
);
CREATE TABLE question_ai_messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    question_id TEXT NOT NULL,
    role TEXT NOT NULL CHECK(role IN ('user','assistant')),
    content TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    FOREIGN KEY(session_id, question_id) REFERENCES question_ai_reviews(session_id, question_id)
);
CREATE INDEX idx_question_ai_messages_review ON question_ai_messages(session_id,question_id,id);

-- Keep module counts and candidate selection on small index pages.
-- The TEXT question ID is explicit because secondary indexes otherwise carry rowid.
CREATE INDEX IF NOT EXISTS idx_bank_questions_catalog_cover
    ON questions(paper_id, module, category, number, id);
