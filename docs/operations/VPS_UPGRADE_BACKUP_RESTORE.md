# Upgrade and recover a systemd VPS installation

This runbook upgrades the Ubuntu systemd installation, creates local event-store backups, and restores a failed or replacement machine. It applies to the installation created by `install/install.sh` and uses only the local `--repository` event-store mode.

> **Governance status: this path is an authorised exception to D-002.** `docs/DECISION_REGISTER.md` D-002 is normative for version 0.1 and says *"Existing VPS connected via SSH; installation and updates via Docker."* The systemd host-side lifecycle described below — install, upgrade, backup timer, restore — is an exception to that clause **along one axis only**, recorded as **ADR-037** in `docs/reference/REFERENCE_STACK_AND_ADRS.md` and accepted on the merge that brought this runbook to `main`. The SSH clause of D-002 stands, and Docker remains the packaging for the runtime image: this exception covers the *host-side* lifecycle and nothing else. (Required by ADR-037's own *Affected contracts* entry, per #663.)

## Know the safety boundary

Each recovery bundle contains the local event archive, the exact approved systemd unit, and a checksum manifest. The Runtime bearer token is never included. The backup script writes the final bundle with mode `0600`.

The event-backup producer runs as the service user under a 256 MiB file-size limit and a 1 GiB address-space limit before the result is sealed. These are finite resource-refusal bounds: 256 MiB is the accepted archive ceiling, while an archive below that ceiling can still be refused if the producer exceeds the address-space limit.

The bundle contains operational history and must still be treated as sensitive data. Copy it off the VPS with your approved encrypted transfer and limit access to recovery operators. The included timer does not copy or delete bundles.

These scripts do not back up PostgreSQL. If your installation uses PostgreSQL, add the database operator's backup and restore procedure before relying on this runbook.

## Install the operator files

Run these commands as `root` from the GraphHelm repository root:

```bash
install -m 0755 deploy/backup-vps.sh /usr/local/sbin/graphhelm-backup-vps
install -m 0755 deploy/restore-vps.sh /usr/local/sbin/graphhelm-restore-vps
install -m 0755 deploy/upgrade-vps.sh /usr/local/sbin/graphhelm-upgrade-vps
install -d -m 0755 /usr/local/libexec/graphhelm
install -m 0755 deploy/seal-vps-file.py /usr/local/libexec/graphhelm/seal-vps-file.py
install -m 0644 deploy/graphhelm-backup.service /etc/systemd/system/graphhelm-backup.service
install -m 0644 deploy/graphhelm-backup.timer /etc/systemd/system/graphhelm-backup.timer
systemd-analyze verify /etc/systemd/system/graphhelm-backup.service /etc/systemd/system/graphhelm-backup.timer
systemctl daemon-reload
```

The upgrade helper must stay beside the installed backup and restore helpers. The file-sealing helper must stay at `/usr/local/libexec/graphhelm/seal-vps-file.py`; it prevents root from following builder- or service-owned result paths. All three operations use `/run/graphhelm-operation.lock`, so they cannot modify the installation at the same time.

## Upgrade the VPS

This runbook requires `/opt/GraphHelm` to remain at the commit used to build the installed binary. Stop if that relationship is unknown or the checkout has local changes. Record the installed commit and start from a healthy service:

```bash
git -C /opt/GraphHelm status --porcelain
git -C /opt/GraphHelm show -s --format='%H %cI %s' HEAD
systemctl is-active graphhelm.service
curl --silent --show-error --fail http://127.0.0.1:8080/health >/dev/null
```

`status --porcelain` must print nothing. Record the `show` output beside the pre-upgrade bundle, because the installed binary creates that archive.

Create and move the pre-upgrade bundle off-host before changing the source checkout:

```bash
graphhelm-backup-vps --destination /var/backups/graphhelm
systemctl is-active graphhelm.service
```

Record the printed bundle path and copy that file off the VPS. Do not continue until the off-host copy is protected as sensitive data.

Choose the reviewed 40-character commit SHA that you intend to install, then detach the clean checkout at that exact commit:

```bash
target_commit=reviewed_40_character_commit_sha
git -C /opt/GraphHelm fetch --prune origin main
git -C /opt/GraphHelm cat-file -e "${target_commit}^{commit}"
git -C /opt/GraphHelm switch --detach "${target_commit}"
test "$(git -C /opt/GraphHelm rev-parse HEAD)" = "${target_commit}"
git -C /opt/GraphHelm status --porcelain
git -C /opt/GraphHelm show -s --format='%H %cI %s' HEAD
```

`status --porcelain` must print nothing. Record the final `show` output with the upgrade evidence. The checkout must contain `Cargo.lock`, `Cargo.toml`, and `apps/cli/Cargo.toml`.

Run the upgrade against the prepared checkout:

```bash
graphhelm-upgrade-vps --source /opt/GraphHelm
```

The helper performs this sequence:

1. Builds `graphhelm-cli` with Rust `1.97.1` and the locked dependency set as the unprivileged `graphhelm-build` user
2. Confirms that the candidate has `events backup` and `events restore`
3. Takes the shared operation lock
4. Saves the old binary
5. Calls the backup helper, which stops `graphhelm.service`, runs local event backup as the unprivileged `graphhelm` user, creates a recovery bundle, and leaves the service stopped
6. Restores the event archive into an isolated temporary repository with the installed CLI, then runs the candidate's `events verify --repository` against only that copy as the unprivileged `nobody` verifier user
7. Swaps the binary only when the local event format is supported
8. Starts the candidate and checks health, an unauthenticated `401`, and an authenticated `404`
9. Confirms that event, token, and systemd-unit fingerprints did not change
10. Starts the service again and repeats the health and authentication smoke

A successful run prints `GraphHelm upgrade completed and passed health/auth/fingerprint proof.` It also prints the retained recovery directory under `/var/backups/graphhelm-upgrade/`.

### Handle event-format changes

The current CLI has no generic `events migrate` command. The current local format was compatible during this runbook's validation, so no migration command ran.

If the candidate reports `formatSupported: false`, the helper stops before swapping the binary, retains the backup, and restarts the old service. Stop the upgrade there. Use only release-specific migration instructions that name a real CLI command. Do not use restore as an improvised migration.

### Confirm the upgraded service

Run these checks after a successful result:

```bash
systemctl is-active graphhelm.service
curl --silent --show-error --fail http://127.0.0.1:8080/health >/dev/null
```

Create a post-upgrade disaster-recovery bundle and record the target commit beside its accepted off-host copy:

```bash
graphhelm-backup-vps --destination /var/backups/graphhelm
git -C /opt/GraphHelm show -s --format='%H %cI %s' HEAD
```

Use a known execution ID when you need to prove that historical events remain readable. The local CLI needs exclusive access to the repository, so stop the service around this check:

```bash
systemctl stop graphhelm.service
graphhelm execution status --events /var/lib/graphhelm/events --execution your_known_execution_id
systemctl start graphhelm.service
```

Wait for `/health` to return successfully before reconnecting clients.

### Let a failed smoke roll back

Do not replace the binary manually when the candidate smoke fails. The helper automatically restores the old binary and the pre-upgrade recovery bundle, then repeats the smoke with the old version.

The successful rollback message starts with `graphhelm upgrade: candidate failed and was rolled back`. Confirm the result:

```bash
systemctl is-active graphhelm.service
curl --silent --show-error --fail http://127.0.0.1:8080/health >/dev/null
```

The recovery directory keeps the old binary, failed candidate, pre-upgrade bundle, failed-store archive, receipts, and fingerprints. Preserve it until you understand the failure.

If rollback is incomplete, the helper leaves the service stopped and prints `rollback was incomplete`. Do not force-start the service. Preserve the printed recovery directory and diagnose the failed binary, restore log, and event archives first.

## Schedule periodic backups

The supplied timer creates one bundle every day at `03:00 UTC`. `Persistent=true` runs a missed backup after the VPS returns.

Each local backup stops `graphhelm.service` long enough to create a consistent event archive, then restarts it. Choose a backup time that permits this short interruption.

Enable the timer and create the first scheduled-style backup now:

```bash
systemctl enable --now graphhelm-backup.timer
systemctl start graphhelm-backup.service
systemctl is-active graphhelm-backup.timer
systemctl list-timers graphhelm-backup.timer --no-pager
systemctl show graphhelm-backup.service --property=Result --value
```

Confirm that the newest file exists and has mode `0600`:

```bash
find /var/backups/graphhelm -maxdepth 1 -type f -name 'graphhelm-vps-*.tar' -printf '%TY-%Tm-%TdT%TH:%TM:%TS %m %p\n' | sort | tail -1
```

Monitor `graphhelm-backup.service`, copy every accepted bundle off-host, and apply retention only after the off-host copy is verified. This repository does not choose or automate a retention policy.

## Restore on a replacement machine

Use the source commit recorded beside the accepted off-host bundle. If that commit is unavailable, stop and identify a documented compatible release before restoring.

Run the replacement-machine steps as `root`. On a clean Ubuntu 24.04 machine, clone GraphHelm and detach at the recorded commit:

```bash
git clone --branch main --single-branch https://github.com/stabem/GraphHelm.git /opt/GraphHelm
target_commit=recorded_40_character_commit_sha
git -C /opt/GraphHelm fetch --prune origin main
git -C /opt/GraphHelm cat-file -e "${target_commit}^{commit}"
git -C /opt/GraphHelm switch --detach "${target_commit}"
test "$(git -C /opt/GraphHelm rev-parse HEAD)" = "${target_commit}"
git -C /opt/GraphHelm status --porcelain
bash /opt/GraphHelm/install/install.sh
systemctl is-active graphhelm.service
curl --silent --show-error --fail http://127.0.0.1:8080/health >/dev/null
```

`status --porcelain` must print nothing. The clean installer creates the service users, binary, systemd unit, empty event directory, and a new token. The restore replaces the empty repository and unit with the backed-up state. It removes the machine-local token and lets the restored Runtime mint a fresh local token; no credential travels in the recovery bundle.

Install the restore helper from the matching GraphHelm checkout:

```bash
install -m 0755 /opt/GraphHelm/deploy/restore-vps.sh /usr/local/sbin/graphhelm-restore-vps
install -d -m 0755 /usr/local/libexec/graphhelm
install -m 0755 /opt/GraphHelm/deploy/seal-vps-file.py /usr/local/libexec/graphhelm/seal-vps-file.py
install -d -m 0700 /var/backups/graphhelm
```

Transfer one protected bundle to the replacement machine. Normalize its owner and mode without modifying the source copy:

```bash
install -o root -g root -m 0600 /path/to/transferred-graphhelm-vps.tar /var/backups/graphhelm/disaster-recovery.tar
stat --format '%U:%G %a %n' /var/backups/graphhelm/disaster-recovery.tar
```

The `stat` result must start with `root:root 600`. Restore the bundle:

```bash
graphhelm-restore-vps --bundle /var/backups/graphhelm/disaster-recovery.tar --replace-existing
systemctl is-active graphhelm.service
curl --silent --show-error --fail http://127.0.0.1:8080/health >/dev/null
```

`--replace-existing` is required because the clean installer creates an empty repository. The restore helper quarantines that repository, verifies the closed bundle layout and checksums, restores the systemd unit and local event archive, removes the prior machine-local token, starts the Runtime so it creates a fresh token, and checks health and authentication.

Confirm that the new token exists only on the replacement machine and has the expected owner, mode, and shape:

```bash
stat --format '%U:%G %a %s %n' /var/lib/graphhelm/events.token
```

The result must start with `graphhelm:graphhelm 600 64`. Distribute that fresh token to clients only through your approved secret-management channel.

Prove a known historical execution after restore:

```bash
systemctl stop graphhelm.service
graphhelm execution status --events /var/lib/graphhelm/events --execution your_known_execution_id
systemctl start graphhelm.service
```

Keep the quarantined repository until the recovered service and known execution both pass. Then configure the periodic timer and off-host copy on the replacement machine.

## Understand failure behavior

| Failure | Result |
|---|---|
| Backup fails after stopping an active service | The backup helper restarts the service unless `--leave-stopped` was explicit |
| Candidate cannot read the event format | No binary swap occurs, the backup remains, and the old service restarts |
| Candidate smoke or fingerprint proof fails | The upgrade helper restores the old binary and pre-upgrade bundle |
| Automatic rollback cannot prove recovery | The service remains stopped and the recovery evidence remains on disk |
| Restore fails after moving an existing repository | The restore helper moves the failed repository aside and restores the prior repository and config |
| Another backup, restore, or upgrade is active | The new operation exits before stopping or changing the service |

## Validation record

This runbook was rehearsed on a disposable Ubuntu 24.04 systemd installation on 2026-08-31. The clean installation, backup, timer, restore, successful upgrade, and forced rollback rehearsal started from `main` commit `b4b0be74253169e8522ec8b73a316eb508494d97`. A second successful full upgrade and local CLI check ran against `f6df511795a86b1de9394eda388a084d877ac58b`. Another real backup, restore, full upgrade, and local CLI check ran against `dc03eafa00085eb858f0d2dd25123ebf0f34a225`.

After the review hardening, the affected flows ran against `main` commit `b60b61f2e521cfc52eb9047c22c8f2d53e6213a8` and were repeated after the final rebase to fresh `main` commit `2b95611fa4a5f3bba520795ef164743860961af9`. The real bundle contained exactly `events.archive`, `graphhelm.service`, and `manifest.sha256`, and did not contain the live token. Restore minted a different machine-local token with owner `graphhelm:graphhelm`, mode `0600`, and length 64. Upgrade restored the archive into a temporary repository, recorded successful isolated `events.restore` and candidate `events.verify` receipts, and completed its health, authentication, and fingerprint proof.

Across the rehearsals, the daily systemd timer, restore over a clean install, successful binary upgrades, and automatic rollback after a deliberately broken `/health` route all ran. A known execution remained at event sequence `12` after backup, restore, upgrade, and rollback. `shellcheck` and both script test suites passed.
