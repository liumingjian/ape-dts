DROP TABLE IF EXISTS public.gaussdb_type_matrix_snapshot;
CREATE TABLE public.gaussdb_type_matrix_snapshot (
  id INTEGER PRIMARY KEY,
  ts_col TIMESTAMP,
  tiny_col SMALLINT,
  nvarchar_col VARCHAR(32),
  clob_col TEXT,
  blob_col BYTEA
);
