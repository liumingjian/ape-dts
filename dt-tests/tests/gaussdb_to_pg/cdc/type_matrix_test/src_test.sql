INSERT INTO public.gaussdb_cdc_type_matrix (
  id, ts_col, tiny_col, nvarchar_col, clob_col, blob_col
) VALUES
  (1, '2026-04-02 16:20:00', 7, 'alpha', 'first clob text', decode('0001ff', 'hex')),
  (2, NULL, NULL, 'beta', NULL, NULL);

UPDATE public.gaussdb_cdc_type_matrix
SET
  ts_col = '2026-04-02 16:21:00',
  tiny_col = 8,
  nvarchar_col = 'alpha-updated',
  clob_col = 'updated clob text',
  blob_col = decode('00a1ff', 'hex')
WHERE id = 1;

DELETE FROM public.gaussdb_cdc_type_matrix WHERE id = 2;
