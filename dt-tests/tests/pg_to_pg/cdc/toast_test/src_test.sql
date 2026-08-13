INSERT INTO test_db_1.toast_table VALUES(1, repeat('a', 10000), 1),(2, repeat('b', 10000), 1);

-- f_1 is not touched, so postgres sends it as an unchanged toast placeholder:
-- the sinker must leave the target column alone instead of nulling it.
UPDATE test_db_1.toast_table SET f_2 = 2;

-- an update that does rewrite the toasted column still carries its new value
UPDATE test_db_1.toast_table SET f_1 = repeat('c', 10000) WHERE f_0 = 2;

DELETE FROM test_db_1.toast_table WHERE f_0 = 1;
