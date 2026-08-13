DROP SCHEMA IF EXISTS test_db_1 CASCADE;

CREATE SCHEMA test_db_1;

CREATE TABLE test_db_1.toast_table ( f_0 serial, f_1 text, f_2 int, PRIMARY KEY (f_0) );
-- EXTERNAL keeps f_1 out of line and uncompressed, so any value over ~2KB is TOASTed
-- and an update that does not touch f_1 leaves it out of the WAL record.
ALTER TABLE test_db_1.toast_table ALTER COLUMN f_1 SET STORAGE EXTERNAL;
