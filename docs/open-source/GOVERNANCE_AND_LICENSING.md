# Open source governance, licensing, and contributions

## 1. Purpose

Build GraphHelm as an open, trustworthy, and adoptable framework under a single permissive code license.

This document is a product and governance strategy, not legal advice. The final texts of the trademark policy and of the terms for any optional hosted service require a specialized attorney.

## 2. Open source constitution

1. Framework, Runtime, and Studio have public source code.
2. No essential function depends on the maintainer's cloud.
3. Everything the Studio does uses a public API.
4. Full self-hosting is supported.
5. External telemetry is opt-in.
6. Protocols and schemas are public.
7. Extensions can be installed outside the official registry.
8. Export contains no intentional lock-in.
9. Commercial services are convenience, support, or management, not core unlocking.
10. Roadmap, RFCs, ADRs, and security policy are public.

## 3. License

**MIT.** One license for GraphHelm-authored code, for every user, with no second tier. The
bundled Studio fonts are third-party font software under OFL-1.1; their notices and full license
texts are in [the font directory](../../apps/studio/src/fonts/README.md). The root MIT license
does not relicense those fonts.

What that means, said plainly rather than left for a reader to infer:

- anyone may use, modify, embed, redistribute and sell the software;
- anyone may run a modified version as a network service and owes nothing back — no source,
  no notice, no fee;
- the only obligation is to keep the copyright and permission notice.

This supersedes an earlier plan for AGPLv3 plus an alternative commercial license. That plan
existed to hold a commercial lever over network use, and it was traded for adoption. The trade
is one-directional: code already published under MIT stays available under MIT to everyone who
received it, so a later change of mind cannot reach what has already been distributed.

## 4. Contributions

Inbound equals outbound: a contribution is offered under the same MIT terms the project is
distributed under, and that is the whole agreement. There is no CLA, no ICLA and no CCLA.

Those documents existed to let one party relicense contributed code commercially. Under a
single permissive license nobody needs that power, so asking contributors to sign anything
would cost goodwill and buy nothing.

Contributors are asked to have the right to contribute what they submit. That is a statement
of fact about the code, not a transfer of rights.

## 5. Copyright and ownership

Each contributor keeps the copyright in what they wrote. Nothing is assigned and nothing needs
to be, because MIT already grants everyone — including this project — the rights required to
distribute the result.

Third-party code still undergoes license compatibility review. A copyleft dependency does not
threaten a commercial tier any more, but it can still impose obligations on everyone who
redistributes GraphHelm, which is now everyone.

## 6. Trademark

A code license does not automatically grant the right to use the name/logo as the official product. Create a separate Trademark Policy:

- nominative use permitted;
- forks may say "compatible with";
- may not pass themselves off as the official release;
- optional partner/certification program;
- protection against malware using the trademark.

"GraphHelm" is the name selected for development; public launch depends on legal search, namespace reservation, and trademark policy.

## 7. Governance structure

### 7.1 Initial phase

- Founder/Lead Maintainer;
- Core Maintainers;
- Module Maintainers;
- Security Team;
- Release Managers;
- Community Moderators.

### 7.2 Evolution

- Technical Steering Committee;
- RFC process;
- transparent voting/consensus;
- conflict of interest policy;
- maintainer succession;
- independent foundation possible after maturity.

## 8. RFC process

Mandatory for:

- new protocol/node type;
- breaking schema change;
- security boundary;
- license/governance change;
- major public API change;
- new trust model;
- critical dependency;
- hosted service coupling;
- data collection change.

Template:

```text
Summary
Motivation
Goals / non-goals
Detailed design
Security/privacy
Compatibility
Alternatives
Migration
Test strategy
Operational impact
Open-source impact
Reference implementation plan
```

States:

- proposed;
- discussion;
- accepted;
- rejected;
- withdrawn;
- implemented;
- superseded.

## 9. ADRs

ADRs record decisions specific to the reference implementation. They do not replace public RFCs for ecosystem-wide changes.

Format:

- context;
- decision;
- alternatives;
- consequences;
- status;
- supersedes.

## 10. Versioning and releases

- SemVer for packages and APIs;
- predictable release train;
- alpha/beta/RC;
- signed tags/artifacts;
- SBOM;
- provenance attestations;
- changelog;
- migration guides;
- commercial/community LTS as capacity allows;
- compatibility matrix.

## 11. Repository

Suggested structure:

```text
/apps/studio
/apps/runtime
/crates-or-packages/core
/packages/graph-dsl
/packages/sdk-typescript
/packages/sdk-python
/packages/schemas
/extensions/builtin
/docs
/rfcs
/adrs
/conformance
/examples
```

The specific language may vary, but the boundaries must remain.

## 12. Contribution workflow

1. issue/discussion for large changes;
2. RFC when necessary;
3. fork/branch;
4. confirm the contribution is authored by the contributor or is licensed for submission;
5. tests/conformance;
6. security/license scans;
7. review by code owners;
8. run the tests the change reaches and applicable Rust lints;
9. merge with changelog;
10. release notes.

## 13. Code review

Require:

- functional correctness;
- contracts/schemas;
- security boundaries;
- backward compatibility;
- observability;
- docs;
- tests;
- performance;
- license provenance.

Changes to the core security/policy/credential broker require a specialized reviewer and, ideally, two approvals.

## 14. Conformance program

Publish a suite to validate:

- Runtime compatibility;
- Studio/API compatibility;
- Graph DSL;
- plugins;
- model adapters;
- sandbox adapters;
- event semantics;
- export/replay.

The "GraphHelm Compatible" seal depends on trademark policy and public tests.

## 15. Security governance

- `SECURITY.md`;
- private disclosure channel;
- response targets;
- CVE process;
- supported versions;
- embargo/coordinated disclosure;
- security advisories;
- dependency alerts;
- incident postmortems published when safe to do so;
- future bug bounty.

## 16. Community norms

- Code of Conduct;
- technical disagreement resolved by evidence;
- no hidden roadmap promises;
- transparent moderation;
- contributor recognition;
- public meeting notes;
- avoid maintainer capture by a vendor.

## 17. Telemetry and community data

Default:

- local metrics on;
- external telemetry off.

An opt-in dataset may collect only clearly described metadata, with anonymization and a delete mechanism. Never include prompts/code/artifacts without a separate, specific opt-in.

## 18. Marketplace/registry governance

The official registry may moderate malware, trademark abuse, and broken packages. However:

- the protocol is open;
- alternate registries are permitted;
- local install is permitted;
- removal/revocation has a public reason when possible;
- paid package terms are visible;
- security metadata cannot be hidden behind payment.

## 19. Compatible monetization

- managed cloud;
- one-click VPS;
- enterprise support;
- SSO/RBAC service;
- hosted observability;
- curated verified registry;
- compliance packs;
- training/certification;
- consulting;
- custom adapters.

Every one of these is a service sold beside the software, never a capability withheld from it.
There is no community edition and no other edition: MIT means the published code is the whole
product, and anything sold has to be worth buying next to it.

## 20. Risks

### Hostile fork

Mitigation: quality, community, brand, release velocity, and open governance; not closing the core.

### A competitor runs it as a service and returns nothing

Accepted, not mitigated. MIT permits exactly this, and it is the price paid for adoption. The
defences left are the ones that were always the real ones: quality, release velocity, brand,
and knowing the problem better than anyone who forked it.

### Contributor rights ambiguity

Mitigation: provenance and license scans. There is no CLA to lean on, so a contribution whose
origin is unclear must be resolved at review time rather than papered over by a signature.

### Incompatible dependency

Mitigation: automated license policy and legal review of critical dependencies.

## 21. Legal documents needed before launch

- MIT LICENSE (in the repository root);
- trademark policy;
- terms/privacy for optional services;
- DPA for enterprise hosting;
- export controls review, if applicable;
- contributor guide;
- third-party notices.

## 22. Reference sources

- MIT license text: Open Source Initiative.

Links and verification date are in `docs/reference/PROVIDER_AND_LICENSE_REFERENCES.md`.
