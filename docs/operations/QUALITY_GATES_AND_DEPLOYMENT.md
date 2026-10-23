# Verification and deployment

## Contribution verification

[Keel](../../KEEL.md) and the [delivery process](../process/DELIVERY.md) define how a
change is proved, independently reviewed, and merged at the reviewed head. Run the
checks the change reaches and preserve their actual results. A merge does not by itself
prove the user-visible promise; the journey observer supplies that evidence.

## Deployment boundaries

- GitHub Actions remains disabled. Never enable or trigger a workflow as a verification
  or deployment fallback.
- Runtime installation and deployment remain on infrastructure controlled by the user,
  under D-001, D-002, and ADR-037. The local MVP installation bar is D-053.
- Secrets remain on the target host; deployment must not overwrite them or expose them
  in commands, logs, or evidence.
- Updates and recovery use a pinned revision and preserve the event store. Follow the
  [VPS upgrade, backup and restore runbook](VPS_UPGRADE_BACKUP_RESTORE.md) for the systemd
  installation, and [getting started](../install/GETTING_STARTED.md) for installation paths.
- Observe the affected journey against the deployed target when deployment changes its
  user-visible behavior. Missing observation remains `OBSERVER_MISSING`; it is not success.

The runbook names the concrete backup, update, verification, and restore commands. This
document adds no pre-push tier, full-suite merge gate, or automatic deployment mechanism.
