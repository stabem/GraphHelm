# Open source governance, licensing, and contributions

## 1. Purpose

Build GraphHelm as an open, trustworthy, and adoptable framework, while preserving the possibility of an alternative commercial license over the same codebase.

This document is a product and governance strategy, not legal advice. The final texts of the commercial license, ICLA, CCLA, trademark, and terms require a specialized attorney.

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

## 3. Dual license strategy

### 3.1 Community edition

**GNU Affero General Public License v3 (AGPLv3).**

Motivation:

- the product is naturally run over a network;
- modifications used to offer a network service must make the corresponding source available to users, per the terms of the license;
- protects the reciprocity of the core.

### 3.2 Commercial license

Alternative agreement for organizations that want to embed, modify, or offer the software without the obligations of the AGPLv3.

Possible offerings:

- self-hosted commercial license;
- support and SLA;
- managed deployment;
- hosted observability;
- managed collaboration/SSO/compliance;
- marketplace and billing;
- consulting and implementation;
- contractual indemnification;
- long-term support.

The commercial license must not depend on keeping essential features out of the community repository.

### 3.3 Same codebase

Goal: avoid an "open core" model in which the real product stays closed. Service add-ons may exist, but the full agentic framework remains open.

## 4. Contributor License Agreement

### 4.1 Model

Non-exclusive CLA:

- contributor retains copyright;
- grants broad, permanent, and irrevocable rights necessary to use, modify, distribute, sublicense, and relicense the contribution;
- includes a related patent grant;
- allows both AGPL and commercial distribution;
- preserves attribution/authorship;
- declares the right to contribute;
- separates ICLA and CCLA.

### 4.2 ICLA

For individuals. Must cover the contributor's own contributions and employer declarations when applicable.

### 4.3 CCLA

For companies covering contributions from their employees. Does not automatically replace an individual declaration; the final legal workflow decides the combination.

### 4.4 CLA UX

- short and readable text;
- public page explaining why it exists;
- electronic signature on the first relevant PR;
- automated status check in CI;
- signer data privacy;
- process for corrections;
- CLA version tracked.

### 4.5 Small contributions

Legal policy must define whether small typo/docs fixes require a CLA. For relicensing simplicity, the recommendation is to require a CLA for every merged PR, with low-friction automation.

## 5. Copyright and ownership

For sustainable dual licensing, the maintainer/entity needs to control sufficient rights over all commercially distributed code. Code without a compatible CLA does not enter the dual-license base or requires separate consent.

Third-party code must undergo license compatibility review. AGPL/GPL dependencies can affect commercial distribution and require specific analysis.

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
4. CLA check;
5. tests/conformance;
6. security/license scans;
7. review by code owners;
8. public CI;
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
- commercial license;
- compliance packs;
- training/certification;
- consulting;
- custom adapters.

The community edition remains fully functional self-hosted.

## 20. Risks

### CLA reduces contributions

Mitigation: short text, honest explanation, simple signature, transparent governance.

### AGPL deters companies

Mitigation: clear commercial license and simple evaluation.

### Hostile fork

Mitigation: quality, community, brand, release velocity, and open governance; not closing the core.

### Contributor rights ambiguity

Mitigation: ICLA/CCLA, provenance, and license scans.

### Incompatible dependency

Mitigation: automated license policy and legal review of critical dependencies.

## 21. Legal documents needed before launch

- AGPLv3 LICENSE;
- commercial license agreement;
- ICLA;
- CCLA;
- CLA privacy notice;
- trademark policy;
- terms/privacy for optional services;
- DPA for enterprise hosting;
- export controls review, if applicable;
- contributor guide;
- third-party notices.

## 22. Reference sources

- GNU AGPLv3 and explanation of the network clause: Free Software Foundation.
- ICLA/CCLA model and explanation: Apache Software Foundation.

Links and verification date are in `docs/reference/PROVIDER_AND_LICENSE_REFERENCES.md`.
