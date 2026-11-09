-- Paper subject is independent of kind. Existing papers are 行测.
ALTER TABLE papers ADD COLUMN subject TEXT NOT NULL DEFAULT 'xingce';

-- Existing questions are objective exact-match. Subjective 申论 questions
-- store their source reference answer server-side and are never gradable.
ALTER TABLE questions ADD COLUMN answer_type TEXT NOT NULL DEFAULT 'objective';
ALTER TABLE questions ADD COLUMN question_type TEXT;
ALTER TABLE questions ADD COLUMN grading_mode TEXT NOT NULL DEFAULT 'exact';
ALTER TABLE questions ADD COLUMN max_score REAL;
ALTER TABLE questions ADD COLUMN reference_answer TEXT;

CREATE INDEX IF NOT EXISTS idx_bank_papers_subject ON papers(subject);
