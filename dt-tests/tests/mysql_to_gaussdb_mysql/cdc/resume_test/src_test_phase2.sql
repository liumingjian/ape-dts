UPDATE gaussdb_mysql_cdc_resume.cdc_resume SET val = 'bb' WHERE id = 2;
DELETE FROM gaussdb_mysql_cdc_resume.cdc_resume WHERE id = 1;
INSERT INTO gaussdb_mysql_cdc_resume.cdc_resume VALUES (3, 'c');

