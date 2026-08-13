CREATE TABLE public.graphhelm_streams (
    workspace_id text NOT NULL,
    project_id text NOT NULL,
    execution_id text NOT NULL DEFAULT '',
    stream_id text NOT NULL,
    next_sequence bigint NOT NULL CHECK (next_sequence BETWEEN 1 AND 9007199254740991),
    last_event_hash text NOT NULL,
    head_key_id text NOT NULL,
    head_key_version text NOT NULL CHECK (length(head_key_version) BETWEEN 1 AND 256),
    head_provider_epoch bigint NOT NULL CHECK (head_provider_epoch BETWEEN 0 AND 9007199254740991),
    head_algorithm text NOT NULL CHECK (head_algorithm = 'hmac-sha256'),
    head_tag bytea NOT NULL CHECK (octet_length(head_tag) = 32),
    head_canonical_sha256 text NOT NULL CHECK (head_canonical_sha256 ~ '^sha256:[0-9a-f]{64}$'),
    PRIMARY KEY (workspace_id, project_id, execution_id, stream_id)
);

CREATE TABLE public.graphhelm_idempotency (
    workspace_id text NOT NULL,
    project_id text NOT NULL,
    execution_id text NOT NULL DEFAULT '',
    stream_id text NOT NULL,
    idempotency_key text NOT NULL,
    request_digest text NOT NULL,
    first_sequence bigint NOT NULL,
    event_count integer NOT NULL CHECK (event_count BETWEEN 1 AND 10000),
    PRIMARY KEY (workspace_id, project_id, execution_id, stream_id, idempotency_key),
    FOREIGN KEY (workspace_id, project_id, execution_id, stream_id)
        REFERENCES public.graphhelm_streams (workspace_id, project_id, execution_id, stream_id)
);

CREATE TABLE public.graphhelm_events (
    workspace_id text NOT NULL,
    project_id text NOT NULL,
    execution_id text NOT NULL DEFAULT '',
    stream_id text NOT NULL,
    sequence bigint NOT NULL,
    event_id text NOT NULL,
    idempotency_key text NOT NULL,
    previous_hash text NOT NULL,
    event_hash text NOT NULL,
    request_digest text NOT NULL,
    envelope jsonb NOT NULL,
    PRIMARY KEY (workspace_id, project_id, execution_id, stream_id, sequence),
    UNIQUE (workspace_id, project_id, execution_id, stream_id, event_id),
    UNIQUE (workspace_id, project_id, execution_id, stream_id, idempotency_key),
    FOREIGN KEY (workspace_id, project_id, execution_id, stream_id)
        REFERENCES public.graphhelm_streams (workspace_id, project_id, execution_id, stream_id)
);

CREATE TABLE public.graphhelm_evidence (
    workspace_id text NOT NULL,
    project_id text NOT NULL,
    execution_id text NOT NULL DEFAULT '',
    evidence_id text NOT NULL,
    record jsonb NOT NULL,
    state text NOT NULL DEFAULT 'available' CHECK (state IN ('available', 'erasure_pending', 'erased', 'expired', 'missing_key', 'integrity_failed')),
    PRIMARY KEY (workspace_id, project_id, execution_id, evidence_id)
);

CREATE TABLE public.graphhelm_artifacts (
    workspace_id text NOT NULL,
    project_id text NOT NULL,
    execution_id text NOT NULL DEFAULT '',
    artifact_id text NOT NULL,
    producer_stream_id text NOT NULL,
    producer_idempotency_key text NOT NULL,
    reference jsonb NOT NULL,
    PRIMARY KEY (workspace_id, project_id, execution_id, artifact_id),
    FOREIGN KEY (workspace_id, project_id, execution_id, producer_stream_id, producer_idempotency_key)
        REFERENCES public.graphhelm_idempotency (workspace_id, project_id, execution_id, stream_id, idempotency_key)
        DEFERRABLE INITIALLY DEFERRED
);

CREATE TABLE public.graphhelm_evidence_refs (
    workspace_id text NOT NULL,
    project_id text NOT NULL,
    execution_id text NOT NULL DEFAULT '',
    stream_id text NOT NULL,
    sequence bigint NOT NULL,
    ordinal integer NOT NULL CHECK (ordinal >= 0),
    evidence_id text NOT NULL,
    PRIMARY KEY (workspace_id, project_id, execution_id, stream_id, sequence, ordinal),
    FOREIGN KEY (workspace_id, project_id, execution_id, stream_id, sequence)
        REFERENCES public.graphhelm_events (workspace_id, project_id, execution_id, stream_id, sequence),
    FOREIGN KEY (workspace_id, project_id, execution_id, evidence_id)
        REFERENCES public.graphhelm_evidence (workspace_id, project_id, execution_id, evidence_id)
);

CREATE TABLE public.graphhelm_artifact_refs (
    workspace_id text NOT NULL,
    project_id text NOT NULL,
    execution_id text NOT NULL DEFAULT '',
    stream_id text NOT NULL,
    sequence bigint NOT NULL,
    ordinal integer NOT NULL CHECK (ordinal >= 0),
    artifact_id text NOT NULL,
    PRIMARY KEY (workspace_id, project_id, execution_id, stream_id, sequence, ordinal),
    FOREIGN KEY (workspace_id, project_id, execution_id, stream_id, sequence)
        REFERENCES public.graphhelm_events (workspace_id, project_id, execution_id, stream_id, sequence),
    FOREIGN KEY (workspace_id, project_id, execution_id, artifact_id)
        REFERENCES public.graphhelm_artifacts (workspace_id, project_id, execution_id, artifact_id)
);

CREATE TABLE public.graphhelm_checkpoints (
    workspace_id text NOT NULL,
    project_id text NOT NULL,
    execution_id text NOT NULL DEFAULT '',
    stream_id text NOT NULL,
    sequence bigint NOT NULL,
    event_hash text NOT NULL,
    repository_format_version integer NOT NULL,
    created_at timestamptz NOT NULL,
    key_id text NOT NULL,
    key_version text NOT NULL,
    provider_epoch bigint NOT NULL CHECK (provider_epoch BETWEEN 0 AND 9007199254740991),
    active_graph_number bigint CHECK (active_graph_number BETWEEN 1 AND 9007199254740991),
    active_graph_semantic_hash text CHECK (
      active_graph_semantic_hash IS NULL OR
      active_graph_semantic_hash ~ '^sha256:[0-9a-f]{64}$'
    ),
    algorithm text NOT NULL,
    tag bytea NOT NULL,
    canonical_sha256 text NOT NULL,
    CHECK ((active_graph_number IS NULL) = (active_graph_semantic_hash IS NULL)),
    PRIMARY KEY (workspace_id, project_id, execution_id, stream_id, sequence),
    FOREIGN KEY (workspace_id, project_id, execution_id, stream_id)
        REFERENCES public.graphhelm_streams (workspace_id, project_id, execution_id, stream_id)
);

DO $graphhelm$
DECLARE table_name text;
BEGIN
  FOREACH table_name IN ARRAY ARRAY[
    'graphhelm_streams', 'graphhelm_idempotency', 'graphhelm_events',
    'graphhelm_evidence', 'graphhelm_artifacts', 'graphhelm_evidence_refs',
    'graphhelm_artifact_refs', 'graphhelm_checkpoints'
  ] LOOP
    EXECUTE format('ALTER TABLE public.%I ENABLE ROW LEVEL SECURITY', table_name);
    EXECUTE format('ALTER TABLE public.%I FORCE ROW LEVEL SECURITY', table_name);
    EXECUTE format(
      'CREATE POLICY graphhelm_scope ON public.%I USING (' ||
      'workspace_id = current_setting(''graphhelm.workspace_id'', true) AND ' ||
      'project_id = current_setting(''graphhelm.project_id'', true) AND ' ||
      'execution_id = COALESCE(current_setting(''graphhelm.execution_id'', true), '''')) ' ||
      'WITH CHECK (' ||
      'workspace_id = current_setting(''graphhelm.workspace_id'', true) AND ' ||
      'project_id = current_setting(''graphhelm.project_id'', true) AND ' ||
      'execution_id = COALESCE(current_setting(''graphhelm.execution_id'', true), ''''))',
      table_name
    );
  END LOOP;
END $graphhelm$;

CREATE FUNCTION public.graphhelm_reject_immutable_row_change()
RETURNS trigger
LANGUAGE plpgsql
AS $graphhelm$
BEGIN
  RAISE EXCEPTION 'GraphHelm history rows are immutable' USING ERRCODE = '55000';
END
$graphhelm$;

CREATE FUNCTION public.graphhelm_validate_stream_head_change()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, public
AS $graphhelm$
DECLARE
  tail_sequence bigint;
  tail_hash text;
BEGIN
  SELECT sequence, event_hash INTO tail_sequence, tail_hash
  FROM public.graphhelm_events
  WHERE workspace_id=NEW.workspace_id AND project_id=NEW.project_id
    AND execution_id=NEW.execution_id AND stream_id=NEW.stream_id
  ORDER BY sequence DESC LIMIT 1;
  IF OLD.workspace_id <> NEW.workspace_id
     OR OLD.project_id <> NEW.project_id
     OR OLD.execution_id <> NEW.execution_id
     OR OLD.stream_id <> NEW.stream_id
     OR NEW.next_sequence <= OLD.next_sequence
     OR NEW.last_event_hash = OLD.last_event_hash
     OR tail_sequence IS NULL
     OR tail_sequence + 1 <> NEW.next_sequence
     OR tail_hash <> NEW.last_event_hash THEN
    RAISE EXCEPTION 'invalid GraphHelm stream-head transition' USING ERRCODE = '55000';
  END IF;
  RETURN NEW;
END
$graphhelm$;

CREATE FUNCTION public.graphhelm_validate_event_append()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, public
AS $graphhelm$
DECLARE
  tail_sequence bigint;
  tail_hash text;
BEGIN
  SELECT sequence, event_hash INTO tail_sequence, tail_hash
  FROM public.graphhelm_events
  WHERE workspace_id=NEW.workspace_id AND project_id=NEW.project_id
    AND execution_id=NEW.execution_id AND stream_id=NEW.stream_id
  ORDER BY sequence DESC LIMIT 1;
  IF NEW.sequence <> COALESCE(tail_sequence + 1, 1)
     OR NEW.previous_hash <> COALESCE(
       tail_hash,
       'sha256:35c8ab0717bef1684ad07efcf3bedd4648c778a2c944cbd2c7e6a4802e2237b3'
     ) THEN
    RAISE EXCEPTION 'invalid GraphHelm event append' USING ERRCODE = '55000';
  END IF;
  RETURN NEW;
END
$graphhelm$;

CREATE TRIGGER graphhelm_events_ordered_append
BEFORE INSERT ON public.graphhelm_events
FOR EACH ROW EXECUTE FUNCTION public.graphhelm_validate_event_append();

CREATE TRIGGER graphhelm_streams_append_only
BEFORE UPDATE ON public.graphhelm_streams
FOR EACH ROW EXECUTE FUNCTION public.graphhelm_validate_stream_head_change();

CREATE TRIGGER graphhelm_streams_no_delete
BEFORE DELETE ON public.graphhelm_streams
FOR EACH ROW EXECUTE FUNCTION public.graphhelm_reject_immutable_row_change();

CREATE FUNCTION public.graphhelm_validate_stream_insert()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, public
AS $graphhelm$
BEGIN
  IF NEW.next_sequence <> 1 OR NEW.last_event_hash <>
    'sha256:35c8ab0717bef1684ad07efcf3bedd4648c778a2c944cbd2c7e6a4802e2237b3' THEN
    RAISE EXCEPTION 'invalid GraphHelm initial stream head' USING ERRCODE = '55000';
  END IF;
  RETURN NEW;
END
$graphhelm$;

CREATE TRIGGER graphhelm_streams_valid_insert
BEFORE INSERT ON public.graphhelm_streams
FOR EACH ROW EXECUTE FUNCTION public.graphhelm_validate_stream_insert();

DO $graphhelm$
DECLARE table_name text;
BEGIN
  FOREACH table_name IN ARRAY ARRAY[
    'graphhelm_idempotency', 'graphhelm_events', 'graphhelm_evidence',
    'graphhelm_artifacts', 'graphhelm_evidence_refs', 'graphhelm_artifact_refs',
    'graphhelm_checkpoints'
  ] LOOP
    EXECUTE format(
      'CREATE TRIGGER %I BEFORE UPDATE OR DELETE ON public.%I FOR EACH ROW EXECUTE FUNCTION public.graphhelm_reject_immutable_row_change()',
      table_name || '_immutable', table_name
    );
  END LOOP;
END
$graphhelm$;

CREATE FUNCTION public.graphhelm_migration_is_current(expected_checksum bytea)
RETURNS boolean
LANGUAGE sql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $graphhelm$
  SELECT count(*) = 1 AND bool_and(
    version = 1 AND success AND checksum = expected_checksum
  )
  FROM public._sqlx_migrations
$graphhelm$;

REVOKE ALL ON FUNCTION public.graphhelm_migration_is_current(bytea) FROM PUBLIC;

CREATE FUNCTION public.graphhelm_configure_runtime_role(role_name name)
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $graphhelm$
DECLARE role_oid oid;
BEGIN
  SELECT oid INTO role_oid FROM pg_roles WHERE rolname = role_name;
  IF role_oid IS NULL THEN
    RAISE EXCEPTION 'runtime role does not exist' USING ERRCODE = '42704';
  END IF;
  IF EXISTS (
    SELECT 1 FROM pg_roles
    WHERE oid = role_oid
      AND (rolsuper OR rolbypassrls OR rolcreaterole OR rolcreatedb OR rolreplication)
  ) THEN
    RAISE EXCEPTION 'runtime role is privileged' USING ERRCODE = '42501';
  END IF;
  IF EXISTS (SELECT 1 FROM pg_auth_members WHERE member = role_oid) THEN
    RAISE EXCEPTION 'runtime role has role memberships' USING ERRCODE = '42501';
  END IF;
  IF has_database_privilege(role_name, current_database(), 'CREATE') OR EXISTS (
    SELECT 1 FROM pg_database
    WHERE datname=current_database() AND datdba=role_oid
  ) THEN
    RAISE EXCEPTION 'runtime role controls the database' USING ERRCODE = '42501';
  END IF;

  EXECUTE format('REVOKE ALL PRIVILEGES ON ALL TABLES IN SCHEMA public FROM %I', role_name);
  EXECUTE format('REVOKE TEMPORARY ON DATABASE %I FROM PUBLIC', current_database());
  EXECUTE format('REVOKE TEMPORARY ON DATABASE %I FROM %I', current_database(), role_name);
  EXECUTE format('REVOKE CREATE ON SCHEMA public FROM %I', role_name);
  EXECUTE format('GRANT USAGE ON SCHEMA public TO %I', role_name);
  EXECUTE format('GRANT EXECUTE ON FUNCTION public.graphhelm_migration_is_current(bytea) TO %I', role_name);
  EXECUTE format('GRANT SELECT, INSERT, UPDATE ON TABLE public.graphhelm_streams TO %I', role_name);
  EXECUTE format(
    'GRANT SELECT, INSERT ON TABLE public.graphhelm_idempotency, public.graphhelm_events, public.graphhelm_evidence, public.graphhelm_artifacts, public.graphhelm_evidence_refs, public.graphhelm_artifact_refs, public.graphhelm_checkpoints TO %I',
    role_name
  );
END
$graphhelm$;

REVOKE ALL ON FUNCTION public.graphhelm_configure_runtime_role(name) FROM PUBLIC;
