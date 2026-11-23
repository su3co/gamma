//! `docs/reference.md`: the command line and the MCP tools, generated from
//! their definitions so the page cannot drift from the program. A test
//! compares the file with what this module writes; regenerate it with
//! `GAMMA_UPDATE_REFERENCE=1 cargo test -p gamma-cli reference`.

use clap::{ArgAction, Command, CommandFactory};
use serde_json::Value;
use std::fmt::Write;

fn arg_name(a: &clap::Arg) -> String {
    let value = || {
        a.get_value_names()
            .and_then(|v| v.first())
            .map(|v| v.to_string())
            .unwrap_or_else(|| a.get_id().as_str().to_uppercase())
    };
    if a.is_positional() {
        return format!("<{}>", value());
    }
    let mut s = match (a.get_short(), a.get_long()) {
        (Some(c), Some(l)) => format!("-{c}, --{l}"),
        (None, Some(l)) => format!("--{l}"),
        (Some(c), None) => format!("-{c}"),
        (None, None) => a.get_id().to_string(),
    };
    if !matches!(
        a.get_action(),
        ArgAction::SetTrue | ArgAction::SetFalse | ArgAction::Count
    ) {
        let _ = write!(s, " <{}>", value());
    }
    s
}

fn one_line(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace('|', "\\|")
}

fn command(out: &mut String, cmd: &Command, path: &str, depth: usize) {
    let name = format!("{path} {}", cmd.get_name());
    let subs: Vec<&Command> = cmd
        .get_subcommands()
        .filter(|c| c.get_name() != "help" && !c.is_hide_set())
        .collect();
    let args: Vec<&clap::Arg> = cmd
        .get_arguments()
        .filter(|a| {
            !a.is_global_set()
                && !a.is_hide_set()
                && !matches!(a.get_id().as_str(), "help" | "version")
        })
        .collect();
    let _ = writeln!(out, "{} `{}`\n", "#".repeat(depth + 2), name.trim());
    if let Some(about) = cmd.get_long_about().or(cmd.get_about()) {
        let _ = writeln!(out, "{}\n", about);
    }
    let mut usage = name.trim().to_string();
    if !subs.is_empty() {
        usage.push_str(" <COMMAND>");
    }
    for a in args.iter().filter(|a| a.is_positional()) {
        let n = arg_name(a);
        if a.is_required_set() {
            let _ = write!(usage, " {n}");
        } else {
            let _ = write!(usage, " [{n}]");
        }
    }
    if args.iter().any(|a| !a.is_positional()) {
        usage.push_str(" [OPTIONS]");
    }
    let _ = writeln!(out, "```text\n{usage}\n```\n");
    if !args.is_empty() {
        let _ = writeln!(out, "| Argument | Meaning |\n|---|---|");
        for a in &args {
            let mut help = a
                .get_help()
                .map(|h| one_line(&h.to_string()))
                .unwrap_or_default();
            let defaults: Vec<String> = a
                .get_default_values()
                .iter()
                .map(|v| v.to_string_lossy().to_string())
                .collect();
            if !defaults.is_empty() && !matches!(a.get_action(), ArgAction::SetTrue) {
                let _ = write!(help, " (default: `{}`)", defaults.join(","));
            }
            let _ = writeln!(out, "| `{}` | {} |", arg_name(a), help.trim());
        }
        out.push('\n');
    }
    for s in subs {
        command(out, s, name.trim(), depth + 1);
    }
}

/// The whole page.
pub fn markdown() -> String {
    let mut cli = crate::Cli::command();
    cli.build();
    let mut out = String::new();
    out.push_str(
        "# Command and tool reference\n\n\
         The `gamma` program's commands and the tools of its MCP server, generated \
         from their definitions in `crates/gamma-cli` (do not edit by hand: run \
         `GAMMA_UPDATE_REFERENCE=1 cargo test -p gamma-cli reference`). How to \
         connect an agent: [agents.md](agents.md).\n\n\
         ## Command line\n\n\
         Every command that works on a project takes these options:\n\n\
         | Option | Meaning |\n|---|---|\n",
    );
    for a in cli.get_arguments().filter(|a| a.is_global_set()) {
        let help = a
            .get_help()
            .map(|h| one_line(&h.to_string()))
            .unwrap_or_default();
        let _ = writeln!(out, "| `{}` | {} |", arg_name(a), help);
    }
    out.push('\n');
    for s in cli
        .get_subcommands()
        .filter(|c| c.get_name() != "help" && !c.is_hide_set())
    {
        command(&mut out, s, "gamma", 1);
    }
    let tools = crate::mcp::tool_list();
    let _ = writeln!(
        out,
        "## MCP tools\n\n`gamma mcp` serves {} tools to an agent over standard input and output \
         (JSON-RPC, MCP). Each forwards to the hub, so the person sees the agent's actions as it \
         makes them. Lengths are in µm unless a description says otherwise.\n",
        tools.len()
    );
    for (name, description, schema) in &tools {
        let _ = writeln!(out, "### `{name}`\n\n{description}\n");
        let props = schema.get("properties").and_then(Value::as_object);
        let required: Vec<&str> = schema
            .get("required")
            .and_then(Value::as_array)
            .map(|r| r.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        match props {
            Some(p) if !p.is_empty() => {
                let _ = writeln!(out, "| Parameter | Type | Meaning |\n|---|---|---|");
                for (k, v) in p {
                    let ty = v.get("type").and_then(Value::as_str).unwrap_or("any");
                    let ty = match v
                        .get("items")
                        .and_then(|i| i.get("type"))
                        .and_then(Value::as_str)
                    {
                        Some(it) if ty == "array" => format!("array of {it}"),
                        _ => ty.to_string(),
                    };
                    let mut d = v
                        .get("description")
                        .and_then(Value::as_str)
                        .map(one_line)
                        .unwrap_or_default();
                    if let Some(e) = v.get("enum").and_then(Value::as_array) {
                        let vals: Vec<String> = e
                            .iter()
                            .map(|x| format!("`{}`", x.as_str().unwrap_or_default()))
                            .collect();
                        let _ = write!(d, " ({})", vals.join(", "));
                    }
                    let req = if required.contains(&k.as_str()) {
                        ", required"
                    } else {
                        ""
                    };
                    let _ = writeln!(out, "| `{k}` | {ty}{req} | {} |", d.trim());
                }
                out.push('\n');
            }
            _ => out.push_str("No parameters.\n\n"),
        }
    }
    out.trim_end().to_string() + "\n"
}

#[cfg(test)]
mod tests {
    #[test]
    fn reference_is_current() {
        let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/reference.md");
        let now = super::markdown();
        if std::env::var_os("GAMMA_UPDATE_REFERENCE").is_some() {
            std::fs::write(&file, &now).unwrap();
            return;
        }
        let saved = std::fs::read_to_string(&file).unwrap_or_default();
        assert!(
            saved == now,
            "docs/reference.md is out of date: run GAMMA_UPDATE_REFERENCE=1 cargo test -p gamma-cli reference"
        );
    }
}
