use std::process::Command;
use std::time::Instant;

use anyhow::{Context, Result, anyhow, bail};
use serde_json::Value;

use crate::model::{CommandOutput, CommandRun, DeriverStatus, StoreNode};

pub struct NixRunner {
    commands: Vec<CommandRun>,
}

impl NixRunner {
    pub fn new() -> Self {
        Self { commands: Vec::new() }
    }

    pub fn into_commands(self) -> Vec<CommandRun> {
        self.commands
    }

    pub fn nix_version(&mut self) -> Result<String> {
        Ok(self.run_required("nix", &["--version"])?.stdout.trim().to_string())
    }

    pub fn realize(&mut self, root: &str) -> Result<String> {
        if root.starts_with("/nix/store/") {
            return Ok(root.to_string());
        }

        let output = self.run_required("nix", &["build", "--no-link", "--print-out-paths", root])?;
        output
            .stdout
            .lines()
            .map(str::trim)
            .find(|line| line.starts_with("/nix/store/"))
            .map(ToOwned::to_owned)
            .ok_or_else(|| anyhow!("nix build did not print a store output path for {root}"))
    }

    pub fn path_info(&mut self, root: &str, recursive: bool) -> Result<Vec<StoreNode>> {
        let output = if recursive {
            self.run_required("nix", &["path-info", "-r", "--json", "--size", "--closure-size", root])?
        } else {
            self.run_required("nix", &["path-info", "--json", "--size", "--closure-size", root])?
        };
        parse_path_info_json(&output.stdout)
    }

    pub fn query_references(&mut self, path: &str) -> Result<Vec<String>> {
        let output = self.run_required("nix-store", &["-q", "--references", path])?;
        Ok(output
            .stdout
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(ToOwned::to_owned)
            .collect())
    }

    pub fn fill_deriver(&mut self, node: &mut StoreNode) -> Result<()> {
        let output = self.run("nix-store", &["-q", "--deriver", &node.path])?;
        if output.exit_code != 0 {
            node.deriver_status = "unknown-deriver".into();
            node.deriver_path = None;
            return Ok(());
        }

        let deriver = output.stdout.trim();
        if deriver.is_empty() || deriver == "unknown-deriver" {
            node.deriver_status = "unknown-deriver".into();
            node.deriver_path = None;
        } else {
            node.deriver_status = "known".into();
            node.deriver_path = Some(deriver.to_string());
        }
        Ok(())
    }

    pub fn fill_derivers(&mut self, nodes: &mut [StoreNode]) -> Result<()> {
        if nodes.is_empty() {
            return Ok(());
        }

        let mut args = vec!["-q", "--deriver"];
        args.extend(nodes.iter().map(|node| node.path.as_str()));
        let output = self.run("nix-store", &args)?;
        let derivers = output.stdout.lines().map(str::trim).collect::<Vec<_>>();
        if output.exit_code != 0 || derivers.len() != nodes.len() {
            for node in nodes {
                self.fill_deriver(node)?;
            }
            return Ok(());
        }

        for (node, deriver) in nodes.iter_mut().zip(derivers) {
            if deriver.is_empty() || deriver == "unknown-deriver" {
                node.deriver_status = "unknown-deriver".into();
                node.deriver_path = None;
            } else {
                node.deriver_status = "known".into();
                node.deriver_path = Some(deriver.to_string());
            }
        }
        Ok(())
    }

    pub fn why_depends_precise(&mut self, root_path: &str, target: &str) -> Result<CommandOutput> {
        self.run("nix", &["why-depends", "--precise", root_path, target])
    }

    pub fn derivation_show(&mut self, drv_path: &str) -> Result<Value> {
        let output = self.run_required("nix", &["derivation", "show", drv_path])?;
        serde_json::from_str(&output.stdout).with_context(|| format!("failed to parse derivation JSON for {drv_path}"))
    }

    pub fn derivation_show_recursive(&mut self, root: &str) -> Result<Value> {
        let output = self.run_required("nix", &["derivation", "show", "-r", root])?;
        serde_json::from_str(&output.stdout)
            .with_context(|| format!("failed to parse recursive derivation JSON for {root}"))
    }

    pub fn realize_store_path(&mut self, path: &str) -> Result<()> {
        self.run_required("nix-store", &["-r", path])?;
        Ok(())
    }

    pub fn last_command(&self) -> Option<&CommandRun> {
        self.commands.last()
    }

    fn run_required(&mut self, program: &str, args: &[&str]) -> Result<CommandOutput> {
        let output = self.run(program, args)?;
        if output.exit_code != 0 {
            let command = self
                .last_command()
                .map(|command| command.command.as_str())
                .unwrap_or(program);
            bail!(
                "command failed: {command}\nexit code: {}\nstderr: {}",
                output.exit_code,
                output.stderr
            );
        }
        Ok(output)
    }

    fn run(&mut self, program: &str, args: &[&str]) -> Result<CommandOutput> {
        let command_string = command_string(program, args);
        let started = Instant::now();
        let output = Command::new(program)
            .args(args)
            .output()
            .with_context(|| format!("failed to spawn command: {command_string}"))?;
        let duration_ms = started.elapsed().as_millis().min(i64::MAX as u128) as i64;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let exit_code = output.status.code().unwrap_or(-1);

        self.commands.push(CommandRun {
            command: command_string,
            exit_code,
            duration_ms,
            stderr_excerpt: excerpt(&stderr, 4096),
        });

        Ok(CommandOutput {
            stdout,
            stderr,
            exit_code,
        })
    }
}

fn parse_path_info_json(stdout: &str) -> Result<Vec<StoreNode>> {
    let value: Value = serde_json::from_str(stdout).context("failed to parse nix path-info JSON")?;
    let mut nodes = Vec::new();

    match value {
        Value::Array(items) => {
            for item in items {
                nodes.push(parse_path_info_item(None, &item)?);
            }
        }
        Value::Object(entries) => {
            for (path, item) in entries {
                nodes.push(parse_path_info_item(Some(&path), &item)?);
            }
        }
        unexpected => bail!("unexpected nix path-info JSON shape: expected array or object, got {unexpected}"),
    }

    Ok(nodes)
}

fn parse_path_info_item(path_key: Option<&str>, item: &Value) -> Result<StoreNode> {
    if item.is_null() {
        let path = path_key.ok_or_else(|| anyhow!("nix path-info returned a null entry without a path key"))?;
        let (store_hash, name) = split_store_path(path);
        return Ok(StoreNode {
            path: path.to_string(),
            store_hash,
            name,
            nar_size: None,
            closure_size: None,
            deriver_status: "not-queried".into(),
            deriver_path: None,
            references_queried: false,
            references: Vec::new(),
        });
    }

    let object = item
        .as_object()
        .ok_or_else(|| anyhow!("unexpected nix path-info entry: expected object, got {item}"))?;
    let path = object
        .get("path")
        .and_then(Value::as_str)
        .or(path_key)
        .ok_or_else(|| anyhow!("nix path-info entry is missing a store path: {item}"))?;
    let nar_size = object
        .get("narSize")
        .or_else(|| object.get("nar_size"))
        .or_else(|| object.get("size"))
        .and_then(Value::as_i64);
    let closure_size = object
        .get("closureSize")
        .or_else(|| object.get("closure_size"))
        .and_then(Value::as_i64);
    let deriver = object.get("deriver");
    let (deriver_status, deriver_path) = match deriver.and_then(Value::as_str) {
        Some(deriver) if !deriver.is_empty() && deriver != "unknown-deriver" => {
            (DeriverStatus::from("known"), Some(deriver.to_string()))
        }
        Some(_) => (DeriverStatus::from("unknown-deriver"), None),
        None if deriver.is_some() => (DeriverStatus::from("unknown-deriver"), None),
        None => (DeriverStatus::from("not-queried"), None),
    };
    let references_queried = object.contains_key("references");
    let references = object
        .get("references")
        .and_then(Value::as_array)
        .map(|references| {
            references
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let (store_hash, name) = split_store_path(path);

    Ok(StoreNode {
        path: path.to_string(),
        store_hash,
        name,
        nar_size,
        closure_size,
        deriver_status,
        deriver_path,
        references_queried,
        references,
    })
}

fn split_store_path(path: &str) -> (String, String) {
    let Some(rest) = path.strip_prefix("/nix/store/") else {
        return (String::new(), path.to_string());
    };
    match rest.split_once('-') {
        Some((store_hash, name)) => (store_hash.to_string(), name.to_string()),
        None => (rest.to_string(), String::new()),
    }
}

fn command_string(program: &str, args: &[&str]) -> String {
    std::iter::once(program)
        .chain(args.iter().copied())
        .map(quote_arg)
        .collect::<Vec<_>>()
        .join(" ")
}

fn quote_arg(arg: &str) -> String {
    if arg
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || "._+-=:/#".contains(ch))
    {
        arg.to_string()
    } else {
        format!("'{}'", arg.replace('\'', "'\\''"))
    }
}

fn excerpt(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}
