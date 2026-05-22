UPDATE public.gaussdb_cdc_failover SET val = 'c' WHERE id = 2;
DELETE FROM public.gaussdb_cdc_failover WHERE id = 1;

