//! The command line: every `keymule` command and its options.

use clap::{Parser, Subcommand};

use crate::{hook, ui};

/// Let AI agents use your tokens and passwords without ever seeing them.
#[derive(Parser)]
#[command(version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Subcommand)]
pub enum Cmd {
    /// Store a new credential (the secret is typed into a hidden prompt).
    Add {
        /// Service preset: bamboo, github, gitlab, jenkins, generic.
        service: Option<String>,
        #[arg(long)]
        name: Option<String>,
        /// Base URL of the API.
        #[arg(long)]
        url: Option<String>,
        /// Username, for services that use basic auth.
        #[arg(long)]
        username: Option<String>,
        /// Send the secret in this header instead of `Authorization: Bearer`.
        #[arg(long)]
        header: Option<String>,
        #[arg(long)]
        description: Option<String>,
        /// Skip checking the token against the service.
        #[arg(long)]
        no_verify: bool,
        /// Ask for the secret in a macOS dialog instead of the terminal.
        #[arg(long)]
        dialog: bool,
    },
    /// List stored credentials (never shows secrets).
    List {
        /// Machine-readable output.
        #[arg(long)]
        json: bool,
    },
    /// Give a credential a new name (and, optionally, a new description).
    Rename {
        name: String,
        new_name: String,
        #[arg(long)]
        description: Option<String>,
    },
    /// Change a credential's URL, username, service or description.
    Edit {
        name: String,
        #[arg(long)]
        url: Option<String>,
        #[arg(long)]
        username: Option<String>,
        #[arg(long)]
        service: Option<String>,
        #[arg(long)]
        description: Option<String>,
    },
    /// Replace a credential's secret, e.g. after rotating it.
    Rotate {
        name: String,
        #[arg(long)]
        no_verify: bool,
        /// Ask for the new secret in a macOS dialog instead of the terminal.
        #[arg(long)]
        dialog: bool,
    },
    /// Show a stored secret in a macOS window. It is never printed, so an
    /// agent that runs this can't read it.
    Reveal { name: String },
    /// Check that a stored credential still works.
    Test { name: String },
    /// Delete a credential and its secret.
    Rm {
        name: String,
        /// Confirm in a macOS dialog instead of the terminal.
        #[arg(long)]
        dialog: bool,
    },
    /// Show the built-in service presets.
    Presets,
    /// Move secrets an older keymule kept as separate keychain items into
    /// keymule's encrypted vault (it also happens by itself as they're used).
    Migrate,
    /// Find secrets that leaked to AI models or sit in plain text where agents can read them.
    Doctor {
        /// Also list unsure matches (secrets without a known format).
        #[arg(long)]
        all: bool,
        /// Machine-readable output (secrets are still masked).
        #[arg(long)]
        json: bool,
        /// Project directory to check for .env and .mcp.json files (default: current directory).
        #[arg(long)]
        dir: Option<std::path::PathBuf>,
    },
    /// Move secrets that MCP server configs keep in plain text into the keychain.
    Adopt {
        /// Show what would change without changing anything.
        #[arg(long)]
        dry_run: bool,
        /// Don't ask for confirmation.
        #[arg(long)]
        yes: bool,
        /// Also adopt this MCP server even if its secrets look like defaults,
        /// e.g. a local database password (repeatable).
        #[arg(long = "server", value_name = "NAME")]
        servers: Vec<String>,
    },
    /// Run a command with a secret: an adopted MCP server (used in agent
    /// configs), or a CLI tool with a stored credential in an env var
    /// (`keymule exec github --env GH_TOKEN -- gh pr create …`).
    Exec {
        name: String,
        /// The env var to put the secret in (a credential: defaults to the
        /// service's, e.g. GH_TOKEN; an MCP server: renames its one value).
        #[arg(long = "env", value_name = "VAR")]
        rename: Vec<String>,
        /// Hand the secret over as a file instead (a private key): VAR gets
        /// the file's path, and so does `{VAR}` in the command. The file is
        /// only readable by you and removed when the command ends.
        #[arg(long = "file", value_name = "VAR")]
        files: Vec<String>,
        /// The command to run.
        #[arg(last = true, required = true)]
        command: Vec<String>,
    },
    /// Copy a credential into git's credential store so git uses it for
    /// https://<host> as <username> (asks you first).
    Install {
        name: String,
        #[arg(long)]
        host: String,
        #[arg(long)]
        username: String,
        /// The repo folder: the service's CLI (gh) may use it there too,
        /// approved in the same dialog (default: this folder).
        #[arg(long)]
        dir: Option<std::path::PathBuf>,
    },
    /// Copy credentials into a repo's GitHub Actions secrets, encrypted with
    /// the repo's key. One dialog for all of them. Uploads with the account
    /// gh is signed in as, or a GitHub credential in keymule, whichever has
    /// admin on the repo.
    ///
    /// keymule gh-secret --repo acme/shop STRIPE_SECRET_KEY=stripe-acme
    /// keymule gh-secret --repo acme/ext CWS_CLIENT_ID=cws-id CWS_CLIENT_SECRET=cws-secret
    #[command(verbatim_doc_comment)]
    GhSecret {
        /// SECRET=credential, one or more (or one credential with --as).
        #[arg(required = true, value_name = "SECRET=CREDENTIAL")]
        items: Vec<String>,
        /// owner/repo
        #[arg(long)]
        repo: String,
        /// The Actions secret, when copying one credential given by name.
        #[arg(long = "as", value_name = "SECRET")]
        secret: Option<String>,
        /// Who uploads: `gh` (gh's active account), `gh:<login>`, or a
        /// GitHub credential's name. Default: the first with admin on the repo.
        #[arg(long)]
        via: Option<String>,
        /// Only say who would upload and what would change; ask nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Allow all: one dialog, then keymule stops asking in this project
    /// (its git repo, or this folder). Deleting still asks.
    Allow {
        /// The folder (default: this folder's git repo).
        #[arg(long)]
        dir: Option<std::path::PathBuf>,
        /// Turn it off here (with --dir, there; with --everywhere, all).
        #[arg(long, conflicts_with = "status")]
        off: bool,
        /// With --off: turn it off in every folder.
        #[arg(long, requires = "off")]
        everywhere: bool,
        /// Say where it's on; change nothing.
        #[arg(long)]
        status: bool,
    },
    /// Remove a credential's copies from git's credential store (keeps it
    /// in keymule).
    Uninstall {
        name: String,
        #[arg(long)]
        host: Option<String>,
        #[arg(long)]
        username: Option<String>,
    },
    /// Connect to a remote (HTTP) MCP server, adding the headers keymule
    /// holds (used in agent configs, as a stdio server).
    Remote {
        name: String,
        /// The server's URL, exactly as approved.
        url: String,
        /// An extra header that isn't secret, "Name: value" (repeatable).
        #[arg(long = "header", value_name = "HEADER")]
        headers: Vec<String>,
    },
    /// Open keymule's local web page: credentials, MCP servers, activity, leaks.
    Ui {
        /// Print the address instead of opening a browser.
        #[arg(long)]
        no_open: bool,
        /// Port on 127.0.0.1 (another free one if it's taken).
        #[arg(long, default_value_t = ui::DEFAULT_PORT)]
        port: u16,
    },
    /// keymule in the macOS menu bar: its page in its own window, no browser.
    Menubar {
        /// Start without opening the window (how it starts at login).
        #[arg(long)]
        background: bool,
        /// Start it at login, or stop that, then exit.
        #[arg(long, value_name = "on|off", value_parser = ["on", "off"])]
        login: Option<String>,
    },
    /// Set keymule up in every agent found: Claude Code, Claude Desktop,
    /// Codex, Cursor, Antigravity (MCP server, paste hook, instructions).
    Init {
        #[arg(long)]
        dry_run: bool,
    },
    /// Run the MCP server on stdio (started by your agent).
    Mcp,
    /// Agent hooks.
    Hook {
        #[command(subcommand)]
        kind: HookKind,
    },
}

#[derive(Subcommand)]
pub enum HookKind {
    /// The prompt hook (Claude Code, Codex, Cursor): hold a prompt with a
    /// secret and ask.
    Prompt {
        /// Which agent runs it.
        #[arg(long, value_enum, default_value_t)]
        agent: hook::Agent,
    },
    /// Before an agent runs a shell command: block reading tokens back out
    /// of git's, gh's or the keychain's stores.
    Bash {
        /// Which agent runs it.
        #[arg(long, value_enum, default_value_t)]
        agent: hook::Agent,
    },
    /// For the Claude Code mod: find secrets in the prompt on stdin (output
    /// is masked).
    Scan,
    /// For the Claude Code mod: hold the secrets in the prompt on stdin until
    /// the user names them through the agent; print the prompt without them.
    Hold,
    /// For the Claude Code mod: save the secrets in the prompt on stdin and
    /// print it with each swapped for its keymule name.
    Save {
        #[arg(long)]
        name: Option<String>,
    },
}
