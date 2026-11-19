CREATE TABLE ai_practice_favorites (
    module TEXT NOT NULL CHECK (module IN ('data_analysis', 'logic_judgment', 'quantitative_relations')),
    session_id TEXT NOT NULL,
    question_no INTEGER NOT NULL CHECK (question_no >= 0),
    created_at_ms INTEGER NOT NULL,
    PRIMARY KEY (module, session_id, question_no)
);

CREATE TABLE ai_practice_agent_threads (
    module TEXT NOT NULL CHECK (module IN ('data_analysis', 'logic_judgment', 'quantitative_relations')),
    session_id TEXT NOT NULL,
    question_no INTEGER NOT NULL CHECK (question_no >= 0),
    thread_id TEXT,
    status TEXT NOT NULL CHECK (status IN ('idle', 'running', 'complete', 'failed', 'stopped')),
    partial TEXT NOT NULL DEFAULT '',
    error TEXT,
    PRIMARY KEY (module, session_id, question_no)
);
CREATE TABLE ai_practice_agent_messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    module TEXT NOT NULL,
    session_id TEXT NOT NULL,
    question_no INTEGER NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('user', 'assistant')),
    content TEXT NOT NULL,
    FOREIGN KEY (module, session_id, question_no) REFERENCES ai_practice_agent_threads(module, session_id, question_no)
);
CREATE INDEX ai_practice_agent_messages_question ON ai_practice_agent_messages(module, session_id, question_no, id);
