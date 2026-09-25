//! Remediation-as-text (Milestone 05f Task 3): the monitor never mutates — it hands the
//! operator the EXACT command to run. The strings are rendered here, inside the commands
//! layer, and the tests parse them back through the real clap `Cli` definition, so the page
//! and the CLI compile from one source and cannot drift (the positive answer to D-040's
//! scope gravity: instead of a button, the text of the mutation).

use std::path::Path;

/// The one remediation the read-only surface suggests today: readying a blocked node. The
/// enum exists so the NEXT suggestion extends a closed vocabulary instead of scattering
/// format! calls.
pub(crate) enum RemediationAction {
    Approve,
}

/// The exact invocation, quoted for direct paste. The events directory rides as given —
/// the monitor knows the serve process's own `--events`, which is by definition the
/// co-located operator's path.
pub(crate) fn render_invocation(
    action: &RemediationAction,
    events: &Path,
    execution: &str,
    node: &str,
) -> String {
    match action {
        RemediationAction::Approve => format!(
            "graphhelm execution approve --events {} --execution {} --node {}",
            events.display(),
            execution,
            node
        ),
    }
}

/// What a failed node costs (the 3am question "can this wait until morning?"): how many
/// distinct downstream nodes sit behind it, and whether any terminal node is still
/// reachable from the entrypoints WITHOUT passing through it. Pure forward reachability
/// over the edge list — O(V+E), no store access.
pub(crate) struct BlastRadius {
    pub(crate) blocked_downstream: usize,
    pub(crate) terminal_reachable: bool,
}

pub(crate) fn blast_radius(
    edges: &[(String, String)],
    entrypoints: &[String],
    terminals: &[String],
    failed: &str,
) -> BlastRadius {
    let downstream = reach(edges, &[failed.to_owned()], None);
    let alive = reach(edges, entrypoints, Some(failed));
    BlastRadius {
        // Downstream of the failed node, excluding itself.
        blocked_downstream: downstream.iter().filter(|node| *node != failed).count(),
        terminal_reachable: terminals.iter().any(|terminal| {
            terminal != failed && (alive.contains(terminal) || entrypoints.contains(terminal))
        }),
    }
}

/// Every node reachable from `from` (inclusive), never entering `skip`.
fn reach(edges: &[(String, String)], from: &[String], skip: Option<&str>) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    let mut queue: Vec<&str> = from
        .iter()
        .map(String::as_str)
        .filter(|node| Some(*node) != skip)
        .collect();
    while let Some(node) = queue.pop() {
        if seen.iter().any(|known| known == node) {
            continue;
        }
        seen.push(node.to_owned());
        for (source, target) in edges {
            if source == node && Some(target.as_str()) != skip {
                queue.push(target);
            }
        }
    }
    seen
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use std::path::PathBuf;

    fn edges(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(source, target)| ((*source).to_owned(), (*target).to_owned()))
            .collect()
    }

    #[test]
    fn remediation_strings_are_the_cli_own_vocabulary() {
        // The round-trip: the emitted string, split into argv, PARSES under the real Cli.
        // A renamed flag or verb in either place fails this test — one source, no drift.
        let command = render_invocation(
            &RemediationAction::Approve,
            &PathBuf::from("C:/data/events"),
            "exec-monitor",
            "review",
        );
        let argv: Vec<&str> = command.split_whitespace().collect();
        assert_eq!(argv[0], "graphhelm");
        let parsed = crate::args::Cli::try_parse_from(&argv);
        assert!(parsed.is_ok(), "{command} must parse: {parsed:?}");
        let crate::args::TopLevel::Execution(execution) = parsed.unwrap().command else {
            panic!("{command} must parse as an execution command");
        };
        let crate::args::ExecutionCommand::Approve {
            events,
            execution,
            node,
        } = execution.command
        else {
            panic!("{command} must parse as approve");
        };
        assert_eq!(events, PathBuf::from("C:/data/events"));
        assert_eq!(execution.as_deref(), Some("exec-monitor"));
        assert_eq!(node, "review");
    }

    #[test]
    fn blast_radius_on_a_diamond_a_chain_and_a_disconnected_failure() {
        // Diamond: entry → (left, right) → join → terminal. Failing `left` blocks nothing
        // exclusively — join and beyond stay reachable through `right`.
        let diamond = edges(&[
            ("entry", "left"),
            ("entry", "right"),
            ("left", "join"),
            ("right", "join"),
            ("join", "terminal"),
        ]);
        let radius = blast_radius(
            &diamond,
            &["entry".to_owned()],
            &["terminal".to_owned()],
            "left",
        );
        assert_eq!(
            radius.blocked_downstream, 2,
            "join and terminal sit behind left"
        );
        assert!(
            radius.terminal_reachable,
            "the right path still reaches the terminal"
        );

        // Chain: entry → middle → terminal. Failing `middle` cuts the only path.
        let chain = edges(&[("entry", "middle"), ("middle", "terminal")]);
        let radius = blast_radius(
            &chain,
            &["entry".to_owned()],
            &["terminal".to_owned()],
            "middle",
        );
        assert_eq!(radius.blocked_downstream, 1);
        assert!(!radius.terminal_reachable, "the chain is severed");

        // Disconnected failure: an island node fails; the main path never noticed.
        let main_path = edges(&[("entry", "terminal")]);
        let radius = blast_radius(
            &main_path,
            &["entry".to_owned()],
            &["terminal".to_owned()],
            "island",
        );
        assert_eq!(radius.blocked_downstream, 0);
        assert!(radius.terminal_reachable);
    }
}
