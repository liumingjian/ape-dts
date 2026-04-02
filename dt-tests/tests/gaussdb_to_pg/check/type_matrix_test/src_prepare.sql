DROP TABLE IF EXISTS public.gaussdb_type_matrix_check;
CREATE TABLE public.gaussdb_type_matrix_check (
  id INTEGER PRIMARY KEY,
  ts_col smalldatetime,
  tiny_col tinyint,
  nvarchar_col nvarchar2(32),
  clob_col clob,
  blob_col blob
);
