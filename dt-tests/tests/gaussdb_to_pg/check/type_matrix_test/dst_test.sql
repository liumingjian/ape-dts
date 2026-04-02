INSERT INTO public.gaussdb_type_matrix_check (
  id, ts_col, tiny_col, nvarchar_col, clob_col, blob_col
) VALUES
  (1, '2026-04-02 13:20:00', 7, 'alpha', 'first clob text', decode('0001ff', 'hex')),
  (2, NULL, NULL, 'beta', NULL, NULL);
