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
        #[arg(long, help = "Client to emulate, as profile[:platform], for example chrome_141:windows")]
        fingerprint: Option<String>,
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

    #[command(subcommand, about = "The browser surface a realm presents to a target")]
    Sandbox(SandboxCommand),

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

    #[command(about = "Find the target's roles by structure and behaviour rather than by name")]
    Locate {
        input: PathBuf,
        #[arg(long)]
        target: String,
        #[arg(long, help = "parse as an es module rather than a classic script")]
        module: bool,
        #[arg(long, help = "write the resolved bindings to a lock file")]
        lock: Option<PathBuf>,
    },

    #[command(about = "Report which locked roles moved in a newer build")]
    Drift {
        lock: PathBuf,
        input: PathBuf,
        #[arg(long)]
        module: bool,
    },

    #[command(about = "Pair the functions of two builds and say what changed")]
    Builds {
        before: PathBuf,
        after: PathBuf,
        #[arg(long)]
        module: bool,
        #[arg(long, default_value = "0.5", help = "least shared structure to call two functions a pair")]
        threshold: f64,
    },

    #[command(about = "Check, or restore, a script's hash of its own source")]
    Integrity {
        input: PathBuf,
        #[arg(long)]
        target: String,
        #[arg(long, help = "rewrite the stored hash to match the current bytes")]
        resign: bool,
        #[arg(long)]
        out: Option<PathBuf>,
    },

    #[command(about = "Check that a rewrite reaches for nothing the original did not")]
    Equivalent {
        original: PathBuf,
        rewritten: PathBuf,
        #[arg(long)]
        module: bool,
    },

    #[command(about = "Grade a built payload against several real ones")]
    Grade {
        built: PathBuf,
        #[arg(long = "real", required = true, num_args = 2.., help = "two or more real payloads")]
        real: Vec<PathBuf>,
    },

    #[command(about = "Align the slots of one build against another by value")]
    Align {
        #[arg(long = "before", required = true, num_args = 1..)]
        before: Vec<PathBuf>,
        #[arg(long = "after", required = true, num_args = 1..)]
        after: Vec<PathBuf>,
    },

    #[command(about = "Plan the pooled runs that attribute many markers in few loads")]
    Pools {
        #[arg(long, help = "restrict to one marker group")]
        group: Option<String>,
        #[arg(long, help = "one run per marker instead of a pooled design")]
        one_at_a_time: bool,
    },

    #[command(about = "List the built in automation markers")]
    Markers {
        #[arg(long, help = "only the tells a tool leaves, or only the tells hiding one leaves")]
        kind: Option<String>,
        #[arg(long)]
        group: Option<String>,
    },

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
pub enum SandboxCommand {
    #[command(about = "List the captured fingerprint profiles in the workspace")]
    List,

    #[command(about = "Print the browser surface that would be installed")]
    Profile {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        target: Option<String>,
        #[arg(long, conflicts_with = "profile")]
        random: bool,
    },

    #[command(about = "Mount the surface and check it looks like a real browser")]
    Check {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        target: Option<String>,
        #[arg(long, conflicts_with = "profile")]
        random: bool,
        #[arg(long, conflicts_with_all = ["profile", "target", "random"])]
        all: bool,
    },

    #[command(about = "Serve the capture page and store the profile the browser sends back")]
    Capture {
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(long, default_value_t = 8099)]
        port: u16,
        #[arg(long, help = "open the page in a real Chrome and store what it sends back")]
        open: bool,
        #[arg(long, help = "label the profile the browser sends back")]
        label: Option<String>,
        #[arg(long, default_value_t = wre_cdp::chrome::DEFAULT_PORT, help = "debugging port for the browser --open drives")]
        chrome_port: u16,
        #[arg(long)]
        keep: bool,
        #[arg(long)]
        force: bool,
    },

    #[command(about = "Store a profile captured with the page's download button")]
    Import {
        input: PathBuf,
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        force: bool,
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
