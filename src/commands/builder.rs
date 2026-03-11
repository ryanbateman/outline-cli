use crate::spec::ApiSpec;
use clap::{Arg, Command};

/// Leak a String to get a &'static str.
///
/// clap's `Command::new()` requires `&'static str`. Since we build the command
/// tree dynamically from the OpenAPI spec at startup, we leak the strings.
/// This is intentional — the command tree lives for the entire process lifetime,
/// so the memory is never "wasted".
fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

/// Build the full clap command tree dynamically from the parsed API spec.
///
/// Structure:
///   outline <resource> <action> [--json '{}'] [--output json] [--dry-run] [--fields "..."]
///
/// Each resource (documents, collections, etc.) becomes a subcommand.
/// Each action (create, list, info, etc.) becomes a sub-subcommand.
pub fn build_cli(spec: &ApiSpec) -> Command {
    let mut app = Command::new("outline")
        .version(env!("CARGO_PKG_VERSION"))
        .about("AI-agent-first CLI for Outline knowledge base")
        .subcommand_required(true)
        .arg_required_else_help(true)
        // Global flags
        .arg(
            Arg::new("output")
                .long("output")
                .short('o')
                .help("Output format")
                .value_parser(["json", "text"])
                .default_value("text")
                .global(true),
        )
        .arg(
            Arg::new("fields")
                .long("fields")
                .help("Comma-separated list of fields to include in response")
                .global(true),
        )
        .arg(
            Arg::new("dry-run")
                .long("dry-run")
                .help("Validate request without executing")
                .action(clap::ArgAction::SetTrue)
                .global(true),
        )
        .arg(
            Arg::new("page-all")
                .long("page-all")
                .help("Automatically paginate and stream all results as NDJSON")
                .action(clap::ArgAction::SetTrue)
                .global(true),
        )
        .arg(
            Arg::new("sanitize")
                .long("sanitize")
                .help("Sanitize response to remove control characters and prompt injection vectors")
                .action(clap::ArgAction::SetTrue)
                .global(true),
        )
        .arg(
            Arg::new("color")
                .long("color")
                .help("Color output")
                .value_parser(["auto", "always", "never"])
                .default_value("auto")
                .global(true),
        );

    // Add schema introspection command
    app = app.subcommand(
        Command::new("schema")
            .about("Inspect API method schema (e.g., outline schema documents.create)")
            .arg(
                Arg::new("method")
                    .help("Method to inspect, e.g. documents.create")
                    .required(true),
            ),
    );

    // Add MCP server command
    app = app.subcommand(
        Command::new("mcp")
            .about("Start MCP (Model Context Protocol) server over stdio")
            .arg(
                Arg::new("expose")
                    .long("expose")
                    .help("Comma-separated list of resources to expose (default: all)")
                    .required(false),
            ),
    );

    // Add shell completions command
    app = app.subcommand(
        Command::new("completions")
            .about("Generate shell completions")
            .arg(
                Arg::new("shell")
                    .help("Shell to generate completions for")
                    .value_parser(["bash", "zsh", "fish", "powershell", "elvish"])
                    .required(true),
            ),
    );

    // Build resource subcommands from spec
    for resource_name in spec.resource_names() {
        let methods = match spec.methods(resource_name) {
            Some(m) => m,
            None => continue,
        };

        // Use tag description if available (tags are capitalized, resource names are lowercase)
        let tag = &methods[0].tag;
        let resource_desc = spec.tag_descriptions.get(tag).cloned().unwrap_or_default();

        let mut resource_cmd = Command::new(leak(resource_name.to_string()))
            .about(leak(format_about(tag, &resource_desc)))
            .subcommand_required(true)
            .arg_required_else_help(true);

        for method in methods {
            let mut action_cmd =
                Command::new(leak(method.action.clone())).about(leak(method.summary.clone()));

            if !method.description.is_empty() {
                action_cmd = action_cmd.long_about(leak(method.description.clone()));
            }

            // Add --json flag if the method accepts a request body
            if method.has_request_body {
                action_cmd = action_cmd.arg(
                    Arg::new("json")
                        .long("json")
                        .help("Raw JSON request payload")
                        .required(false),
                );
            }

            resource_cmd = resource_cmd.subcommand(action_cmd);
        }

        // Inject helper commands for this resource (if a helper exists)
        if let Some(helper) = crate::helpers::get_helper(resource_name) {
            resource_cmd = helper.inject_commands(resource_cmd);
        }

        app = app.subcommand(resource_cmd);
    }

    app
}

/// Format the about text for a resource command.
fn format_about(tag: &str, description: &str) -> String {
    if description.is_empty() {
        format!("Manage {tag}")
    } else {
        // Truncate long descriptions for the short about text
        let first_sentence = description.split('.').next().unwrap_or(description);
        if first_sentence.len() > 80 {
            format!("{}...", &first_sentence[..77])
        } else {
            first_sentence.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::parse_spec;

    fn test_spec() -> ApiSpec {
        let json = include_str!("../../api/spec3.json");
        parse_spec(json).expect("spec should parse")
    }

    #[test]
    fn cli_builds_without_panic() {
        let spec = test_spec();
        let _cmd = build_cli(&spec);
    }

    #[test]
    fn cli_has_resource_subcommands() {
        let spec = test_spec();
        let cmd = build_cli(&spec);
        let subcmds: Vec<&str> = cmd.get_subcommands().map(|c| c.get_name()).collect();
        assert!(
            subcmds.contains(&"documents"),
            "should have documents command"
        );
        assert!(
            subcmds.contains(&"collections"),
            "should have collections command"
        );
        assert!(subcmds.contains(&"users"), "should have users command");
        assert!(subcmds.contains(&"schema"), "should have schema command");
    }

    #[test]
    fn documents_has_action_subcommands() {
        let spec = test_spec();
        let cmd = build_cli(&spec);
        let docs_cmd = cmd
            .get_subcommands()
            .find(|c| c.get_name() == "documents")
            .expect("documents subcommand should exist");

        let actions: Vec<&str> = docs_cmd.get_subcommands().map(|c| c.get_name()).collect();
        assert!(actions.contains(&"create"), "should have create action");
        assert!(actions.contains(&"list"), "should have list action");
        assert!(actions.contains(&"info"), "should have info action");
        assert!(actions.contains(&"update"), "should have update action");
        assert!(actions.contains(&"delete"), "should have delete action");
    }

    #[test]
    fn action_with_body_has_json_flag() {
        let spec = test_spec();
        let cmd = build_cli(&spec);
        let docs_cmd = cmd
            .get_subcommands()
            .find(|c| c.get_name() == "documents")
            .unwrap();
        let create_cmd = docs_cmd
            .get_subcommands()
            .find(|c| c.get_name() == "create")
            .unwrap();

        let args: Vec<&str> = create_cmd
            .get_arguments()
            .map(|a| a.get_id().as_str())
            .collect();
        assert!(args.contains(&"json"), "create should have --json flag");
    }

    #[test]
    fn all_spec_methods_have_commands() {
        let spec = test_spec();
        let cmd = build_cli(&spec);

        for (resource_name, methods) in &spec.resources {
            let resource_cmd = cmd
                .get_subcommands()
                .find(|c| c.get_name() == resource_name.as_str())
                .unwrap_or_else(|| panic!("Missing resource command: {resource_name}"));

            for method in methods {
                let _action_cmd = resource_cmd
                    .get_subcommands()
                    .find(|c| c.get_name() == method.action.as_str())
                    .unwrap_or_else(|| {
                        panic!(
                            "Missing action command: {}.{}",
                            resource_name, method.action
                        )
                    });
            }
        }
    }

    #[test]
    fn global_flags_present() {
        let spec = test_spec();
        let cmd = build_cli(&spec);
        let arg_names: Vec<&str> = cmd.get_arguments().map(|a| a.get_id().as_str()).collect();
        assert!(arg_names.contains(&"output"), "should have --output flag");
        assert!(arg_names.contains(&"fields"), "should have --fields flag");
        assert!(arg_names.contains(&"dry-run"), "should have --dry-run flag");
        assert!(
            arg_names.contains(&"page-all"),
            "should have --page-all flag"
        );
        assert!(
            arg_names.contains(&"sanitize"),
            "should have --sanitize flag"
        );
    }
}
