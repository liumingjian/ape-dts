gaussdb_mysql_struct_basic
CREATE DATABASE gaussdb_mysql_struct_basic DEFAULT CHARACTER SET UTF8 COLLATE utf8mb4_0900_ai_ci

gaussdb_mysql_struct_basic.struct_basic
SET search_path = gaussdb_mysql_struct_basic;
CREATE TABLE struct_basic (
    id integer NOT NULL,
    name varchar(32) CHARACTER SET `UTF8` COLLATE utf8mb4_0900_ai_ci NOT NULL,
    amount decimal(10,2) DEFAULT (0.00)
)
CHARACTER SET = "UTF8" COLLATE = "utf8mb4_0900_ai_ci"
WITH (orientation=row, compression=no, storage_type=USTORE, segment=off);
COMMENT ON TABLE struct_basic IS 'struct_basic_comment';
COMMENT ON COLUMN struct_basic.name IS 'name_comment';
CREATE INDEX idx_name USING ubtree ON gaussdb_mysql_struct_basic.struct_basic (name) WITH (storage_type=USTORE) TABLESPACE pg_default;
ALTER TABLE struct_basic ADD CONSTRAINT struct_basic_pkey PRIMARY KEY USING ubtree  (id) WITH (storage_type=USTORE);
