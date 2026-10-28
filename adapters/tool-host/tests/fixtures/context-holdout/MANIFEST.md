# The frozen context-ranking holdout corpus (#1705)

`tree/` is the corpus the holdout cases in `adapters/tool-host/tests/context_quality.rs` are
measured over. Every file is a byte-exact copy (LF line endings, pinned by `.gitattributes`) of
the path of the same name in this repository at commit
`b1914aff6f8c7f224914ec2d0a6fa22bbbdc5db2`. The test re-derives each sha256 from the bytes on
disk and refuses a corpus that does not match this table.

Why a second corpus: the ten development cases of #1065 were written from their target files'
own vocabulary, and the ranking has been looked at against them. They are a development set.
This holdout is separate: its ten questions were written in a maintainer's plain words, before
the ranking was run on them, and the oracle path of each was fixed at the same time. Ranking
changes are tuned on the development set only; the holdout is read, never tuned against.

Reporting is split in two so that ranking quality and corpus drift do not share one number: the
frozen holdout corpus below (asserted, because bytes and ranking are both fixed) and the live
repository tree (printed, never asserted, because every merge moves it).

The targets are the files the questions were written about: seven implementation files and three
normative documents. The five decoys were chosen before any run, as the repository's broad
documents that name every subsystem; none was chosen for the number it produces.

Updating this corpus is a fixture update in AGENTS.md's sense and needs its reason in the
commit. Rewriting a question or an oracle after a ranking result is seen is retuning on the
holdout; freeze a new holdout instead.

## Targets (one per question)

| Path | sha256 |
| --- | --- |
| `AGENTS.md` | `4b4900fd79b290c885ea0786fa5a12144d7b82ab9292552bc47604ae53147e16` |
| `KEEL.md` | `2a21ba26dcabed621d81f7351a0a4a091efff5adc1f27a46534d7eb861118365` |
| `core/events/src/limits.rs` | `cf48a5e8f470816079fcc3823430e2a8c2a366d1f09e511869335ad2f3a36637` |
| `core/governor/src/apply.rs` | `4473e29af43c7c6fef48a03a509c59c72930edad6ab28eb8f3ca364e2cab2a30` |
| `core/graph/src/canonical.rs` | `b5585b8d8c3152164647bfe74b593b77c73cd0e2ded88a29df8cca0c4fd8b19a` |
| `core/policy/src/evaluator.rs` | `dc1d2a7308e100189ebdb020b5f721dd936d39b178aa3c29bf96695ebfc1de23` |
| `core/process/src/lib.rs` | `4c412abe12fb6f3d7ad0b98ed7d0e71d15cdf0b44dd536449d369b795e8060b7` |
| `core/simulation/src/engine.rs` | `cb18a5c40335277a4ba42e481538c75ccebcea9071b0ad4e8e3c31f9405ee5e9` |
| `docs/process/DELIVERY.md` | `fc9085b3f928a95bf0bf252dbdd66b0d0788bd259f087309492a9e52aebeeccd` |
| `docs/process/LANES.md` | `1bbf510d74489db76c67eee1385cc1077ef1ea2d553e1bf23ebf11fd7ed56fce` |

## Decoys (broad documents chosen before any run)

| Path | sha256 |
| --- | --- |
| `INSTALL.md` | `d0d16062e9c10d02d7dcd618f761b3f168faaa4e32d322c86e649cbfc5bd9af6` |
| `README.md` | `27f4f3b45a96a8b51a32affc09e14e465303a6b08dab69d22787f3f07df86902` |
| `docs/DECISION_REGISTER.md` | `3df5061710817eb68633ef3cbb8c6e89a83c263d664fb0e08f06fe4378127c21` |
| `docs/product/PRODUCT_REQUIREMENTS.md` | `2f05ff66e60e9b4fcb7bbcde0820c41af4f34adf764225b67d53c0c4b70329db` |
| `docs/product/ROADMAP_AND_ACCEPTANCE.md` | `1809f3a625e1976b11febb3a25bb0b503b3ca423d1fb06f8be262575c40c5367` |
