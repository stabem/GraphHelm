# Security policy

GraphHelm is experimental software. Treat the current `main` branch as the supported development
version. There is no separate stable release line or guaranteed security support period yet.
Older commits and prerelease tags are not maintained as separate security branches; a fix may be
available only on a newer `main` commit.

## Reporting a vulnerability

Do not disclose a suspected vulnerability in a public issue, pull request, discussion, or chat.

GitHub private vulnerability reporting is currently disabled. GitHub makes it available only
after the repository becomes public. As part of publication, the maintainer must enable it in
**Settings → Security → Advanced Security → Private vulnerability reporting** and verify the
**Report a vulnerability** button on the
[Security advisories page](https://github.com/stabem/GraphHelm/security/advisories). Once that
button works, use it as the project's private disclosure channel.

Until then, there is no documented public-facing private contact address for security reports.
Do not send sensitive details through a public GitHub surface. Publication is not complete until
a private reporting channel is verified; if the GitHub feature cannot be enabled, the maintainer
must provide and verify another private channel.

Please include, when the private channel is available:

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
