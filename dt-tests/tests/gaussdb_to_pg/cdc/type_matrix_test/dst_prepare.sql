DROP TABLE IF EXISTS public.gaussdb_cdc_type_matrix;
CREATE TABLE public.gaussdb_cdc_type_matrix (
  id INTEGER PRIMARY KEY,
  ts_col TIMESTAMP,
  tiny_col SMALLINT,
  nvarchar_col VARCHAR(32),
  clob_col TEXT,
  blob_col BYTEA
);
