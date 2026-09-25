# Public repository readiness

This is a point-in-time assessment for [issue #1309](https://github.com/stabem/GraphHelm/issues/1309), not permission to change repository visibility. The repository was private when this assessment was made. The final publication decision must use a fresh scan of the exact remote state.

## Secret and disclosure inventory

The source snapshot was `c7f562e8be4e901b410b8e987162d9f4f8b4bd03` on 2026-09-25. Gitleaks `8.30.1` (official Windows x64 asset SHA-256 `d29144deff3a68aa93ced33dddf84b7fdc26070add4aa0f4513094c8332afc4e`) scanned `git log --all` after fetching all 297 remote branch heads, 12 tags, and 732 pull-request head refs. The resulting local closure held 8,314 commits. The scan reported 172 pattern matches across 38 rule-and-file groups. Reviewed groups were test sentinels, deliberately invalid credential examples, checksums, commit hashes, or other acceptance evidence; no live credential was confirmed. The report is redacted and remains outside the repository. These classifications do not prove that no unknown secret exists.

TruffleHog `3.97.9` ran over the current working tree with verification and update checks disabled. Its 13 unverified URI matches were in test examples or documentation describing userinfo stripping. This is a second detector over the current tree, not a second Git-history scan and not an online validity check.

GitHub surfaces outside Git were also inspected. All 13 historical Actions run log archives (28 extracted files) produced no Gitleaks matches; there were no Actions artifacts. The scan of 1,309 issue/pull-request bodies, 7,430 issue comments, 3,930 review comments, and 3,149 review summaries produced seven matches. Their source text was checked: they refer to test sentinels, synthetic `apiKey` examples, or a commit hash, not a confirmed live credential. Review summaries were fully paginated, including pull requests with more than 20 reviews. Raw logs and downloaded discussion bodies remain outside the repository.

An offline `cargo metadata --locked` check found 352 third-party Rust packages and no missing license field or license-file reference. The Studio lockfile listed 162 Node packages and no missing license field; 12 entries are platform variants of `lightningcss` under MPL-2.0. This is a metadata inventory, not a full legal or binary-distribution review.

## Before changing visibility

- Choose a private vulnerability-reporting channel and make `SECURITY.md` describe the channel that actually works. GitHub private vulnerability reporting is available to public repositories; it cannot serve as a verified pre-public channel while this repository remains private.
- Decide whether the existing Git history may expose the developer username, local paths, worktree names, and execution metadata in acceptance evidence. Those values occur in historical commits and cannot be removed by editing the current tree. A history rewrite would change commit identities and disrupt existing pull requests and evidence; it requires a separate migration plan and explicit owner decision. No raw credential was confirmed in the reviewed examples.
- Review the publication's exact remote branch and pull-request refs again after the final merge. Check the new diff and any new Actions logs, issues, comments, and reviews since this snapshot. Do not treat this point-in-time scan as a permanent pass.
- Verify the security reporting channel, project documentation, local validation result, and current license/provenance obligations. GitHub secret scanning is disabled in the private repository; enable and review the available protection features after publication.
- Resolve the launch-name requirement in Decision Register D-034: it calls for legal clearance and namespace reservation before a public launch. Record whether opening this source repository is the launch covered by that decision, and obtain the required clearance if so.

The current decision is **not ready to change visibility** until the reporting channel, metadata disclosure, naming decision, and final remote-state checks are resolved. Product release readiness remains separate from publishing the source repository.
