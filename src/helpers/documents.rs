//! Document helper commands: +new, +edit, +search
//!
//! These provide human-interactive workflows for common document operations,
//! bridging terminal prompts and $EDITOR to the Outline API.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use anyhow::{Context, Result};
use clap::{Arg, ArgMatches, Command};
use serde_json::json;

use crate::auth::Credentials;
use crate::executor::execute_request;
use crate::output::color;
use crate::spec::ApiSpec;

use super::Helper;

/// Helper for document operations.
pub struct DocumentsHelper;

impl Helper for DocumentsHelper {
    fn inject_commands(&self, cmd: Command) -> Command {
        cmd.subcommand(
            Command::new("+new")
                .about("[Helper] Interactively create a new document")
                .arg(
                    Arg::new("title")
                        .long("title")
                        .short('t')
                        .help("Document title (prompted if not provided)"),
                )
                .arg(
                    Arg::new("collection")
                        .long("collection")
                        .short('c')
                        .help("Collection ID (prompted with picker if not provided)"),
                )
                .arg(
                    Arg::new("no-editor")
                        .long("no-editor")
                        .help("Skip opening $EDITOR (create with empty body)")
                        .action(clap::ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("draft")
                        .long("draft")
                        .help("Create as draft (don't publish immediately)")
                        .action(clap::ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("yes")
                        .long("yes")
                        .short('y')
                        .help("Skip confirmation prompt")
                        .action(clap::ArgAction::SetTrue),
                ),
        )
        .subcommand(
            Command::new("+edit")
                .about("[Helper] Fetch a document, edit in $EDITOR, and update")
                .arg(
                    Arg::new("id")
                        .long("id")
                        .help("Document ID or URL slug")
                        .required(true),
                )
                .arg(
                    Arg::new("yes")
                        .long("yes")
                        .short('y')
                        .help("Skip confirmation prompt")
                        .action(clap::ArgAction::SetTrue),
                ),
        )
        .subcommand(
            Command::new("+search")
                .about("[Helper] Interactive document search")
                .arg(Arg::new("query").help("Search query").required(false))
                .arg(
                    Arg::new("collection")
                        .long("collection")
                        .short('c')
                        .help("Limit search to a specific collection ID"),
                )
                .arg(
                    Arg::new("open")
                        .long("open")
                        .help("Open selected document in browser")
                        .action(clap::ArgAction::SetTrue),
                ),
        )
    }

    fn handle<'a>(
        &'a self,
        matches: &'a ArgMatches,
        credentials: &'a Credentials,
        api_spec: &'a ApiSpec,
        color: bool,
    ) -> Pin<Box<dyn Future<Output = Result<bool>> + Send + 'a>> {
        Box::pin(async move {
            if let Some(sub) = matches.subcommand_matches("+new") {
                handle_new(sub, credentials, api_spec, color).await?;
                return Ok(true);
            }
            if let Some(sub) = matches.subcommand_matches("+edit") {
                handle_edit(sub, credentials, color).await?;
                return Ok(true);
            }
            if let Some(sub) = matches.subcommand_matches("+search") {
                handle_search(sub, credentials, color).await?;
                return Ok(true);
            }
            Ok(false)
        })
    }
}

// ---------------------------------------------------------------------------
// +new — Interactive document creation
// ---------------------------------------------------------------------------

async fn handle_new(
    matches: &ArgMatches,
    credentials: &Credentials,
    _api_spec: &ApiSpec,
    c: bool,
) -> Result<()> {
    let publish = !matches.get_flag("draft");
    let no_editor = matches.get_flag("no-editor");
    let skip_confirm = matches.get_flag("yes");

    // 1. Resolve collection (from flag or interactive picker)
    let collection_id = match matches.get_one::<String>("collection") {
        Some(id) => id.clone(),
        None => pick_collection(credentials).await?,
    };

    // 2. Get title (from flag or prompt)
    let title = match matches.get_one::<String>("title") {
        Some(t) => t.clone(),
        None => prompt_input("Document title")?,
    };

    // 3. Get body content (from $EDITOR or empty)
    let text = if no_editor {
        String::new()
    } else {
        edit_in_editor(&format!("# {title}\n\n"), &title)?
    };

    // 4. Build payload and show summary
    let payload = json!({
        "title": title,
        "collectionId": collection_id,
        "text": text,
        "publish": publish,
    });

    eprintln!("\n{}", color::bold("Document Summary", c));
    eprintln!(
        "  {}      {}",
        color::blue("Title:", c),
        color::bold(&title, c)
    );
    eprintln!(
        "  {} {}",
        color::blue("Collection:", c),
        color::dim(&collection_id, c)
    );
    eprintln!("  {}    {publish}", color::blue("Publish:", c));
    eprintln!("  {}       {} chars", color::blue("Body:", c), text.len());
    eprintln!();

    // 5. Confirm (skip if --yes)
    if !skip_confirm && !confirm("Create this document?")? {
        eprintln!("{}", color::dim("Cancelled.", c));
        return Ok(());
    }

    // 6. Execute
    let response = execute_request(credentials, "/documents.create", Some(&payload)).await?;
    eprintln!();

    if response.is_success() {
        let data = &response.body["data"];
        let doc_title = data["title"].as_str().unwrap_or("(untitled)");
        let doc_url = data["url"].as_str().unwrap_or("");
        let doc_id = data["id"].as_str().unwrap_or("");
        let sym = if c { "\u{2713}" } else { "OK" };
        eprintln!(
            "{} Created: {} {}",
            color::green(sym, c),
            color::bold(doc_title, c),
            color::dim(&format!("({doc_id})"), c),
        );
        if !doc_url.is_empty() {
            let full_url = format!("{}{}", display_base_url(credentials), doc_url);
            eprintln!(
                "  {} {}",
                color::dim("URL:", c),
                color::cyan_underline(&full_url, c),
            );
        }
    } else {
        let msg = response.body["message"]
            .as_str()
            .or_else(|| response.body["error"].as_str())
            .unwrap_or("Unknown error");
        let sym = if c { "\u{2717}" } else { "ERROR" };
        eprintln!("{} {} ({})", color::red_bold(sym, c), msg, response.status);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// +edit — Fetch, edit in $EDITOR, update
// ---------------------------------------------------------------------------

async fn handle_edit(matches: &ArgMatches, credentials: &Credentials, c: bool) -> Result<()> {
    let doc_id = matches.get_one::<String>("id").expect("id is required");
    let skip_confirm = matches.get_flag("yes");

    edit_document_by_id(doc_id, credentials, skip_confirm, c).await
}

// ---------------------------------------------------------------------------
// Shared: fetch a document by ID, edit in $EDITOR, update
// ---------------------------------------------------------------------------

async fn edit_document_by_id(
    doc_id: &str,
    credentials: &Credentials,
    skip_confirm: bool,
    c: bool,
) -> Result<()> {
    // 1. Fetch the document
    eprintln!(
        "{} {}",
        color::dim("Fetching document", c),
        color::dim(doc_id, c),
    );
    let response =
        execute_request(credentials, "/documents.info", Some(&json!({"id": doc_id}))).await?;

    if !response.is_success() {
        let msg = response.body["message"]
            .as_str()
            .or_else(|| response.body["error"].as_str())
            .unwrap_or("Unknown error");
        anyhow::bail!("Failed to fetch document: {} ({})", msg, response.status);
    }

    let data = &response.body["data"];
    let title = data["title"].as_str().unwrap_or("(untitled)");
    let original_text = data["text"].as_str().unwrap_or("");

    eprintln!("{} {}", color::dim("Editing:", c), color::bold(title, c));

    // 2. Open in $EDITOR
    let edited_text = edit_in_editor(original_text, title)?;

    // 3. Check for changes
    if edited_text == original_text {
        eprintln!("{}", color::dim("No changes detected.", c));
        return Ok(());
    }

    // 4. Show diff summary
    let original_lines = original_text.lines().count();
    let edited_lines = edited_text.lines().count();
    let original_len = original_text.len();
    let edited_len = edited_text.len();

    let line_delta = edited_lines as i64 - original_lines as i64;
    let char_delta = edited_len as i64 - original_len as i64;

    eprintln!("\n{}", color::bold("Changes", c));
    eprintln!(
        "  {} {} \u{2192} {} ({})",
        color::blue("Lines:", c),
        original_lines,
        edited_lines,
        format_delta(line_delta, c),
    );
    eprintln!(
        "  {} {} \u{2192} {} ({})",
        color::blue("Chars:", c),
        original_len,
        edited_len,
        format_delta(char_delta, c),
    );
    eprintln!();

    // 5. Confirm (skip if --yes)
    if !skip_confirm && !confirm("Update this document?")? {
        eprintln!("{}", color::dim("Cancelled.", c));
        return Ok(());
    }

    // 6. Execute update
    let payload = json!({
        "id": doc_id,
        "text": edited_text,
    });

    let update_response = execute_request(credentials, "/documents.update", Some(&payload)).await?;
    eprintln!();

    if update_response.is_success() {
        let updated = &update_response.body["data"];
        let updated_title = updated["title"].as_str().unwrap_or(title);
        let updated_url = updated["url"].as_str().unwrap_or("");
        let sym = if c { "\u{2713}" } else { "OK" };
        eprintln!(
            "{} Updated: {}",
            color::green(sym, c),
            color::bold(updated_title, c),
        );
        if !updated_url.is_empty() {
            let full_url = format!("{}{}", display_base_url(credentials), updated_url);
            eprintln!(
                "  {} {}",
                color::dim("URL:", c),
                color::cyan_underline(&full_url, c),
            );
        }
    } else {
        let msg = update_response.body["message"]
            .as_str()
            .or_else(|| update_response.body["error"].as_str())
            .unwrap_or("Unknown error");
        let sym = if c { "\u{2717}" } else { "ERROR" };
        eprintln!(
            "{} {} ({})",
            color::red_bold(sym, c),
            msg,
            update_response.status,
        );
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// +search — Interactive document search with action menu
// ---------------------------------------------------------------------------

async fn handle_search(matches: &ArgMatches, credentials: &Credentials, c: bool) -> Result<()> {
    let open_in_browser = matches.get_flag("open");

    // 1. Get query (from arg or prompt)
    let query = match matches.get_one::<String>("query") {
        Some(q) => q.clone(),
        None => prompt_input("Search query")?,
    };

    // 2. Build search payload
    let mut payload = json!({"query": query});
    if let Some(collection_id) = matches.get_one::<String>("collection") {
        payload["collectionId"] = json!(collection_id);
    }

    // 3. Execute search
    let response = execute_request(credentials, "/documents.search", Some(&payload)).await?;

    if !response.is_success() {
        let msg = response.body["message"]
            .as_str()
            .or_else(|| response.body["error"].as_str())
            .unwrap_or("Unknown error");
        let sym = if c { "\u{2717}" } else { "ERROR" };
        eprintln!(
            "{} Search failed: {} ({})",
            color::red_bold(sym, c),
            msg,
            response.status,
        );
        return Ok(());
    }

    let results = match response.body["data"].as_array() {
        Some(arr) if !arr.is_empty() => arr,
        _ => {
            eprintln!("{} for '{}'.", color::yellow("No results found", c), query,);
            return Ok(());
        }
    };

    // 4. Fetch collection names for display
    let collection_names = fetch_collection_names(credentials).await;

    // 5. Build results list
    let mut items: Vec<SearchItem> = Vec::new();

    for result in results.iter() {
        let doc = &result["document"];
        let title = doc["title"].as_str().unwrap_or("(untitled)");
        let doc_id = doc["id"].as_str().unwrap_or("");
        let url = doc["url"].as_str().unwrap_or("");
        let raw_context = result["context"].as_str().unwrap_or("").trim();
        let context = strip_html_tags(raw_context);
        let updated_at = doc["updatedAt"].as_str().unwrap_or("").to_string();
        let created_at = doc["createdAt"].as_str().unwrap_or("").to_string();
        let revision = doc["revision"].as_u64().unwrap_or(0);
        let coll_id = doc["collectionId"].as_str().unwrap_or("");
        let collection_name = collection_names.get(coll_id).cloned().unwrap_or_default();

        items.push(SearchItem {
            title: title.to_string(),
            id: doc_id.to_string(),
            url: url.to_string(),
            context,
            updated_at,
            created_at,
            revision,
            collection_name,
        });
    }

    // 6. Interactive result picker (always shown)
    eprintln!();
    let selection = pick_search_result(&items, c)?;
    let idx = match selection {
        Some(idx) => idx,
        None => {
            eprintln!("{}", color::dim("No selection made.", c));
            return Ok(());
        }
    };
    let item = &items[idx];

    // 7. --open shortcut: skip action menu, go straight to browser
    if open_in_browser {
        open_in_browser_fn(credentials, item, c);
        return Ok(());
    }

    // 8. Action menu
    eprintln!();
    let actions = &[
        "Edit in $EDITOR",
        "Open in browser",
        "Show details",
        "Cancel",
    ];

    let action = dialoguer::Select::new()
        .with_prompt(format!("Action for \"{}\"", item.title))
        .items(actions)
        .default(0)
        .interact_opt()
        .context("Action selection cancelled")?;

    eprintln!();

    match action {
        Some(0) => {
            // Edit in $EDITOR — reuses the shared edit flow
            edit_document_by_id(&item.id, credentials, false, c).await?;
        }
        Some(1) => {
            // Open in browser
            open_in_browser_fn(credentials, item, c);
        }
        Some(2) => {
            // Show details
            let full_url = format!("{}{}", display_base_url(credentials), item.url);
            eprintln!(
                "  {}    {}",
                color::blue("Title:", c),
                color::bold(&item.title, c),
            );
            eprintln!(
                "  {}       {}",
                color::blue("ID:", c),
                color::dim(&item.id, c),
            );
            eprintln!(
                "  {}      {}",
                color::blue("URL:", c),
                color::cyan_underline(&full_url, c),
            );
            if !item.updated_at.is_empty() {
                eprintln!(
                    "  {}  {}",
                    color::blue("Updated:", c),
                    format_timestamp(&item.updated_at),
                );
            }
            if !item.created_at.is_empty() {
                eprintln!(
                    "  {}  {}",
                    color::blue("Created:", c),
                    format_timestamp(&item.created_at),
                );
            }
            if item.revision > 0 {
                eprintln!("  {} {}", color::blue("Revision:", c), item.revision,);
            }
            if !item.context.is_empty() {
                let preview: String = item
                    .context
                    .chars()
                    .take(200)
                    .collect::<String>()
                    .replace('\n', " ");
                eprintln!(
                    "  {}    {}",
                    color::blue("Match:", c),
                    color::dim(&preview, c),
                );
            }
        }
        _ => {
            // Cancel or Esc
            eprintln!("{}", color::dim("Cancelled.", c));
        }
    }

    Ok(())
}

/// Open a search result in the browser.
fn open_in_browser_fn(credentials: &Credentials, item: &SearchItem, c: bool) {
    let full_url = format!("{}{}", display_base_url(credentials), item.url);
    eprintln!(
        "{} {}",
        color::dim("Opening:", c),
        color::bold(&item.title, c),
    );
    if let Err(e) = open::that(&full_url) {
        let sym = if c { "\u{2717}" } else { "ERROR" };
        eprintln!("{} Failed to open browser: {e}", color::red_bold(sym, c));
        eprintln!(
            "  {} {}",
            color::dim("URL:", c),
            color::cyan_underline(&full_url, c),
        );
    }
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Return the base URL for display purposes.
///
/// When `OUTLINE_DEMO_URL` is set, use it instead of the real instance URL.
/// This allows recording demos without exposing the real server hostname.
fn display_base_url(credentials: &Credentials) -> String {
    std::env::var("OUTLINE_DEMO_URL")
        .unwrap_or_else(|_| credentials.api_url.trim_end_matches("/api").to_string())
}

/// Format a numeric delta with color: green for positive, red for negative, dim for zero.
fn format_delta(delta: i64, c: bool) -> String {
    let text = format!("{delta:+}");
    if delta > 0 {
        color::green(&text, c)
    } else if delta < 0 {
        color::red(&text, c)
    } else {
        color::dim(&text, c)
    }
}

struct SearchItem {
    title: String,
    id: String,
    url: String,
    context: String,
    updated_at: String,
    created_at: String,
    revision: u64,
    collection_name: String,
}

/// Strip HTML tags from a string (e.g. `<b>` / `</b>` in search context).
fn strip_html_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}

/// Format an ISO 8601 timestamp into `YYYY-MM-DD HH:MM`.
fn format_timestamp(iso: &str) -> String {
    // Input: "2026-03-12T17:20:55.730Z"
    // Output: "2026-03-12 17:20"
    if iso.len() >= 16 {
        let date_part = &iso[..10];
        let time_part = &iso[11..16];
        format!("{date_part} {time_part}")
    } else {
        iso.to_string()
    }
}

/// Fetch collections and let the user pick one.
async fn pick_collection(credentials: &Credentials) -> Result<String> {
    let response = execute_request(credentials, "/collections.list", None).await?;

    if !response.is_success() {
        anyhow::bail!("Failed to fetch collections");
    }

    let collections = response.body["data"]
        .as_array()
        .context("Expected collections array")?;

    if collections.is_empty() {
        anyhow::bail!("No collections found. Create one first.");
    }

    let names: Vec<String> = collections
        .iter()
        .map(|c| {
            let name = c["name"].as_str().unwrap_or("(unnamed)");
            let id = c["id"].as_str().unwrap_or("");
            format!("{name}  ({id})")
        })
        .collect();

    let selection = dialoguer::Select::new()
        .with_prompt("Select collection")
        .items(&names)
        .default(0)
        .interact()
        .context("Collection selection cancelled")?;

    let id = collections[selection]["id"]
        .as_str()
        .context("Collection missing ID")?
        .to_string();

    Ok(id)
}

/// Fetch all collections and return an ID → name lookup map.
/// Best-effort: returns an empty map on failure so search still works.
async fn fetch_collection_names(credentials: &Credentials) -> HashMap<String, String> {
    let response = match execute_request(credentials, "/collections.list", None).await {
        Ok(r) if r.is_success() => r,
        _ => return HashMap::new(),
    };
    let mut map = HashMap::new();
    if let Some(collections) = response.body["data"].as_array() {
        for col in collections {
            if let (Some(id), Some(name)) = (col["id"].as_str(), col["name"].as_str()) {
                map.insert(id.to_string(), name.to_string());
            }
        }
    }
    map
}

/// Prompt the user for text input.
fn prompt_input(prompt: &str) -> Result<String> {
    dialoguer::Input::new()
        .with_prompt(prompt)
        .interact_text()
        .context("Input cancelled")
}

/// Ask for yes/no confirmation.
fn confirm(prompt: &str) -> Result<bool> {
    dialoguer::Confirm::new()
        .with_prompt(prompt)
        .default(true)
        .interact()
        .context("Confirmation cancelled")
}

/// Open content in $EDITOR, return the edited content.
fn edit_in_editor(initial_content: &str, title_hint: &str) -> Result<String> {
    // Sanitize title for use in filename
    let safe_title: String = title_hint
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .take(50)
        .collect();

    let suffix = format!(".{safe_title}.md");

    let edited = dialoguer::Editor::new()
        .extension(&suffix)
        .edit(initial_content)
        .context("Editor failed")?;

    match edited {
        Some(text) => Ok(text),
        None => {
            // Editor returned nothing (user saved empty or quit without saving)
            // Return the initial content unchanged
            Ok(initial_content.to_string())
        }
    }
}

/// Let the user pick from search results.
///
/// Each item displays: Title (bold)  Collection (dim)  Date (dim)
fn pick_search_result(items: &[SearchItem], c: bool) -> Result<Option<usize>> {
    if items.is_empty() {
        return Ok(None);
    }

    // Find the longest title and collection name for column alignment
    let max_title = items
        .iter()
        .map(|i| i.title.len().min(40))
        .max()
        .unwrap_or(20);
    let max_coll = items
        .iter()
        .map(|i| i.collection_name.len().min(20))
        .max()
        .unwrap_or(10);

    let display: Vec<String> = items
        .iter()
        .map(|item| {
            // Truncate title to 40 chars
            let title = if item.title.len() > 40 {
                format!("{}...", &item.title[..37])
            } else {
                item.title.clone()
            };

            // Truncate collection name to 20 chars
            let coll = if item.collection_name.len() > 20 {
                format!("{}...", &item.collection_name[..17])
            } else {
                item.collection_name.clone()
            };

            // ISO date: first 10 chars of updated_at
            let date = if item.updated_at.len() >= 10 {
                &item.updated_at[..10]
            } else {
                &item.updated_at
            };

            format!(
                "{}  {}  {}",
                color::bold(&format!("{title:<max_title$}"), c),
                color::dim(&format!("{coll:<max_coll$}"), c),
                color::dim(date, c),
            )
        })
        .collect();

    let selection = dialoguer::Select::new()
        .with_prompt("Select document")
        .items(&display)
        .default(0)
        .interact_opt()
        .context("Selection cancelled")?;

    Ok(selection)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Command;

    #[test]
    fn inject_commands_adds_helpers() {
        let helper = DocumentsHelper;
        let cmd = Command::new("documents").subcommand_required(false);
        let cmd = helper.inject_commands(cmd);

        // Verify +new, +edit, +search subcommands exist
        let sub_names: Vec<&str> = cmd.get_subcommands().map(|s| s.get_name()).collect();
        assert!(sub_names.contains(&"+new"), "Missing +new");
        assert!(sub_names.contains(&"+edit"), "Missing +edit");
        assert!(sub_names.contains(&"+search"), "Missing +search");
    }

    #[test]
    fn new_command_has_expected_args() {
        let helper = DocumentsHelper;
        let cmd = Command::new("documents");
        let cmd = helper.inject_commands(cmd);

        let new_cmd = cmd
            .get_subcommands()
            .find(|s| s.get_name() == "+new")
            .expect("+new should exist");

        let arg_names: Vec<&str> = new_cmd
            .get_arguments()
            .map(|a| a.get_id().as_str())
            .collect();
        assert!(arg_names.contains(&"title"));
        assert!(arg_names.contains(&"collection"));
        assert!(arg_names.contains(&"no-editor"));
        assert!(arg_names.contains(&"draft"));
    }

    #[test]
    fn edit_command_has_id_arg() {
        let helper = DocumentsHelper;
        let cmd = Command::new("documents");
        let cmd = helper.inject_commands(cmd);

        let edit_cmd = cmd
            .get_subcommands()
            .find(|s| s.get_name() == "+edit")
            .expect("+edit should exist");

        let arg_names: Vec<&str> = edit_cmd
            .get_arguments()
            .map(|a| a.get_id().as_str())
            .collect();
        assert!(arg_names.contains(&"id"));
    }

    #[test]
    fn search_command_has_expected_args() {
        let helper = DocumentsHelper;
        let cmd = Command::new("documents");
        let cmd = helper.inject_commands(cmd);

        let search_cmd = cmd
            .get_subcommands()
            .find(|s| s.get_name() == "+search")
            .expect("+search should exist");

        let arg_names: Vec<&str> = search_cmd
            .get_arguments()
            .map(|a| a.get_id().as_str())
            .collect();
        assert!(arg_names.contains(&"query"));
        assert!(arg_names.contains(&"collection"));
        assert!(arg_names.contains(&"open"));
    }
}
