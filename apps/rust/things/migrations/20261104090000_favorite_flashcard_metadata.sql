ALTER TABLE flashcards ADD COLUMN source_question_id TEXT;
ALTER TABLE flashcards ADD COLUMN correct_answer TEXT NOT NULL DEFAULT '[]';
