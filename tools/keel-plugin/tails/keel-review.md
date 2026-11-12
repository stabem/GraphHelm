
## What comes next

| After this | Next skill | Who acts |
|---|---|---|
| APPROVE or APPROVE-WITH-RISK on the head | keel-merge | the approving reviewer merges; the author never does |
| BLOCK on the head | keel-exec on the same branch, then a new review on the new head | author |
| Journey flow waits for approval | keel-journey-qa | journey QA agent |

## If in doubt

| Situation | Skill |
|---|---|
| New task, no plan yet | task-plan |
| Writing code for a claimed task | implement |
| Proving a journey | journey-prove |
| Asked to review a PR | blind-review |
| Got a BLOCK on your head | implement (same branch) |
| Approved and you are the reviewer | merge |
| Unsure what the next step is | graphhelm-work (asks GraphHelm next) |
| Unsure about the whole flow | graphhelm-guide |
