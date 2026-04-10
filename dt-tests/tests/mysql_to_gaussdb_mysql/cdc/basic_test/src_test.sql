INSERT INTO gaussdb_mysql_cdc_basic.cdc_basic VALUES (1, 'a'), (2, 'b'), (3, 'c');
UPDATE gaussdb_mysql_cdc_basic.cdc_basic SET val = 'bb' WHERE id = 2;
DELETE FROM gaussdb_mysql_cdc_basic.cdc_basic WHERE id = 3;
