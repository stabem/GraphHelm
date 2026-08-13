ALTER TABLE public.graphhelm_evidence
  ADD COLUMN created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
  ADD COLUMN hold_revision bigint NOT NULL DEFAULT 0 CHECK (hold_revision >= 0),
  ADD COLUMN cleanup_eligible_at timestamptz,
  ADD COLUMN ciphertext_deleted_at timestamptz;

DROP TRIGGER graphhelm_evidence_immutable ON public.graphhelm_evidence;

CREATE FUNCTION public.graphhelm_validate_evidence_retention_change()
RETURNS trigger LANGUAGE plpgsql SET search_path = pg_catalog, public AS $graphhelm$
BEGIN
  IF OLD.workspace_id <> NEW.workspace_id OR OLD.project_id <> NEW.project_id
     OR OLD.execution_id <> NEW.execution_id OR OLD.evidence_id <> NEW.evidence_id
     OR OLD.created_at <> NEW.created_at THEN
    RAISE EXCEPTION 'invalid GraphHelm Evidence transition' USING ERRCODE='55000';
  END IF;
  IF NEW.state='erasure_pending' AND OLD.state IN ('available','expired','missing_key','integrity_failed')
     AND NEW.hold_revision=OLD.hold_revision
     AND NEW.record=OLD.record AND NEW.cleanup_eligible_at IS NULL
     AND NEW.ciphertext_deleted_at IS NULL THEN RETURN NEW; END IF;
  IF OLD.state='erasure_pending' AND NEW.state='erased' AND NEW.record=OLD.record
     AND NEW.hold_revision=OLD.hold_revision
     AND NEW.cleanup_eligible_at IS NOT NULL AND NEW.ciphertext_deleted_at IS NULL THEN RETURN NEW; END IF;
  IF OLD.state='erased' AND NEW.state='erased' AND OLD.cleanup_eligible_at=NEW.cleanup_eligible_at
     AND NEW.hold_revision=OLD.hold_revision
     AND OLD.ciphertext_deleted_at IS NULL AND NEW.ciphertext_deleted_at IS NOT NULL
     AND NEW.record='{}'::jsonb THEN RETURN NEW; END IF;
  IF NEW.hold_revision=OLD.hold_revision+1
     AND (to_jsonb(OLD)-'hold_revision')=(to_jsonb(NEW)-'hold_revision') THEN RETURN NEW; END IF;
  RAISE EXCEPTION 'invalid GraphHelm Evidence transition' USING ERRCODE='55000';
END $graphhelm$;

CREATE TRIGGER graphhelm_evidence_retention_only
BEFORE UPDATE OR DELETE ON public.graphhelm_evidence
FOR EACH ROW EXECUTE FUNCTION public.graphhelm_validate_evidence_retention_change();

CREATE TABLE public.graphhelm_retention_policies (
  workspace_id text NOT NULL, project_id text NOT NULL, execution_id text NOT NULL DEFAULT '',
  policy_id text NOT NULL, policy_version text NOT NULL, retention_class text NOT NULL,
  minimum_age_seconds bigint NOT NULL CHECK (minimum_age_seconds BETWEEN 0 AND 9007199254740991),
  cleanup_delay_seconds bigint NOT NULL CHECK (cleanup_delay_seconds BETWEEN 0 AND 9007199254740991),
  PRIMARY KEY (workspace_id,project_id,execution_id,policy_id,policy_version)
);

CREATE TABLE public.graphhelm_retention_operations (
  workspace_id text NOT NULL, project_id text NOT NULL, execution_id text NOT NULL DEFAULT '',
  operation_id text NOT NULL, idempotency_key text NOT NULL, request_digest text NOT NULL,
  policy_id text NOT NULL, policy_version text NOT NULL, authority text NOT NULL,
  authority_key_id text NOT NULL, authority_algorithm text NOT NULL, authority_tag bytea NOT NULL,
  reason_code text NOT NULL, evaluated_at text NOT NULL CHECK (evaluated_at ~ '^[0-9]{4}-.*Z$'),
  requested_at text NOT NULL CHECK (requested_at ~ '^[0-9]{4}-.*Z$'), provider_epoch bigint NOT NULL,
  prepared_key_id text NOT NULL, prepared_algorithm text NOT NULL, prepared_tag bytea NOT NULL,
  finalized_key_id text, finalized_algorithm text, finalized_tag bytea,
  state text NOT NULL CHECK (state IN ('prepared','finalized')),
  completed_at text CHECK (completed_at IS NULL OR completed_at ~ '^[0-9]{4}-.*Z$'),
  PRIMARY KEY (workspace_id,project_id,execution_id,operation_id),
  UNIQUE (workspace_id,project_id,execution_id,idempotency_key),
  FOREIGN KEY (workspace_id,project_id,execution_id,policy_id,policy_version)
    REFERENCES public.graphhelm_retention_policies(workspace_id,project_id,execution_id,policy_id,policy_version),
  CHECK ((state='prepared' AND completed_at IS NULL AND finalized_tag IS NULL AND finalized_key_id IS NULL AND finalized_algorithm IS NULL)
      OR (state='finalized' AND completed_at IS NOT NULL AND finalized_tag IS NOT NULL AND finalized_key_id IS NOT NULL AND finalized_algorithm IS NOT NULL))
);

CREATE TABLE public.graphhelm_retention_targets (
  workspace_id text NOT NULL, project_id text NOT NULL, execution_id text NOT NULL DEFAULT '',
  operation_id text NOT NULL, ordinal integer NOT NULL CHECK (ordinal BETWEEN 0 AND 9999),
  evidence_id text NOT NULL, key_handle_id text NOT NULL, ciphertext_sha256 text NOT NULL,
  classification text NOT NULL CHECK (classification IN ('public','internal','confidential','restricted')),
  prior_state text NOT NULL CHECK (prior_state IN ('available','expired','missing_key','integrity_failed')),
  provider_receipt_epoch bigint, provider_receipt_key_id text,
  provider_receipt_algorithm text, provider_receipt_tag bytea,
  PRIMARY KEY (workspace_id,project_id,execution_id,operation_id,ordinal),
  UNIQUE (workspace_id,project_id,execution_id,operation_id,evidence_id),
  FOREIGN KEY (workspace_id,project_id,execution_id,operation_id)
    REFERENCES public.graphhelm_retention_operations(workspace_id,project_id,execution_id,operation_id),
  FOREIGN KEY (workspace_id,project_id,execution_id,evidence_id)
    REFERENCES public.graphhelm_evidence(workspace_id,project_id,execution_id,evidence_id),
  CHECK ((provider_receipt_epoch IS NULL) = (provider_receipt_tag IS NULL)),
  CHECK ((provider_receipt_epoch IS NULL) = (provider_receipt_key_id IS NULL)),
  CHECK ((provider_receipt_epoch IS NULL) = (provider_receipt_algorithm IS NULL))
);

CREATE TABLE public.graphhelm_legal_holds (
  workspace_id text NOT NULL, project_id text NOT NULL, execution_id text NOT NULL DEFAULT '',
  hold_id text NOT NULL, evidence_id text NOT NULL, authority text NOT NULL,
  reason_code text NOT NULL, placed boolean NOT NULL, changed_at text NOT NULL CHECK (changed_at ~ '^[0-9]{4}-.*Z$'),
  authentication_key_id text NOT NULL, authentication_algorithm text NOT NULL, authentication_tag bytea NOT NULL,
  PRIMARY KEY (workspace_id,project_id,execution_id,hold_id,placed),
  FOREIGN KEY (workspace_id,project_id,execution_id,evidence_id)
    REFERENCES public.graphhelm_evidence(workspace_id,project_id,execution_id,evidence_id)
);

CREATE TABLE public.graphhelm_evidence_tombstones (
  workspace_id text NOT NULL, project_id text NOT NULL, execution_id text NOT NULL DEFAULT '',
  evidence_id text NOT NULL, operation_id text NOT NULL, ciphertext_sha256 text NOT NULL,
  classification text NOT NULL, retention_class text NOT NULL, prior_state text NOT NULL,
  policy_id text NOT NULL, policy_version text NOT NULL, authority text NOT NULL,
  reason_code text NOT NULL, requested_at text NOT NULL, completed_at text NOT NULL,
  provider_epoch bigint NOT NULL,
  PRIMARY KEY (workspace_id,project_id,execution_id,evidence_id),
  FOREIGN KEY (workspace_id,project_id,execution_id,operation_id)
    REFERENCES public.graphhelm_retention_operations(workspace_id,project_id,execution_id,operation_id)
);

CREATE TABLE public.graphhelm_cleanup_receipts (
  workspace_id text NOT NULL, project_id text NOT NULL, execution_id text NOT NULL DEFAULT '',
  operation_id text NOT NULL, idempotency_key text NOT NULL, request_digest text NOT NULL, evidence_id text NOT NULL,
  ciphertext_sha256 text NOT NULL, requested_at text NOT NULL, deleted_at text NOT NULL,
  PRIMARY KEY (workspace_id,project_id,execution_id,operation_id,evidence_id),
  UNIQUE (workspace_id,project_id,execution_id,idempotency_key,evidence_id),
  FOREIGN KEY (workspace_id,project_id,execution_id,evidence_id)
    REFERENCES public.graphhelm_evidence(workspace_id,project_id,execution_id,evidence_id)
);

CREATE FUNCTION public.graphhelm_validate_retention_operation_change()
RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog,public AS $graphhelm$
BEGIN
  IF TG_OP='DELETE' OR OLD.state<>'prepared' OR NEW.state<>'finalized'
     OR (to_jsonb(OLD)-ARRAY['state','completed_at','finalized_key_id','finalized_algorithm','finalized_tag'])
        <> (to_jsonb(NEW)-ARRAY['state','completed_at','finalized_key_id','finalized_algorithm','finalized_tag'])
     OR NEW.completed_at IS NULL OR NEW.finalized_key_id IS NULL OR NEW.finalized_algorithm IS NULL OR NEW.finalized_tag IS NULL
  THEN RAISE EXCEPTION 'invalid retention operation transition' USING ERRCODE='55000'; END IF;
  RETURN NEW;
END $graphhelm$;
CREATE TRIGGER graphhelm_retention_operations_guarded
BEFORE UPDATE OR DELETE ON public.graphhelm_retention_operations
FOR EACH ROW EXECUTE FUNCTION public.graphhelm_validate_retention_operation_change();

CREATE FUNCTION public.graphhelm_validate_retention_target_change()
RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog,public AS $graphhelm$
BEGIN
  IF TG_OP='DELETE'
     OR (to_jsonb(OLD)-ARRAY['provider_receipt_epoch','provider_receipt_key_id','provider_receipt_algorithm','provider_receipt_tag'])
        <> (to_jsonb(NEW)-ARRAY['provider_receipt_epoch','provider_receipt_key_id','provider_receipt_algorithm','provider_receipt_tag'])
     OR OLD.provider_receipt_epoch IS NOT NULL OR OLD.provider_receipt_key_id IS NOT NULL
     OR OLD.provider_receipt_algorithm IS NOT NULL OR OLD.provider_receipt_tag IS NOT NULL
     OR NEW.provider_receipt_epoch IS NULL OR NEW.provider_receipt_key_id IS NULL
     OR NEW.provider_receipt_algorithm IS NULL OR NEW.provider_receipt_tag IS NULL
  THEN RAISE EXCEPTION 'invalid retention target transition' USING ERRCODE='55000'; END IF;
  RETURN NEW;
END $graphhelm$;
CREATE TRIGGER graphhelm_retention_targets_guarded
BEFORE UPDATE OR DELETE ON public.graphhelm_retention_targets
FOR EACH ROW EXECUTE FUNCTION public.graphhelm_validate_retention_target_change();

DO $graphhelm$
DECLARE table_name text;
BEGIN
  FOREACH table_name IN ARRAY ARRAY[
    'graphhelm_retention_policies','graphhelm_retention_operations','graphhelm_retention_targets',
    'graphhelm_legal_holds','graphhelm_evidence_tombstones','graphhelm_cleanup_receipts'
  ] LOOP
    EXECUTE format('ALTER TABLE public.%I ENABLE ROW LEVEL SECURITY',table_name);
    EXECUTE format('ALTER TABLE public.%I FORCE ROW LEVEL SECURITY',table_name);
    EXECUTE format('CREATE POLICY graphhelm_scope ON public.%I USING (workspace_id=current_setting(''graphhelm.workspace_id'',true) AND project_id=current_setting(''graphhelm.project_id'',true) AND execution_id=COALESCE(current_setting(''graphhelm.execution_id'',true),'''')) WITH CHECK (workspace_id=current_setting(''graphhelm.workspace_id'',true) AND project_id=current_setting(''graphhelm.project_id'',true) AND execution_id=COALESCE(current_setting(''graphhelm.execution_id'',true),''''))',table_name);
  END LOOP;
END $graphhelm$;

DO $graphhelm$
DECLARE table_name text;
BEGIN
  FOREACH table_name IN ARRAY ARRAY['graphhelm_retention_policies','graphhelm_legal_holds','graphhelm_evidence_tombstones','graphhelm_cleanup_receipts'] LOOP
    EXECUTE format('CREATE TRIGGER %I BEFORE UPDATE OR DELETE ON public.%I FOR EACH ROW EXECUTE FUNCTION public.graphhelm_reject_immutable_row_change()',table_name || '_immutable',table_name);
  END LOOP;
END $graphhelm$;

CREATE FUNCTION public.graphhelm_migrations_are_current(expected_one bytea, expected_two bytea)
RETURNS boolean LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog,public AS $graphhelm$
  SELECT count(*)=2 AND bool_and(success) AND
    bool_and(CASE version WHEN 1 THEN checksum=expected_one WHEN 2 THEN checksum=expected_two ELSE false END)
  FROM public._sqlx_migrations
$graphhelm$;
REVOKE ALL ON FUNCTION public.graphhelm_migrations_are_current(bytea,bytea) FROM PUBLIC;

CREATE OR REPLACE FUNCTION public.graphhelm_configure_runtime_role(role_name name)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $graphhelm$
DECLARE role_oid oid;
BEGIN
  SELECT oid INTO role_oid FROM pg_roles WHERE rolname=role_name;
  IF role_oid IS NULL OR EXISTS (SELECT 1 FROM pg_roles WHERE oid=role_oid AND (rolsuper OR rolbypassrls OR rolcreaterole OR rolcreatedb OR rolreplication))
     OR EXISTS (SELECT 1 FROM pg_auth_members WHERE member=role_oid)
     OR has_database_privilege(role_name,current_database(),'CREATE')
     OR EXISTS (SELECT 1 FROM pg_database WHERE datname=current_database() AND datdba=role_oid) THEN
    RAISE EXCEPTION 'runtime role is not eligible' USING ERRCODE='42501';
  END IF;
  EXECUTE format('REVOKE ALL PRIVILEGES ON ALL TABLES IN SCHEMA public FROM %I',role_name);
  EXECUTE format('REVOKE TEMPORARY ON DATABASE %I FROM PUBLIC',current_database());
  EXECUTE format('REVOKE TEMPORARY ON DATABASE %I FROM %I',current_database(),role_name);
  EXECUTE format('REVOKE CREATE ON SCHEMA public FROM %I',role_name);
  EXECUTE format('GRANT USAGE ON SCHEMA public TO %I',role_name);
  EXECUTE format('GRANT EXECUTE ON FUNCTION public.graphhelm_migrations_are_current(bytea,bytea) TO %I',role_name);
  EXECUTE format('GRANT SELECT,INSERT,UPDATE ON TABLE public.graphhelm_streams,public.graphhelm_evidence,public.graphhelm_retention_operations,public.graphhelm_retention_targets TO %I',role_name);
  EXECUTE format('GRANT SELECT,INSERT ON TABLE public.graphhelm_legal_holds TO %I',role_name);
  EXECUTE format('GRANT SELECT,INSERT ON TABLE public.graphhelm_idempotency,public.graphhelm_events,public.graphhelm_artifacts,public.graphhelm_evidence_refs,public.graphhelm_artifact_refs,public.graphhelm_checkpoints,public.graphhelm_retention_policies,public.graphhelm_evidence_tombstones,public.graphhelm_cleanup_receipts TO %I',role_name);
END $graphhelm$;
