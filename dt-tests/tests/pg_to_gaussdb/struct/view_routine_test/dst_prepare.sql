DO $ape_dts$
BEGIN
  BEGIN
    EXECUTE 'DROP MATERIALIZED VIEW IF EXISTS public.gaussdb_struct_vr_matview';
  EXCEPTION WHEN OTHERS THEN
    EXECUTE 'DROP VIEW IF EXISTS public.gaussdb_struct_vr_matview';
  END;
END
$ape_dts$;
DROP VIEW IF EXISTS public.gaussdb_struct_vr_view;
DROP PROCEDURE IF EXISTS public.gaussdb_struct_vr_proc();
DROP FUNCTION IF EXISTS public.gaussdb_struct_vr_func();
DROP TABLE IF EXISTS public.gaussdb_struct_vr_base;
