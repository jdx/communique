//! What each communique command does to the world.
//!
//! communique's usage spec is derived from clap, and clap has no way to express
//! this, so the classification lives here and is applied in [`crate::usage`].
//!
//! The three values are defined by the usage spec:
//!
//! - `read` — only inspects state; running it twice is the same as running it
//!   once, and not running it changes nothing.
//! - `write` — creates or modifies state, but removes nothing the user cannot
//!   recreate.
//! - `destructive` — removes something the user installed or configured, where
//!   getting it back means redoing work. Deserves a confirmation prompt.
//!
//! **An unlisted command means "unknown", not "safe".** Consumers treat the
//! absence of a value as "ask", so leaving a command out is the conservative
//! choice and mislabeling one `read` is the dangerous one.

use std::collections::HashMap;

use usage::SpecCommandEffect::{self, Read, Write};

/// Commands whose effect is fixed, keyed by their full path under `communique`.
pub const EFFECTS: &[(&str, SpecCommandEffect)] = &[
    // Bare `generate` only prints, but `--changelog` rewrites CHANGELOG.md and
    // `--github-release` replaces the body of a published release. `write` is
    // the honest floor: it keeps the command out of any read-only allowlist,
    // which is the distinction that actually matters here.
    ("generate", Write),
    ("init", Write),
    ("sponsors", Read),
    ("usage", Read),
];

/// Annotate every command in the spec that has a declared effect.
pub fn apply(spec: &mut usage::Spec) {
    let effects: HashMap<&str, SpecCommandEffect> = EFFECTS.iter().copied().collect();
    annotate(&mut spec.cmd, &mut vec![], &effects);
}

fn annotate(
    cmd: &mut usage::SpecCommand,
    path: &mut Vec<String>,
    effects: &HashMap<&str, SpecCommandEffect>,
) {
    for (name, sub) in cmd.subcommands.iter_mut() {
        path.push(name.clone());
        if let Some(effect) = effects.get(path.join(" ").as_str()) {
            sub.effect = Some(*effect);
        }
        annotate(sub, path, effects);
        path.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Cli;
    use clap::CommandFactory;
    use std::collections::HashSet;

    /// Every command in the tree, hidden ones included: a hidden command is
    /// still runnable.
    fn all_commands() -> Vec<String> {
        let spec: usage::Spec = Cli::command().into();
        let mut out = vec![];
        collect(&spec.cmd, &mut vec![], &mut out);
        out
    }

    fn collect(cmd: &usage::SpecCommand, path: &mut Vec<String>, out: &mut Vec<String>) {
        for (name, sub) in &cmd.subcommands {
            path.push(name.clone());
            out.push(path.join(" "));
            collect(sub, path, out);
            path.pop();
        }
    }

    /// Adding a command without deciding what it does to the world is the
    /// failure mode this table exists to prevent, so make it a test failure
    /// rather than a silently missing annotation.
    #[test]
    fn every_command_is_classified() {
        let known: HashSet<&str> = EFFECTS.iter().map(|(name, _)| *name).collect();
        let missing: Vec<String> = all_commands()
            .into_iter()
            .filter(|cmd| !known.contains(cmd.as_str()))
            .collect();
        assert!(
            missing.is_empty(),
            "these commands have no entry in EFFECTS (src/command_effects.rs) — \
             decide whether each is read, write or destructive:\n  {}",
            missing.join("\n  ")
        );
    }

    /// Catches entries left behind by a renamed or removed command.
    #[test]
    fn no_classification_refers_to_a_missing_command() {
        let present: HashSet<String> = all_commands().into_iter().collect();
        let stale: Vec<&str> = EFFECTS
            .iter()
            .map(|(name, _)| *name)
            .filter(|name| !present.contains(*name))
            .collect();
        assert!(
            stale.is_empty(),
            "these entries no longer match a command:\n  {}",
            stale.join("\n  ")
        );
    }

    #[test]
    fn classifications_are_not_duplicated() {
        let mut seen = HashSet::new();
        for (name, _) in EFFECTS {
            assert!(seen.insert(name), "{name} is classified twice");
        }
    }
}
