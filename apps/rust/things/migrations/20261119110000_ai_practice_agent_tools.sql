ALTER TABLE ai_practice_agent_messages ADD COLUMN tool_name TEXT;
ALTER TABLE ai_practice_agent_messages ADD COLUMN tool_status TEXT;
ALTER TABLE ai_practice_agent_messages ADD COLUMN tool_detail TEXT;
ALTER TABLE ai_practice_agent_messages ADD COLUMN tool_item TEXT;
CREATE INDEX ai_practice_agent_messages_tool ON ai_practice_agent_messages(module, session_id, question_no, tool_item);
