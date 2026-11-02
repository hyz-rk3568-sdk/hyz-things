-- Question Bank's own SQLite database: safe upgrade from the legacy identity index.
-- Existing practice rows, history, favorites and wrong-question state are never rebuilt.
ALTER TABLE questions ADD COLUMN module TEXT;
ALTER TABLE questions ADD COLUMN category TEXT;
CREATE INDEX IF NOT EXISTS idx_bank_questions_taxonomy
    ON questions(module, category, paper_id, number);
CREATE INDEX IF NOT EXISTS idx_bank_attempts_question
    ON practice_attempts(question_id);
