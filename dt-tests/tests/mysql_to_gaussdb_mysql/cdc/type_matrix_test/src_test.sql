INSERT INTO gaussdb_mysql_cdc_type_matrix.type_matrix
    (id, c_int, c_big, c_dec, c_double, c_varchar, c_text, c_datetime, c_timestamp, c_date, c_time, c_json)
VALUES
    (1, -1, 9223372036854770000, 123.45, 3.14159, 'hello', 'text', '2026-04-10 12:34:56', '2026-04-10 12:34:56', '2026-04-10', '12:34:56', '{"k":"v"}'),
    (2, NULL, 0, 0.00, 0.0, '', 'unicode 中文', NULL, NULL, NULL, NULL, 'null');

UPDATE gaussdb_mysql_cdc_type_matrix.type_matrix
SET
    c_varchar = 'hello2',
    c_dec = 999.99,
    c_text = 'text2',
    c_datetime = '2026-04-10 12:35:56',
    c_json = '{"k":"vv"}'
WHERE id = 1;

DELETE FROM gaussdb_mysql_cdc_type_matrix.type_matrix WHERE id = 2;

