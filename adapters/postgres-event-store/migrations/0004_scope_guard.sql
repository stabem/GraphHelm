-- Scope isolation hardening.
--
-- The scope policies compared each column directly against `current_setting(..., true)`. A custom
-- GUC set with `set_config(..., true)` is transaction-local, and at transaction end it reverts to
-- the empty string rather than becoming unset. On a pooled connection an unscoped query therefore
-- degraded from matching nothing to matching every row whose scope column is empty, and the
-- `WITH CHECK` twin would have permitted writing one. Nothing in the schema forbade such a row;
-- the fail-closed property rested entirely on application-side validation.
--
-- `NULLIF` restores it inside the database: an empty GUC now compares as NULL and selects no rows.
-- `execution_id` is deliberately excluded, because the empty string is its legitimate value for a
-- scope that has no execution. A CHECK constraint makes the invariant structural as well, so an
-- empty-scope row cannot exist even if a future policy regresses.
DO $graphhelm$
DECLARE scoped_table text;
BEGIN
  FOR scoped_table IN
    SELECT c.relname
    FROM pg_policy p
    JOIN pg_class c ON c.oid = p.polrelid
    JOIN pg_namespace n ON n.oid = c.relnamespace
    WHERE n.nspname = 'public' AND p.polname = 'graphhelm_scope'
    ORDER BY c.relname COLLATE "C"
  LOOP
    EXECUTE format('DROP POLICY graphhelm_scope ON public.%I', scoped_table);
    EXECUTE format(
      'CREATE POLICY graphhelm_scope ON public.%I USING (' ||
      'workspace_id = NULLIF(current_setting(''graphhelm.workspace_id'', true), '''') AND ' ||
      'project_id = NULLIF(current_setting(''graphhelm.project_id'', true), '''') AND ' ||
      'execution_id = COALESCE(current_setting(''graphhelm.execution_id'', true), '''')) ' ||
      'WITH CHECK (' ||
      'workspace_id = NULLIF(current_setting(''graphhelm.workspace_id'', true), '''') AND ' ||
      'project_id = NULLIF(current_setting(''graphhelm.project_id'', true), '''') AND ' ||
      'execution_id = COALESCE(current_setting(''graphhelm.execution_id'', true), ''''))',
      scoped_table
    );
    EXECUTE format(
      'ALTER TABLE public.%I ADD CONSTRAINT graphhelm_scope_not_empty ' ||
      'CHECK (workspace_id <> '''' AND project_id <> '''')',
      scoped_table
    );
  END LOOP;
END $graphhelm$;

-- The ledger function is versioned by arity, so a fourth migration needs a fourth parameter.
CREATE FUNCTION public.graphhelm_migrations_are_current(
  expected_one bytea, expected_two bytea, expected_three bytea, expected_four bytea
)
RETURNS boolean LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog,public AS $graphhelm$
  SELECT count(*)=4 AND bool_and(success) AND bool_and(
    CASE version
      WHEN 1 THEN checksum=expected_one
      WHEN 2 THEN checksum=expected_two
      WHEN 3 THEN checksum=expected_three
      WHEN 4 THEN checksum=expected_four
      ELSE false
    END
  )
  FROM public._sqlx_migrations
$graphhelm$;
REVOKE ALL ON FUNCTION public.graphhelm_migrations_are_current(bytea,bytea,bytea,bytea) FROM PUBLIC;

-- The superseded three-argument ledger function is removed rather than left executable. Retaining
-- it would leave a SECURITY DEFINER reader of `_sqlx_migrations` that reports a stale contract as
-- current.
DROP FUNCTION public.graphhelm_migrations_are_current(bytea,bytea,bytea);

-- Runtime role configuration revoked table privileges but never function privileges, so a role
-- configured against an earlier migration kept EXECUTE on superseded SECURITY DEFINER functions
-- after an upgrade. Revoking functions makes the routine idempotent across migration levels.
CREATE OR REPLACE FUNCTION public.graphhelm_configure_runtime_role(role_name name)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $graphhelm$
DECLARE role_oid oid;
BEGIN
  SELECT oid INTO role_oid FROM pg_roles WHERE rolname=role_name;
  IF role_oid IS NULL
     OR EXISTS (SELECT 1 FROM pg_roles WHERE oid=role_oid AND (rolsuper OR rolbypassrls OR rolcreaterole OR rolcreatedb OR rolreplication))
     OR EXISTS (SELECT 1 FROM pg_auth_members WHERE member=role_oid)
     OR has_database_privilege(role_name,current_database(),'CREATE')
     OR EXISTS (SELECT 1 FROM pg_database WHERE datname=current_database() AND datdba=role_oid)
  THEN
    RAISE EXCEPTION 'runtime role is not eligible' USING ERRCODE='42501';
  END IF;
  EXECUTE format('REVOKE ALL PRIVILEGES ON ALL TABLES IN SCHEMA public FROM %I',role_name);
  EXECUTE format('REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA public FROM %I',role_name);
  EXECUTE format('REVOKE TEMPORARY ON DATABASE %I FROM PUBLIC',current_database());
  EXECUTE format('REVOKE TEMPORARY ON DATABASE %I FROM %I',current_database(),role_name);
  EXECUTE format('REVOKE CREATE ON SCHEMA public FROM %I',role_name);
  EXECUTE format('GRANT USAGE ON SCHEMA public TO %I',role_name);
  EXECUTE format('GRANT EXECUTE ON FUNCTION public.graphhelm_migrations_are_current(bytea,bytea,bytea,bytea) TO %I',role_name);
  EXECUTE format('GRANT SELECT,INSERT,UPDATE ON TABLE public.graphhelm_streams,public.graphhelm_evidence,public.graphhelm_retention_operations,public.graphhelm_retention_targets,public.graphhelm_projection_active TO %I',role_name);
  EXECUTE format('GRANT SELECT,INSERT ON TABLE public.graphhelm_legal_holds,public.graphhelm_projection_checkpoints TO %I',role_name);
  EXECUTE format('GRANT SELECT,INSERT ON TABLE public.graphhelm_idempotency,public.graphhelm_events,public.graphhelm_artifacts,public.graphhelm_evidence_refs,public.graphhelm_artifact_refs,public.graphhelm_checkpoints,public.graphhelm_retention_policies,public.graphhelm_evidence_tombstones,public.graphhelm_cleanup_receipts TO %I',role_name);
END $graphhelm$;
