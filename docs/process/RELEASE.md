# Release

Delivered still means merged into `main` ([`keel/KEEL_MERGE.md`](../../keel/KEEL_MERGE.md)). A
release is a separate, later step: it gives `main` a version, a tag and a record. This file is the
only place that says who cuts one, when and how (#2253).

## Owner and trigger

- **Owner:** the release lane, Yamato (`openbot-yamato`). **Approver:** Kakashi
  (`openbot-kakashi`), who approves the release pull request's procedure and contents.
- **When:** Thursdays, when [`docs/releases/UNRELEASED.md`](../releases/UNRELEASED.md) is not empty;
  or on demand, when the owner needs a merged fix installed.

## Versions between releases

A plugin pull request does not bump any version. `plugin_version_guard.py --base <base> --head
<head>` (run by the reached plan) only checks that each changed release group's manifests agree and
do not go below the base. `--write` without `--release` still regenerates extension digests and
canonical goldens. Installed plugin caches are keyed by version, so a host sees new plugin content
only after a release, or after a reinstall from a checkout.

## Steps

1. Start from a fresh `origin/main` commit and claim the release issue as usual
   (`issue-<N>-release-<version>`).
2. Find the last release tag: `git describe --tags --abbrev=0 --match "v*" origin/main`.
3. Move the entries of `docs/releases/UNRELEASED.md` into `docs/releases/<version>.md`; leave
   `UNRELEASED.md` with its headings only.
4. Bump the workspace version in `Cargo.toml` (`[workspace.package] version`) to `<version>`.
   Patch by default; minor when a public contract changes (a new CLI command or flag, an
   event/schema/API change, or a removed surface). Same version for Cargo and the tag.
5. Bump each plugin and builtin extension group whose shipped content changed since the tag:
   `python tools/reached-tests/plugin_version_guard.py --base <last tag> --write --release`.
6. Prove it: `python tools/reached-tests/plugin_version_guard.py --base <last tag> --head HEAD --release`
   passes (every changed group carries one agreeing, higher version), and
   `python tools/reached-tests/run_reached.py --base origin/main --head HEAD --stage implement`
   passes.
7. One pull request, one blind review, merge by the approving reviewer (DELIVERY.md). Kakashi's
   approval of the release is a comment on that pull request.
8. Tag the merged commit and push the tag: `git tag v<version> <merge sha>`,
   `git push origin v<version>`. No GitHub Actions take part.
9. Optional Windows package from that commit: [`docs/releases/WINDOWS_BINARY.md`](../releases/WINDOWS_BINARY.md).
   A package is not a published or installed release.

## Proof of a release

- `git rev-parse v<version>^{commit}` equals the merge sha.
- An install from the tag on Windows reports `graphhelm --version` = `<version>`, and the plugins
  load at the new versions after a restart. Name every platform actually observed; another OS
  is not observed by proxy.

## Recovery

Reinstall from the previous tag. A wrong tag that nobody installed is deleted and re-pushed only with
Kakashi's approval; a tag that was installed is never moved: cut the next patch version instead.
