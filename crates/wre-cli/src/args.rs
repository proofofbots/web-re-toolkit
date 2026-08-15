use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "wre",
    version,
    about = "Web reverse engineering toolkit",
    long_about = "Capture a browser run, open the script, borrow its primitives, lift its virtual machine and attribute its signals."
)]
pub struct Cli {
    #[arg(long, global = true, help = "Workspace root, defaults to the nearest wre.toml or .git")]
    pub root: Option<PathBuf>,

    #[arg(long, global = true, help = "Print machine readable json instead of a table")]
    pub json: bool,

    #[arg(long, global = true, default_value = "info", help = "Log level")]
    pub log: String,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    #[command(about = "Write a target manifest to targets/<name>.toml")]
    Init {
        name: String,
        #[arg(long)]
        url: Option<String>,
        #[arg(long)]
        force: bool,
    },

    #[command(about = "List the target manifests in this workspace")]
    Targets,

    #[command(about = "Check a manifest without running anything")]
    Check {
        target: String,
    },

    #[command(about = "Find a target's surface in a document, no browser")]
    Discover {
        url: String,
        #[arg(long)]
        target: Option<String>,
        #[arg(long)]
        proxy: Option<String>,
    },

    #[command(about = "Manage the shared Chrome instance")]
    Browser {
        #[arg(long, default_value_t = wre_cdp::chrome::DEFAULT_PORT)]
        port: u16,
        #[arg(long)]
        status: bool,
        #[arg(long)]
        stop: bool,
        #[arg(long)]
        start: bool,
        #[arg(long)]
        headless: bool,
    },

    #[command(about = "Record a browser run into a capture bundle")]
    Capture {
        #[arg(long)]
        target: Option<String>,
        #[arg(long)]
        url: Option<String>,
        #[arg(long)]
        page: Option<String>,
        #[arg(long, default_value_t = 12)]
        wait: u64,
        #[arg(long)]
        headless: bool,
        #[arg(long, default_value_t = wre_cdp::chrome::DEFAULT_PORT)]
        port: u16,
        #[arg(long)]
        keep_storage: bool,
        #[arg(long)]
        proxy: Option<String>,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        no_probe: bool,
        #[arg(long)]
        scripts: bool,
    },

    #[command(about = "Copy a capture into captures/<name> so it survives an artifacts wipe")]
    Pin {
        #[arg(long)]
        from: Option<PathBuf>,
        name: String,
    },

    #[command(about = "Summarise a capture bundle")]
    Show {
        path: PathBuf,
        #[arg(long)]
        requests: bool,
        #[arg(long)]
        scripts: bool,
        #[arg(long)]
        probes: bool,
    },

    #[command(about = "Run the deobfuscation pipeline over a file")]
    Deobf {
        input: PathBuf,
        #[arg(long)]
        target: Option<String>,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        rename: bool,
        #[arg(long)]
        no_infer: bool,
        #[arg(long)]
        remove_unused: bool,
        #[arg(long)]
        only: Vec<String>,
        #[arg(long)]
        skip: Vec<String>,
        #[arg(long, default_value_t = 8)]
        sweeps: usize,
        #[arg(long)]
        stats: bool,
    },

    #[command(about = "Reformat a file without changing it")]
    Beautify {
        input: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
    },

    #[command(about = "List the passes in the pipeline")]
    Passes,

    #[command(about = "Report the browser surface each function reaches")]
    Surface {
        input: PathBuf,
        #[arg(long)]
        function: Option<String>,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },

    #[command(about = "Find the target's own primitives and call them")]
    Mount {
        input: PathBuf,
        #[arg(long)]
        target: Option<String>,
        #[arg(long)]
        role: Option<String>,
        #[arg(long, default_value = "[]")]
        args: String,
        #[arg(long)]
        eval: Option<String>,
    },

    #[command(subcommand, about = "Headless clients: bundles, binaries and language packages")]
    Client(ClientCommand),

    #[command(subcommand, about = "Custom virtual machine workbench")]
    Vm(VmCommand),

    #[command(subcommand, about = "Payload workbench")]
    Wire(WireCommand),

    #[command(subcommand, about = "Environment snapshots and realms")]
    Env(EnvCommand),

    #[command(subcommand, about = "Transport fingerprints")]
    Tls(TlsCommand),

    #[command(about = "Diff two generated maps, ignoring counter renames")]
    Diff {
        before: PathBuf,
        after: PathBuf,
        #[arg(long)]
        raw: bool,
    },

    #[command(about = "Save a generated map as a baseline")]
    Baseline {
        name: String,
        map: PathBuf,
    },

    #[command(about = "Attribute payload addresses to knobs from recorded captures")]
    Sweep {
        #[arg(long)]
        baseline: Vec<PathBuf>,
        #[arg(long)]
        arm: Vec<String>,
        #[arg(long)]
        pointer: Option<String>,
    },

    #[command(about = "List the built in automation markers")]
    Markers,

    #[command(about = "Run every check that needs no browser")]
    Verify {
        #[arg(long)]
        target: Option<String>,
        #[arg(long)]
        capture: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
pub enum VmCommand {
    #[command(about = "Look for a dispatch loop and a handler table")]
    Discover { input: PathBuf },

    #[command(about = "Probe every handler in a table for its operand and register shape")]
    Probe {
        input: PathBuf,
        #[arg(long)]
        table: String,
        #[arg(long)]
        frame: PathBuf,
        #[arg(long, default_value_t = 0)]
        limit: usize,
        #[arg(long)]
        out: Option<PathBuf>,
    },

    #[command(about = "Print a decoded instruction stream")]
    Listing {
        program: PathBuf,
    },

    #[command(about = "Lift a decoded instruction stream to readable javascript")]
    Lift {
        program: PathBuf,
        #[arg(long)]
        entry: Vec<usize>,
        #[arg(long)]
        dispatch: bool,
        #[arg(long)]
        annotate: bool,
        #[arg(long)]
        out: Option<PathBuf>,
    },

    #[command(about = "Report basic blocks, loops and reducibility")]
    Cfg {
        program: PathBuf,
        #[arg(long, default_value_t = 0)]
        entry: usize,
    },

    #[command(about = "Align a recorded trace to handler identities")]
    Align {
        trace: PathBuf,
        #[arg(long)]
        against: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
pub enum WireCommand {
    #[command(about = "Decode a body")]
    Open {
        input: PathBuf,
        #[arg(long, default_value = "json")]
        codec: String,
        #[arg(long)]
        key: Option<String>,
        #[arg(long)]
        out: Option<PathBuf>,
    },

    #[command(about = "Encode a value")]
    Seal {
        input: PathBuf,
        #[arg(long, default_value = "json")]
        codec: String,
        #[arg(long)]
        key: Option<String>,
        #[arg(long)]
        out: Option<PathBuf>,
    },

    #[command(about = "Check that a body decodes and re-encodes byte for byte")]
    Roundtrip {
        input: PathBuf,
        #[arg(long, default_value = "json")]
        codec: String,
        #[arg(long)]
        key: Option<String>,
    },

    #[command(about = "Diff two payloads by address")]
    Diff { left: PathBuf, right: PathBuf },

    #[command(about = "Build a payload from a donor with fields replaced")]
    Forge {
        donor: PathBuf,
        #[arg(long)]
        set: Vec<String>,
        #[arg(long)]
        out: Option<PathBuf>,
    },

    #[command(about = "Infer a schema across several payloads")]
    Schema {
        inputs: Vec<PathBuf>,
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
pub enum EnvCommand {
    #[command(about = "Print the script that captures an environment snapshot")]
    Script {
        #[arg(long, default_value_t = 4)]
        depth: usize,
    },

    #[command(about = "Capture a snapshot from a live page")]
    Snapshot {
        url: String,
        #[arg(long, default_value_t = wre_cdp::chrome::DEFAULT_PORT)]
        port: u16,
        #[arg(long)]
        headless: bool,
        #[arg(long, default_value_t = 4)]
        depth: usize,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long, default_value_t = 8)]
        wait: u64,
    },

    #[command(about = "Run a script inside a realm materialised from a snapshot")]
    Run {
        script: PathBuf,
        #[arg(long)]
        snapshot: Option<PathBuf>,
        #[arg(long)]
        expression: Option<String>,
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },
}

#[derive(Debug, Subcommand)]
pub enum TlsCommand {
    #[command(about = "Compute JA3 and JA4 from a raw ClientHello")]
    Hello {
        input: PathBuf,
        #[arg(long)]
        hex: bool,
    },

    #[command(about = "Compute the HTTP/2 settings fingerprint from a raw frame stream")]
    H2 {
        input: PathBuf,
        #[arg(long)]
        hex: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum ClientCommand {
    #[command(about = "Scaffold a client crate under clients/<id> and wire it into wred")]
    New {
        id: String,
        #[arg(long)]
        summary: Option<String>,
        #[arg(long)]
        force: bool,
    },

    #[command(about = "List the bundles declared in clients.toml")]
    Bundles,

    #[command(about = "List the targets compiled into a wred binary")]
    List {
        #[arg(long)]
        bin: Option<PathBuf>,
    },

    #[command(about = "Print the ops, events and capabilities of a target")]
    Describe {
        target: Option<String>,
        #[arg(long)]
        bin: Option<PathBuf>,
    },

    #[command(about = "Write the bundle descriptor the generators read")]
    Schema {
        #[arg(long)]
        bin: Option<PathBuf>,
        #[arg(long)]
        out: Option<PathBuf>,
    },

    #[command(about = "Cross build wred for a bundle into dist/<bundle>/bin")]
    Build {
        #[arg(long, default_value = "default")]
        bundle: String,
        #[arg(long)]
        platform: Vec<String>,
        #[arg(long, help = "Ad hoc codesign the apple binaries, needed for arm64")]
        sign: bool,
        #[arg(long, help = "Use cargo-zigbuild instead of cargo")]
        zig: bool,
        #[arg(long)]
        debug: bool,
    },

    #[command(about = "Generate the node, python, go and rust packages")]
    Package {
        #[arg(long, default_value = "default")]
        bundle: String,
        #[arg(long)]
        lang: Vec<String>,
        #[arg(long)]
        version: Option<String>,
        #[arg(long)]
        bin: Option<PathBuf>,
        #[arg(long)]
        out: Option<PathBuf>,
    },

    #[command(about = "Run a conformance suite against one or more bindings")]
    Test {
        target: Option<String>,
        #[arg(long)]
        bin: Option<PathBuf>,
        #[arg(long)]
        suite: Option<PathBuf>,
        #[arg(long)]
        lang: Vec<String>,
    },

    #[command(about = "Print the commands that publish a bundle's packages")]
    Publish {
        #[arg(long, default_value = "default")]
        bundle: String,
        #[arg(long)]
        lang: Vec<String>,
    },

    #[command(about = "Summarise a diagnostics report")]
    Diag {
        path: PathBuf,
    },
}
