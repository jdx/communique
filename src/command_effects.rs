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
//! absence of a value as "ask", so an unset effect is never dangerous the way
//! a wrong `read` is. That said, the tests below require every command to be
//! listed: for a CLI this small there is no reason to leave one undecided, and
//! "unknown" should be a deliberate choice rather than something forgotten.

use std::collections::HashMap;

use usage::SpecCommandEffect::{self, Destructive, Read, Write};

/// Commands whose effect is fixed, keyed by their full path under `communique`.
pub const EFFECTS: &[(&str, SpecCommandEffect)] = &[
    // Bare `generate` only prints; the danger is in its flags, below.
    ("generate", Read),
    ("init", Write),
    ("sponsors", Read),
    ("usage", Read),
];

/// Flags that raise the effect of their command, keyed by (command, flag).
///
/// usage 4 takes the effect of an invocation to be the maximum of the
/// command's effect and that of every flag supplied, so these only ever raise.
/// Most flags belong nowhere near this table — it is for the few that change
/// what the command does to the world.
pub const FLAG_EFFECTS: &[(&str, &str, SpecCommandEffect)] = &[
    // Rewrites CHANGELOG.md in place.
    ("generate", "changelog", Write),
    // Replaces the body of an already-published GitHub release.
    ("generate", "github-release", Write),
    ("generate", "output", Write),
    // Overwrites an existing communique.toml.
    ("init", "force", Destructive),
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
        let full = path.join(" ");
        if let Some(effect) = effects.get(full.as_str()) {
            sub.effect = Some(*effect);
        }
        for (cmd_path, flag_name, effect) in FLAG_EFFECTS {
            if *cmd_path != full {
                continue;
            }
            if let Some(flag) = sub.flags.iter_mut().find(|f| f.name == *flag_name) {
                flag.effect = Some(*effect);
            }
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

    /// A renamed or removed flag would otherwise silently stop being annotated.
    #[test]
    fn every_flag_effect_matches_a_real_flag() {
        let spec: usage::Spec = Cli::command().into();
        let mut missing = vec![];
        for (cmd_path, flag_name, _) in FLAG_EFFECTS {
            let cmd = spec.cmd.subcommands.get(*cmd_path);
            match cmd {
                Some(c) if c.flags.iter().any(|f| f.name == *flag_name) => {}
                _ => missing.push(format!("{cmd_path} --{flag_name}")),
            }
        }
        assert!(
            missing.is_empty(),
            "these FLAG_EFFECTS entries do not match a real flag:\n  {}",
            missing.join("\n  ")
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
