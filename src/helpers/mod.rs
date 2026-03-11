//! Helper commands — human-ergonomic workflows built on top of the API.
//!
//! Helpers add `+` prefixed subcommands to resource groups (e.g., `documents +new`).
//! They provide interactive prompts, `$EDITOR` integration, and multi-step workflows
//! that abstract away raw `--json` payloads.
//!
//! Architecture follows the gws (Google Workspace CLI) pattern:
//! - `inject_commands()` adds clap subcommands to the resource command tree
//! - `handle()` dispatches to the helper implementation, returning `Ok(true)` if handled
//! - Helpers delegate to the same `execute_request` used by the normal CLI path

mod documents;

use std::future::Future;
use std::pin::Pin;

use anyhow::Result;
use clap::{ArgMatches, Command};

use crate::auth::Credentials;
use crate::spec::ApiSpec;

/// Trait for resource-specific helper commands.
///
/// Helpers inject human-friendly `+` subcommands alongside the spec-generated
/// API commands. They're static (not spec-driven) and bridge interactive
/// terminal input to the API executor.
pub trait Helper: Send + Sync {
    /// Inject `+` subcommands into the resource's clap Command.
    fn inject_commands(&self, cmd: Command) -> Command;

    /// Attempt to handle a matched subcommand.
    ///
    /// Returns `Ok(true)` if the helper handled the command (caller should stop).
    /// Returns `Ok(false)` if this isn't a helper command (fall through to normal dispatch).
    /// Returns `Err(...)` if the helper handled it but encountered an error.
    fn handle<'a>(
        &'a self,
        matches: &'a ArgMatches,
        credentials: &'a Credentials,
        api_spec: &'a ApiSpec,
        color: bool,
    ) -> Pin<Box<dyn Future<Output = Result<bool>> + Send + 'a>>;
}

/// Get the helper for a given resource name, if one exists.
pub fn get_helper(resource: &str) -> Option<Box<dyn Helper>> {
    match resource {
        "documents" => Some(Box::new(documents::DocumentsHelper)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn documents_helper_exists() {
        assert!(get_helper("documents").is_some());
    }

    #[test]
    fn unknown_resource_returns_none() {
        assert!(get_helper("nonexistent").is_none());
        assert!(get_helper("collections").is_none());
    }
}
