"""Protect the no-paid-fallback preflight boundary without credentials or inference."""
import copy
import json
import time
from pathlib import Path

import native_quota


def snapshot(percent=67):
    return ({"account": {"type": "chatgpt", "planType": "prolite"}},
            {"ordinaryUsageAllowed": True,
             "rateLimitsByLimitId": {"codex": {"primary": {"usedPercent": percent,
                                                           "resetsAt": time.time() + 3600}}}})


def test_included_allowance_must_be_observed_and_below_conservative_cutoff():
    account, limits = snapshot()
    assert native_quota.check_snapshot(account, limits)["allowed"]
    assert not native_quota.check_snapshot(*snapshot(80))["allowed"]
    assert not native_quota.check_snapshot(*snapshot(100))["allowed"]
    assert not native_quota.check_snapshot(account, limits, cutoff=100)["allowed"]
    limits["ordinaryUsageAllowed"] = None
    assert not native_quota.check_snapshot(account, limits)["allowed"]


def test_credit_availability_does_not_authorize_credit_fallback():
    account, limits = snapshot()
    limits["rateLimitsByLimitId"]["codex"]["credits"] = {"hasCredits": True, "balance": "123"}
    observed = native_quota.check_snapshot(account, limits)
    assert observed["allowed"] and observed["paidFallbackAuthorized"] is False
    assert "123" not in str(observed)
    limits["ordinaryUsageAllowed"] = False
    assert not native_quota.check_snapshot(account, limits)["allowed"]


def test_wrong_bucket_or_account_and_stale_window_refuse():
    account, limits = snapshot()
    wrong = copy.deepcopy(limits)
    wrong["rateLimitsByLimitId"] = {"other-model": wrong["rateLimitsByLimitId"]["codex"]}
    wrong["rateLimits"] = limits["rateLimitsByLimitId"]["codex"]
    assert not native_quota.check_snapshot(account, wrong)["allowed"]
    assert not native_quota.check_snapshot({"account": {"type": "apiKey"}}, limits)["allowed"]
    limits["rateLimitsByLimitId"]["codex"]["primary"]["resetsAt"] = 1
    assert not native_quota.check_snapshot(account, limits)["allowed"]


def test_native_claude_requires_first_party_and_explicit_overage_rejection():
    fixture = Path(__file__).parent / "fixtures/native-sessions/claude-subscription.json"
    rows = json.loads(fixture.read_text(encoding="utf-8"))

    def observe(values):
        return native_quota.claude_subscription_observation("\n".join(json.dumps(v) for v in values))

    assert observe(rows)["allowed"]
    missing_rate = [r for r in rows if r.get("type") != "rate_limit_event"]
    assert not observe(missing_rate)["allowed"]
    changed = copy.deepcopy(rows)
    next(r for r in changed if r.get("type") == "rate_limit_event")["rate_limit_info"]["isUsingOverage"] = True
    assert not observe(changed)["allowed"]
    changed = copy.deepcopy(rows)
    next(r for r in changed if r.get("subtype") == "init")["apiKeySource"] = "ANTHROPIC_API_KEY"
    assert not observe(changed)["allowed"]
