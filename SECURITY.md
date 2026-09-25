# Security policy

GraphHelm is experimental software. Treat the current `main` branch as the supported development
version. There is no separate stable release line or guaranteed security support period yet.
Older commits and prerelease tags are not maintained as separate security branches; a fix may be
available only on a newer `main` commit.

## Reporting a vulnerability

Do not disclose a suspected vulnerability in a public issue, pull request, discussion, or chat.

Use **Report a vulnerability** on the
[Security advisories page](https://github.com/stabem/GraphHelm/security/advisories). This GitHub
form is the project's private disclosure channel. If the button is unavailable, do not post
sensitive details in a public issue, pull request, discussion, or chat. The maintainer must restore
the private reporting form before accepting reports through this channel.

Please include:

- a short description and impact;
- affected commit, release, or component;
- reproduction steps or a minimal proof of concept;
- any suggested fix;
- whether the report is already known to others.

The response targets are an acknowledgement within seven calendar days, an initial impact and
affected-version assessment within 30 days, and an update at least every 30 days while a report
remains open. These are targets, not a guaranteed fix deadline. The maintainer will coordinate
disclosure with the reporter, publish a security advisory when a fix or mitigation is ready, and
request a CVE identifier when the issue warrants one. Do not include secrets or real user data in
a report.

## Scope

Report vulnerabilities in the GraphHelm source, schemas, CLI, Runtime, Studio, adapters, install
scripts, and documented build or deployment paths. Third-party dependency vulnerabilities should
include the dependency name and affected version so they can be checked against the supported tree.

## Safe testing

Use a local checkout, synthetic data, and disposable credentials. Do not access another person's
data, disrupt shared services, send unsolicited traffic, or test production infrastructure.
