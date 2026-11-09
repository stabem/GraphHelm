"""#2643 slice 1: guard for the ready-step Stop block.

A ready step blocks the stop at most once per step key (issue, step, head): the key is recorded when
the block is injected, and the same key never blocks again. A session turn blocks at most
MAX_BLOCKS_PER_TURN times; after that the stop is allowed and the refusal is logged by the caller.
Pure logic: no Runtime, no subprocess, no files.
"""
from __future__ import annotations

MAX_BLOCKS_PER_TURN = 3


def new_state() -> dict:
    return {"injected": set(), "blocks": 0}


def step_key(issue: int, step: str, head: str) -> tuple:
    return (issue, step, head)


def should_block(state: dict, key: tuple) -> bool:
    return key not in state["injected"] and state["blocks"] < MAX_BLOCKS_PER_TURN


def record_block(state: dict, key: tuple) -> None:
    state["injected"].add(key)
    state["blocks"] += 1
