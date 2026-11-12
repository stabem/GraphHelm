# Captures paint only secrets (#2848)

Static before/after mocks of a journey frame, drawn before the change (the script that drew them is not kept; they are images only).

- `form-before.png`: a form as a capture showed it: every field, plain or secret, covered by a magenta block.
- `form-after.png`: the same form as a capture shows it now. Search, e-mail and note fields keep their content and get a magenta outline. The password field, the field filled from a flow secret and the visible echo of that secret are painted magenta (the outline also rings the painted fields).
- `studio-before.png`: the Studio's Proof screen with its journey search box covered by a magenta block in a frame.
- `studio-after.png`: the same screen with the search box readable inside its outline.
- `real-after.png`: build evidence, not part of the approved set. It is a capture made by the built driver (`tools/journey-driver/driver.mjs`) on the driver's own fixture page `/fields`: a plain search, e-mail and note field show their text with an outline, an empty contenteditable is outlined, a password field and a field filled from a secret are painted, and so is the echo of that secret.

Rule drawn: a capture paints only password fields, fields the flow filled from a `secret`, visible text equal to a known secret, and embedded frames; every other editable field is outlined, not covered.

Accepted trade, stated to the owner before approval: a value typed in a plain field (for example an e-mail address) becomes visible in captures. A flow that handles something sensitive must mark it as a `secret`.

Not drawn in the mocks, decided in the build: a text field whose `autocomplete` names a credential or payment value (`current-password`, `new-password`, `one-time-code`, `cc-number`, `cc-csc`, `cc-exp`, `cc-exp-month`, `cc-exp-year`) is painted too, as a cheap guard for a secret typed into a `type=text` field.

Approved by Gabriel (owner), 2026-11-12, in the session chat on `form-before.png`, `form-after.png`, `studio-before.png` and `studio-after.png` ("Aprovo, implemente"; earlier, on the rule: "Sim, só segredo pintado").
Approval recorded on GitHub: https://github.com/stabem/GraphHelm/issues/2848#issuecomment-6823318374
