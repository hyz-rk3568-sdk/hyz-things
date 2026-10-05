CREATE TABLE flashcards (
    id TEXT PRIMARY KEY NOT NULL,
    source_file TEXT NOT NULL,
    source_order INTEGER NOT NULL,
    deck_path TEXT NOT NULL,
    front_markdown TEXT NOT NULL,
    back_markdown TEXT NOT NULL,
    source_hash TEXT NOT NULL,
    active INTEGER NOT NULL CHECK (active IN (0, 1)),
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
);

CREATE TABLE flashcard_card_tags (
    card_id TEXT NOT NULL,
    tag TEXT NOT NULL,
    PRIMARY KEY (card_id, tag),
    FOREIGN KEY (card_id) REFERENCES flashcards(id) ON DELETE CASCADE
);

CREATE TABLE flashcard_review_state (
    card_id TEXT PRIMARY KEY NOT NULL,
    due_at_ms INTEGER NOT NULL,
    last_reviewed_at_ms INTEGER,
    stability REAL NOT NULL,
    difficulty REAL NOT NULL,
    state INTEGER NOT NULL CHECK (state BETWEEN 0 AND 3),
    review_count INTEGER NOT NULL,
    lapse_count INTEGER NOT NULL,
    FOREIGN KEY (card_id) REFERENCES flashcards(id) ON DELETE CASCADE
);

CREATE TABLE flashcard_review_logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    card_id TEXT NOT NULL,
    reviewed_at_ms INTEGER NOT NULL,
    rating TEXT NOT NULL CHECK (rating IN ('again', 'hard', 'good')),
    state INTEGER NOT NULL CHECK (state BETWEEN 0 AND 3),
    due_at_ms INTEGER NOT NULL,
    stability REAL NOT NULL,
    difficulty REAL NOT NULL,
    FOREIGN KEY (card_id) REFERENCES flashcards(id) ON DELETE CASCADE
);

CREATE INDEX flashcards_active_deck_idx ON flashcards(active, deck_path);
CREATE INDEX flashcard_card_tags_tag_idx ON flashcard_card_tags(tag);
CREATE INDEX flashcard_review_state_due_idx ON flashcard_review_state(due_at_ms);
CREATE INDEX flashcard_review_logs_card_reviewed_idx ON flashcard_review_logs(card_id, reviewed_at_ms);
