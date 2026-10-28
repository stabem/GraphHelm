# The frozen context-ranking holdout v2 corpus (#1705)

`tree/` is the corpus the `HOLDOUT_V2_CASES` in `adapters/tool-host/tests/context_quality.rs` are
measured over. Every file is a byte-exact copy (LF line endings, pinned by `.gitattributes`) of
the path of the same name in this repository at commit
`d8f173ef0e26d55da61d1edd39df5c98935fe6ff`. The test re-derives each sha256 from the bytes on
disk and refuses a corpus that does not match this table.

Why a second holdout: the first holdout (`context-holdout/`) has been measured and discussed since it was frozen, so
it can no longer be treated as unseen. This one was written afterwards, by a session that had not read the
ranking code (`source_channel.rs`), any open ranking change, or any context-quality result. The ten
questions are in a contributor's plain words and the oracle of each was chosen by reading the
file's own purpose, not by choosing keywords. None of the ten oracles is an oracle of the first
holdout or of the development set.

Questions and oracles were committed and pushed before the ranking was run on them; the measured
floor was recorded in a later commit. Rewriting a question or an oracle after a ranking result is
seen is retuning on the holdout; freeze a new holdout instead.

The targets are seven implementation files and three normative or process documents. The eleven
decoys were chosen before any run as near neighbours and broad documents (a sibling threat model,
the model-gateway spec the manifest module cites, the planning step beside the review step, the
largest source files); none was chosen for the number it produces.

## Targets (one per question)

| Path | sha256 |
| --- | --- |
| `core/execution/src/autopilot.rs` | `0561a1df5be52044fc28cc53720a8b1ff959a4b7624cdf1da7dbda4bac6843a0` |
| `core/execution/src/ready.rs` | `9867b5fb7b92d2e493fc7cd396c62f0cb9f52143126e45ee6c30ef5d60892296` |
| `core/gateway/src/manifest.rs` | `e615e68df5b9830b49e986ff30dafe840024ce2d04802d9ad47e91d4371fce2a` |
| `core/policy/src/keel_lines.rs` | `3b5c11ede16e23e49cdb974fc38f70a5306d1f3753508f606fb1c8687aab3e58` |
| `core/runtime/src/delegation.rs` | `4f2327e282f427dbba05fc9f7b1a3936efa3f9fa2a5fe25ea3ad0bc2d2ac92f8` |
| `core/runtime/src/owner_output.rs` | `483d68938372ba7c1f589ea7b27d7a076a5cf5a3cbbb5e4cb5b5c37d0ac432f9` |
| `core/schema/src/bounded_value.rs` | `a27268f2caf1bb77f524a1c94a64dc369f9e8ca6ffadc04e93e9313602d5f38e` |
| `docs/operations/VPS_UPGRADE_BACKUP_RESTORE.md` | `259b22cad93b5227d71d9446247c131ac39ee6b8c0af08cb963b2d870b648571` |
| `docs/security/EVENT_EVIDENCE_STORE_THREAT_MODEL.md` | `2e782fae9995cd7e9d84281df09c4a4efe648d2888826e5b4f1b4db5c2799b37` |
| `keel/KEEL_REVIEW.md` | `16bedd84b6a3fc5a9f1ba3e4188ea75e79ce377029345053eb0080ab22ad591f` |

## Decoys (chosen before any run)

| Path | sha256 |
| --- | --- |
| `core/events/src/local.rs` | `0ee7cc70a7743d35b73b59a1f21a648aa493e0a8a95a63d3103fc0a2616afa28` |
| `core/execution/src/tasks.rs` | `caebedbaa2399d4f0d0b6093cd35ec93405a40c1f9ea0b90dc91bc6bbdcc0b35` |
| `core/runtime/src/driver.rs` | `cddd72f73cda26109442697dfded9d1039ff4de55038e8201e809431d5f0a620` |
| `docs/architecture/SYSTEM_ARCHITECTURE.md` | `1970b7190c8274ed26c801ef2cfd63416a15e6b7bd8495e6a0f32c114797e518` |
| `docs/keel/KEEL_SPEC.md` | `c1130f0d472a513a7f8bb718eae5c66f8250569a099361a452fd5d15f4b247ed` |
| `docs/models/UNIVERSAL_MODEL_GATEWAY.md` | `c30bdee8ebe3cae5cc521c8954a8d90ae29055995225983f5c4a685450d2162f` |
| `docs/operations/OBSERVABILITY_AND_RECOVERY.md` | `b1c25dcea4de9dd3e87b60c944ee7d3c986480c2f6bc77933b7aea2192540c76` |
| `docs/process/DELIVERY.md` | `fc9085b3f928a95bf0bf252dbdd66b0d0788bd259f087309492a9e52aebeeccd` |
| `docs/security/SECURITY_ISOLATION_THREAT_MODEL.md` | `ccae42ebc3a9aa54f9d67c102b4e6a882a61a2f4cb375131b73bca6c03bd9224` |
| `keel/KEEL_PLAN.md` | `a8b3bc784d15fedab5bcc4085497de26362013ec97c53f8bc4575ee262274a48` |
| `README.md` | `27f4f3b45a96a8b51a32affc09e14e465303a6b08dab69d22787f3f07df86902` |
