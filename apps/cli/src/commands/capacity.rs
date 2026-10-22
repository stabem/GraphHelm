//! #1796 (Studio v2 R5): what the machine and the providers have left, read-only.
//!
//! Three parts, each measured or marked unknown, never guessed:
//! - `providers`: the gateway manifest's routes grouped by provider. No provider publishes how much
//!   subscription quota is left, so `quotaLeft` is always `null` with the reason.
//! - `buildSlots`: every configured slot root's holder and waiters (`workspace slot status`), with
//!   no host path, process id or ticket name, plus the machine's Cargo jobs budget and the jobs the
//!   admission ledger has granted.
//! - `memory`: the operating system's available and total physical memory, a sample, not a
//!   reservation; `null` with a reason on a platform this reader does not know.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

/// Why `quotaLeft` is always `null`: the Runtime has no number to serve.
pub(crate) const QUOTA_UNKNOWN: &str = "no provider publishes the quota left";

/// The whole reply of `GET /v1/capacity`.
pub(crate) fn read(manifest: Option<&Path>, slot_roots: &[PathBuf]) -> Value {
    json!({
        "providers": providers(manifest),
        "buildSlots": build_slots(slot_roots),
        "memory": memory(),
    })
}

/// The manifest's routes by provider, in first-seen order, with how many are enabled.
fn providers(manifest: Option<&Path>) -> Value {
    let Some(manifest) = manifest else {
        return json!({"configured": false, "reason": "no gateway manifest is configured on this server", "list": []});
    };
    let outcome = super::gateway::routes::run(manifest);
    let Some(routes) = outcome
        .output
        .data
        .as_ref()
        .and_then(|data| data["routes"].as_array())
        .filter(|_| outcome.output.ok)
    else {
        return json!({"configured": true, "reason": "the gateway manifest could not be read", "list": []});
    };
    let mut list: Vec<(String, u64, u64)> = Vec::new();
    for route in routes {
        let provider = route["provider"].as_str().unwrap_or("unknown").to_owned();
        let enabled = u64::from(route["enabled"].as_bool().unwrap_or(false));
        match list.iter_mut().find(|row| row.0 == provider) {
            Some(row) => {
                row.1 += 1;
                row.2 += enabled;
            }
            None => list.push((provider, 1, enabled)),
        }
    }
    let list: Vec<Value> = list
        .into_iter()
        .map(|(provider, routes, enabled)| {
            json!({"provider": provider, "routes": routes, "enabledRoutes": enabled,
                "quotaLeft": null, "quotaLeftReason": QUOTA_UNKNOWN})
        })
        .collect();
    json!({"configured": true, "list": list})
}

/// Each slot root's queue without host details, and the machine-wide jobs budget.
fn build_slots(roots: &[PathBuf]) -> Value {
    let mut busy = 0_u64;
    let mut queued = 0_u64;
    let mut unreadable = 0_u64;
    let mut holders = Vec::new();
    let mut waiting = Vec::new();
    for root in roots {
        if !root.is_dir() {
            unreadable += 1;
            continue;
        }
        let outcome = super::workspace_slot::run_status(root);
        let Some(data) = outcome.output.data.filter(|_| outcome.output.ok) else {
            unreadable += 1;
            continue;
        };
        if let Some(holder) = data["holder"].as_object() {
            busy += 1;
            holders.push(
                json!({"lane": holder.get("lane"), "label": holder.get("label"),
                "heldSeconds": holder.get("heldSeconds")}),
            );
        }
        for waiter in data["waiting"].as_array().into_iter().flatten() {
            queued += 1;
            waiting.push(json!({"lane": waiter["lane"], "label": waiter["label"],
                "stage": waiter["stage"], "waitedSeconds": waiter["waitedSeconds"]}));
        }
    }
    let (jobs_budget, jobs_granted, jobs_reason) =
        match super::workspace_slot::build_admission_dir().and_then(|dir| {
            Ok((
                super::workspace_slot::jobs_budget(&dir)?,
                super::workspace_slot::granted_jobs(&dir)?,
            ))
        }) {
            Ok((budget, granted)) => (json!(budget), json!(granted), Value::Null),
            Err(reason) => (Value::Null, Value::Null, json!(reason)),
        };
    json!({"configured": roots.len(), "unreadable": unreadable, "busy": busy, "queued": queued,
        "holders": holders, "waiting": waiting, "jobsBudget": jobs_budget,
        "jobsGranted": jobs_granted, "jobsReason": jobs_reason})
}

/// Available and total physical memory in bytes, or `null` with the reason.
fn memory() -> Value {
    match physical_memory() {
        Ok((available, total)) => {
            json!({"availableBytes": available, "totalBytes": total, "reason": null})
        }
        Err(reason) => json!({"availableBytes": null, "totalBytes": null, "reason": reason}),
    }
}

#[cfg(windows)]
fn physical_memory() -> Result<(u64, u64), &'static str> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut status = MEMORYSTATUSEX {
        dwLength: u32::try_from(std::mem::size_of::<MEMORYSTATUSEX>())
            .map_err(|_| "the memory status could not be read")?,
        ..Default::default()
    };
    // SAFETY: `status` is a live, writable MEMORYSTATUSEX whose `dwLength` names its own size, as
    // the call requires; the call only writes into it.
    let ok = unsafe { GlobalMemoryStatusEx(&mut status) };
    if ok == 0 {
        return Err("the memory status could not be read");
    }
    Ok((status.ullAvailPhys, status.ullTotalPhys))
}

#[cfg(target_os = "linux")]
fn physical_memory() -> Result<(u64, u64), &'static str> {
    let text = std::fs::read_to_string("/proc/meminfo")
        .map_err(|_| "the memory status could not be read")?;
    let field = |name: &str| {
        text.lines()
            .find_map(|line| line.strip_prefix(name))
            .and_then(|rest| rest.trim().strip_suffix("kB"))
            .and_then(|kb| kb.trim().parse::<u64>().ok())
            .map(|kb| kb * 1024)
    };
    match (field("MemAvailable:"), field("MemTotal:")) {
        (Some(available), Some(total)) => Ok((available, total)),
        _ => Err("the memory status could not be read"),
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
fn physical_memory() -> Result<(u64, u64), &'static str> {
    Err("this platform's memory is not read")
}
