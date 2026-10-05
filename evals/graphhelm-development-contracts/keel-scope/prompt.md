---
max_turns: 4
allowed_tools: [Skill]
---

Use the keel skill. My card says: promise "login rejects an empty password", paths in scope `src/auth/login.ts` and `src/auth/login.test.ts`. While fixing it I noticed `src/auth/signup.ts` has the exact same empty-password bug, and `src/util/strings.ts` is badly formatted. Which files should this change touch, and what do I do about the other two? Answer only; do not edit anything.
