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

CREATE TABLE public.gaussdb_struct_vr_base (
  id INTEGER PRIMARY KEY,
  val TEXT
);

CREATE OR REPLACE FUNCTION public.gaussdb_struct_vr_func()
RETURNS integer
LANGUAGE sql
AS 'SELECT 42';

-- NOTE: keep the whole procedure in one statement so the dt-tests line-based SQL parser
-- doesn't split on inner semicolons. The trailing ';' is preserved by appending a block
-- comment and an extra ';' terminator (only the last ';' is stripped by the parser).
CREATE OR REPLACE PROCEDURE public.gaussdb_struct_vr_proc() AS BEGIN PERFORM 1; END; /*ape_dts*/;

CREATE VIEW public.gaussdb_struct_vr_view AS
SELECT id, val FROM public.gaussdb_struct_vr_base;

-- NOTE: the current HCS GaussDB environment enables ustore by default and does not support
-- materialized views ("materialized view is not supported in ustore yet"). We still create a
-- view with the same name so struct sync can validate cross-engine view compatibility.
CREATE VIEW public.gaussdb_struct_vr_matview AS
SELECT id, val FROM public.gaussdb_struct_vr_base;
