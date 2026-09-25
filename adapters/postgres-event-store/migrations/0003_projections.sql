CREATE TABLE public.graphhelm_projection_checkpoints (
  workspace_id text NOT NULL,
  project_id text NOT NULL,
  execution_id text NOT NULL DEFAULT '',
  stream_id text NOT NULL CHECK (length(stream_id) BETWEEN 1 AND 256),
  projection_name text NOT NULL CHECK (length(projection_name) BETWEEN 1 AND 256),
  projection_version integer NOT NULL CHECK (projection_version BETWEEN 1 AND 2147483647),
  generation bigint NOT NULL CHECK (generation BETWEEN 1 AND 9007199254740991),
  last_sequence bigint NOT NULL CHECK (last_sequence BETWEEN 0 AND 9007199254740991),
  last_event_hash text,
  format_version integer NOT NULL CHECK (format_version = 1),
  state jsonb NOT NULL,
  PRIMARY KEY (
    workspace_id, project_id, execution_id, stream_id, projection_name,
    projection_version, generation, last_sequence
  ),
  CHECK (
    (last_sequence = 0 AND last_event_hash IS NULL) OR
    (last_sequence > 0 AND last_event_hash ~ '^sha256:[0-9a-f]{64}$')
  )
);

CREATE TABLE public.graphhelm_projection_active (
  workspace_id text NOT NULL,
  project_id text NOT NULL,
  execution_id text NOT NULL DEFAULT '',
  stream_id text NOT NULL,
  projection_name text NOT NULL,
  projection_version integer NOT NULL,
  generation bigint NOT NULL,
  last_sequence bigint NOT NULL,
  PRIMARY KEY (
    workspace_id, project_id, execution_id, stream_id, projection_name,
    projection_version
  ),
  FOREIGN KEY (
    workspace_id, project_id, execution_id, stream_id, projection_name,
    projection_version, generation, last_sequence
  ) REFERENCES public.graphhelm_projection_checkpoints (
    workspace_id, project_id, execution_id, stream_id, projection_name,
    projection_version, generation, last_sequence
  )
);

DO $graphhelm$
DECLARE table_name text;
BEGIN
  FOREACH table_name IN ARRAY ARRAY[
    'graphhelm_projection_checkpoints', 'graphhelm_projection_active'
  ] LOOP
    EXECUTE format('ALTER TABLE public.%I ENABLE ROW LEVEL SECURITY', table_name);
    EXECUTE format('ALTER TABLE public.%I FORCE ROW LEVEL SECURITY', table_name);
    EXECUTE format(
      'CREATE POLICY graphhelm_scope ON public.%I USING (' ||
      'workspace_id=current_setting(''graphhelm.workspace_id'',true) AND ' ||
      'project_id=current_setting(''graphhelm.project_id'',true) AND ' ||
      'execution_id=COALESCE(current_setting(''graphhelm.execution_id'',true),'''')) ' ||
      'WITH CHECK (' ||
      'workspace_id=current_setting(''graphhelm.workspace_id'',true) AND ' ||
      'project_id=current_setting(''graphhelm.project_id'',true) AND ' ||
      'execution_id=COALESCE(current_setting(''graphhelm.execution_id'',true),''''))',
      table_name
    );
  END LOOP;
END $graphhelm$;

CREATE TRIGGER graphhelm_projection_checkpoints_immutable
BEFORE UPDATE OR DELETE ON public.graphhelm_projection_checkpoints
FOR EACH ROW EXECUTE FUNCTION public.graphhelm_reject_immutable_row_change();

CREATE FUNCTION public.graphhelm_validate_projection_active_change()
RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog,public AS $graphhelm$
DECLARE checkpoint_hash text;
DECLARE stream_next bigint;
DECLARE stream_hash text;
DECLARE stream_exists boolean;
BEGIN
  IF TG_OP='DELETE' THEN
    RAISE EXCEPTION 'invalid GraphHelm projection activation' USING ERRCODE='55000';
  END IF;
  IF TG_OP='UPDATE' AND (
       OLD.workspace_id<>NEW.workspace_id
       OR OLD.project_id<>NEW.project_id
       OR OLD.execution_id<>NEW.execution_id
       OR OLD.stream_id<>NEW.stream_id
       OR OLD.projection_name<>NEW.projection_name
       OR OLD.projection_version<>NEW.projection_version
       OR NEW.generation<=OLD.generation
     ) THEN
    RAISE EXCEPTION 'invalid GraphHelm projection activation' USING ERRCODE='55000';
  END IF;
  LOCK TABLE public.graphhelm_streams IN SHARE MODE;
  SELECT last_event_hash INTO checkpoint_hash
  FROM public.graphhelm_projection_checkpoints
  WHERE workspace_id=NEW.workspace_id AND project_id=NEW.project_id
    AND execution_id=NEW.execution_id AND stream_id=NEW.stream_id
    AND projection_name=NEW.projection_name
    AND projection_version=NEW.projection_version
    AND generation=NEW.generation AND last_sequence=NEW.last_sequence;
  IF NOT FOUND THEN
    RAISE EXCEPTION 'invalid GraphHelm projection activation' USING ERRCODE='55000';
  END IF;
  SELECT next_sequence,last_event_hash INTO stream_next,stream_hash
  FROM public.graphhelm_streams
  WHERE workspace_id=NEW.workspace_id AND project_id=NEW.project_id
    AND execution_id=NEW.execution_id AND stream_id=NEW.stream_id;
  stream_exists := FOUND;
  IF (stream_exists AND (
        stream_next<>NEW.last_sequence+1 OR stream_hash<>checkpoint_hash
      )) OR (NOT stream_exists AND (
        NEW.last_sequence<>0 OR checkpoint_hash IS NOT NULL
      )) THEN
    RAISE EXCEPTION 'invalid GraphHelm projection source head' USING ERRCODE='55000';
  END IF;
  RETURN NEW;
END $graphhelm$;

CREATE TRIGGER graphhelm_projection_active_guarded
BEFORE INSERT OR UPDATE OR DELETE ON public.graphhelm_projection_active
FOR EACH ROW EXECUTE FUNCTION public.graphhelm_validate_projection_active_change();

CREATE FUNCTION public.graphhelm_migrations_are_current(
  expected_one bytea, expected_two bytea, expected_three bytea
)
RETURNS boolean LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog,public AS $graphhelm$
  SELECT count(*)=3 AND bool_and(success) AND bool_and(
    CASE version
      WHEN 1 THEN checksum=expected_one
      WHEN 2 THEN checksum=expected_two
      WHEN 3 THEN checksum=expected_three
      ELSE false
    END
  )
  FROM public._sqlx_migrations
$graphhelm$;
REVOKE ALL ON FUNCTION public.graphhelm_migrations_are_current(bytea,bytea,bytea) FROM PUBLIC;

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
  EXECUTE format('REVOKE TEMPORARY ON DATABASE %I FROM PUBLIC',current_database());
  EXECUTE format('REVOKE TEMPORARY ON DATABASE %I FROM %I',current_database(),role_name);
  EXECUTE format('REVOKE CREATE ON SCHEMA public FROM %I',role_name);
  EXECUTE format('GRANT USAGE ON SCHEMA public TO %I',role_name);
  EXECUTE format('GRANT EXECUTE ON FUNCTION public.graphhelm_migrations_are_current(bytea,bytea,bytea) TO %I',role_name);
  EXECUTE format('GRANT SELECT,INSERT,UPDATE ON TABLE public.graphhelm_streams,public.graphhelm_evidence,public.graphhelm_retention_operations,public.graphhelm_retention_targets,public.graphhelm_projection_active TO %I',role_name);
  EXECUTE format('GRANT SELECT,INSERT ON TABLE public.graphhelm_legal_holds,public.graphhelm_projection_checkpoints TO %I',role_name);
  EXECUTE format('GRANT SELECT,INSERT ON TABLE public.graphhelm_idempotency,public.graphhelm_events,public.graphhelm_artifacts,public.graphhelm_evidence_refs,public.graphhelm_artifact_refs,public.graphhelm_checkpoints,public.graphhelm_retention_policies,public.graphhelm_evidence_tombstones,public.graphhelm_cleanup_receipts TO %I',role_name);
END $graphhelm$;
