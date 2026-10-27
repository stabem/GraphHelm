# Cross-method headline

| Arm | Runs | P/F/I | Score (95% CI) | Tokens total | Tokens/run | Est. USD total | Est. $/run | Est. $/proven | Tokens/proven | Wall median/max (s) | Timeouts (run-level) | Code lines (true, sum) | Artifact files (mean) |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `s` | 14 | 6/8/0 | 6/14 = 43% [21%, 67%] | 79,693,476 | 5,692,391 | 40.95 | 2.925 | 6.824 | 13,282,246 | 1982 / 2880 | 5 run-level >=2400s (stage cap) | 751 | 13.4 |
| `o` | 14 | 5/8/1 | 5/13 = 38% [18%, 64%] | 57,984,717 | 4,141,765 | 29.44 | 2.103 | 5.888 | 11,596,943 | 824 / 3082 | 1 run-level >=2400s (stage cap) | 479 | 7.9 |

# Per-run table (28)

| # | Task | Arm.Run | Verdict (reasons) | TokTotal | Est.$ | Wall s | Stages (s) | CodeLines(true) | ArtFiles | AddedNA |
|---|---|---|---|---|---|---|---|---|---|---|
| 01 | docs-config-risk | o1 | FAIL (FAIL) | 2,263,643 | 1.151 | 344 | propose=250 apply=94 | 18 | 7 | - |
| 02 | docs-config-risk | o2 | FAIL (FAIL) | 3,838,082 | 1.953 | 827 | propose=496 apply=331 | 21 | 8 | - |
| 03 | docs-config-risk | s1 | FAIL (FAIL) | 2,926,454 | 1.510 | 985 | specify=211 plan=250 tasks=241 implement=283 | 14 | 13 | - |
| 04 | docs-config-risk | s2 | FAIL (FAIL) | 2,741,886 | 1.417 | 1072 | specify=247 plan=207 tasks=200 implement=418 | 12 | 13 | - |
| 05 | docs-doc-comments | o1 | PASS (PASS) | 4,447,941 | 2.258 | 731 | propose=336 apply=395 | 30 | 7 | - |
| 06 | docs-doc-comments | o2 | PASS (PASS) | 2,600,416 | 1.322 | 517 | propose=228 apply=289 | 30 | 7 | - |
| 07 | docs-doc-comments | s1 | PASS (PASS) | 3,097,340 | 1.623 | 1692 | specify=449 plan=573 tasks=446 implement=225 | 30 | 13 | - |
| 08 | docs-doc-comments | s2 | PASS (PASS) | 3,170,795 | 1.657 | 1320 | specify=256 plan=287 tasks=349 implement=428 | 30 | 14 | - |
| 09 | om-win-arm64 | o1 | FAIL (FAIL) | 2,908,114 | 1.484 | 821 | propose=436 apply=385 | 0 | 8 | - |
| 10 | om-win-arm64 | o2 | INCOMPLETE (FAIL,outside_card=1,new_files=1) | 7,786,271 | 3.954 | 3082 | propose=682 apply=2400 | 76 | 8 | windows-arm64-verify-2026-10-26.md |
| 11 | om-win-arm64 | s1 | FAIL (FAIL,FAIL) | 3,898,181 | 2.016 | 1274 | specify=294 plan=509 tasks=226 implement=244 | 0 | 14 | - |
| 12 | om-win-arm64 | s2 | FAIL (FAIL,outside_card=2,new_files=1) | 6,339,502 | 3.305 | 2871 | specify=479 plan=730 tasks=552 implement=1111 | 194 | 15 | windows-arm64-2026-10-26.md |
| 13 | py-actor-session-header | o1 | PASS (PASS) | 2,859,463 | 1.451 | 867 | propose=713 apply=154 | 2 | 8 | - |
| 14 | py-actor-session-header | o2 | PASS (PASS) | 3,614,811 | 1.841 | 1152 | propose=437 apply=715 | 2 | 8 | - |
| 15 | py-actor-session-header | s1 | PASS (PASS) | 5,053,312 | 2.576 | 1667 | specify=189 plan=665 tasks=593 implement=219 | 2 | 13 | - |
| 16 | py-actor-session-header | s2 | PASS (PASS) | 3,720,653 | 1.902 | 1354 | specify=332 plan=521 tasks=297 implement=203 | 2 | 13 | - |
| 17 | py-mcp-meta | o1 | FAIL (PASS,new_tests=1) | 2,579,690 | 1.309 | 596 | propose=288 apply=308 | 8 | 8 | - |
| 18 | py-mcp-meta | o2 | FAIL (PASS,new_tests=4) | 5,559,590 | 2.826 | 1213 | propose=628 apply=586 | 12 | 9 | - |
| 19 | py-mcp-meta | s1 | FAIL (PASS,new_tests=2) | 5,604,406 | 2.883 | 2599 | specify=349 plan=982 tasks=716 implement=553 | 8 | 13 | - |
| 20 | py-mcp-meta | s2 | FAIL (PASS,new_tests=3) | 6,883,273 | 3.525 | 2619 | specify=380 plan=705 tasks=801 implement=733 | 8 | 13 | - |
| 21 | py-sessionend-cap | o1 | FAIL (FAIL) | 3,106,484 | 1.574 | 622 | propose=378 apply=244 | 33 | 8 | - |
| 22 | py-sessionend-cap | o2 | PASS (PASS) | 2,768,519 | 1.400 | 384 | propose=239 apply=145 | 17 | 8 | - |
| 23 | py-sessionend-cap | s1 | PASS (PASS) | 5,551,999 | 2.857 | 2285 | specify=637 plan=983 tasks=199 implement=466 | 26 | 14 | - |
| 24 | py-sessionend-cap | s2 | PASS (PASS) | 6,475,199 | 3.334 | 2272 | specify=404 plan=987 tasks=569 implement=312 | 18 | 14 | - |
| 25 | rs-signal-id-retry | o1 | FAIL (FAIL) | 6,493,348 | 3.296 | 1112 | propose=484 apply=628 | 137 | 8 | - |
| 26 | rs-signal-id-retry | o2 | FAIL (FAIL) | 7,158,345 | 3.624 | 1048 | propose=387 apply=661 | 93 | 8 | - |
| 27 | rs-signal-id-retry | s1 | FAIL (PASS,outside_card=4,new_files=4,new_tests=3) | 9,885,247 | 5.040 | 2880 | specify=194 plan=813 tasks=321 implement=1552 | 193 | 13 | format.json; format.json |
| 28 | rs-signal-id-retry | s2 | FAIL (FAIL) | 14,345,229 | 7.300 | 2504 | specify=352 plan=554 tasks=442 implement=1156 | 214 | 13 | - |

# Per-stage wall (s)

| Arm | Stage | n | mean | median | max |
|---|---|---|---|---|---|
| s | implement | 14 | 565 | 423 | 1552 |
| s | plan | 14 | 626 | 619 | 987 |
| s | specify | 14 | 341 | 340 | 637 |
| s | tasks | 14 | 425 | 395 | 801 |
| o | apply | 14 | 524 | 358 | 2400 |
| o | propose | 14 | 427 | 412 | 713 |

# Sensitivity

| Basis | a (battery 1) | k (battery 1) | s | o |
|---|---|---|---|---|
| all tasks | 8/12 $1.1977 | 9/14 $0.8762 | 6/14 $6.824 | 5/13 $5.888 |
| excl rs-signal-id-retry | 8/11 $0.4355 | 9/12 $0.4360 | 6/12 $4.7677 | 5/11 $4.5043 |
| excl rs + docs-config-risk | 8/10 $0.4135 | 9/11 $0.4660 | 6/10 $4.2799 | 5/9 $3.8836 |

# Failure-mode histogram (blocking reasons, counted per run)

- s: FAIL x6, PASS x3, new_tests x3, outside_card x2, new_files x2
- o: FAIL x6, PASS x2, new_tests x2

# Paired reliability (run1 vs run2 per task)

- s: agreement 7/7
- o: agreement 5/7; splits: om-win-arm64: FAIL vs INCOMPLETE; py-sessionend-cap: FAIL vs PASS