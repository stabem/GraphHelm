# The frozen context-quality corpus (#1065)

`tree/` is the corpus `adapters/tool-host/tests/context_quality.rs` measures hit rate@3 (success@3) over.
Every file is a byte-exact copy (LF line endings, pinned by `.gitattributes`) of the path of the
same name in this repository at commit `1ac2438ed920a302da05f1fa25910653eb5d92ff` (the head of
#1078 on 2026-09-14). The test re-derives each sha256 from the bytes on disk and refuses a corpus
that does not match this table, so the floor it asserts is a property of THESE bytes and of
nothing that moves.

Why frozen: the first measurement ran over the live repository tree, and three merges of
documentation (`docs/install/GETTING_STARTED.md`, `docs/product/PROVIDER_LESS_MODE.md`, the
decision register) moved the number from 0.60 to 0.40 without a line of retrieval code changing.
A floor over a moving corpus is not a deterministic test (AGENTS.md: assertions must not depend on
a moving population). The live tree is still measured, and the number printed, with no floor.

The ten target files are the ones the ten objectives were written from. The five decoys are the
documents that outrank an implementation file under a term-count ranking: they mention every
subsystem by name. Nothing was chosen for the number it produces: the targets are fixed by the
objectives, and the decoys are the files that were observed displacing them in the live runs.

Updating this corpus is a fixture update in AGENTS.md's sense and needs its reason in the commit:
re-copy the files, re-pin the commit above, regenerate the table (`sha256sum` over LF bytes), and
re-measure the floor.

## Targets (one per objective)

| Path | sha256 |
| --- | --- |
| `adapters/tool-host/src/source_channel.rs` | `df78e19f976bcd89b68789a0dd43396873a3c25136dcfdebcebe545ff3170f08` |
| `adapters/tool-host/src/source_reader.rs` | `0377a98d4d2b33e3585a44555ff6e30d32835ffad4a7b74fd05179edb1ef2252` |
| `apps/cli/src/commands/development.rs` | `e9027b345b2474b344f70c0574a8d3aec1f2e60fc1a100fa8e0ed97a25ac5ced` |
| `apps/cli/src/commands/serve/mod.rs` | `d7e0734b766727b6ebff355d20d81c2794b8aed8fe1304b86e53548a9a352971` |
| `core/runtime/src/context_accounting.rs` | `08870202336a8549add694c8bbc7adcef1acdaf65dd311a5aa3fc53f49f938cf` |
| `core/runtime/src/context_compiler.rs` | `9356baf4b5730868771905201750162d4074959fff622c1140dac0ae46445194` |
| `core/runtime/src/driver.rs` | `4d3be33a123e4285f8e0dfed29c91a7849a37709b17e85ec9fd4f28befd587a5` |
| `core/runtime/src/ports.rs` | `9a1cb4b04b6d69bf0208e061e26e4b122fdda618594a494d97e9c580e87f99ee` |
| `core/runtime/src/prompt.rs` | `72ac254769b8b72df7df85d9bedf0fc0c302cf4b86662231043831766857b6da` |
| `core/runtime/src/retrieval.rs` | `6c4cf8809318146e5f1ae033d946c49cecbe561245f3bbc7f17ba0a2a5be05a1` |

## Decoys (the documents that outrank implementation files)

| Path | sha256 |
| --- | --- |
| `CHANGELOG.md` | `e33e60239dc32aa37ebaad9f38588d507c84c7e279869dc768d59820decf5808` |
| `docs/DECISION_REGISTER.md` | `3484dd38dcc746ec376e89326d2bd04e671f997a2e2d57c1d12c48711b32126b` |
| `docs/context/CONTEXT_KNOWLEDGE_DREAMS.md` | `10fa4fbc7375aa51793b7ca3506bea7735238b9c4a0dfec9fa3844bc4d6560dc` |
| `docs/install/GETTING_STARTED.md` | `589776e9441cd1d01ed6c27b074ebe72cfa3c0472ddd3c69221f7fcdeb6ab424` |
| `docs/product/PROVIDER_LESS_MODE.md` | `ffb42d9a6b9f299122d4c9c43191de82348ad3824e498c2aaf4e9712aeb1ffe9` |
