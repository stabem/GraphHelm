# KEEL_MERGE

Step 4. Only the reviewer who approved merges. Read [`KEEL.md`](../KEEL.md) first.

1. **Pin the head.** Merge exactly the commit you reviewed:
   `gh pr merge <N> --squash --match-head-commit <sha>`.
2. **Main moved, run again.** If main moved after your review, rerun the reached plan on the new
   main plus the head before you merge. Two changes that each pass alone can break together.
3. **Closing keywords** in the body and the commits must match what the change closes.
4. **Check after.** Read what landed, then run the change's main proof on main and confirm it is
   green. If it is red, the merge is not delivered: fix main first.
5. **Clean up what you created.** The author deletes the branch (unless a pull request is stacked
   on it), the worktree and the build directories. Nobody deletes another lane's.

Delivered means merged into main and green there.
