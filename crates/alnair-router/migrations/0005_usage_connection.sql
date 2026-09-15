-- Which upstream connection served each usage row. Stored as a snapshot so
-- history survives renames and deletions.

ALTER TABLE usage_records ADD COLUMN connection_name TEXT;
