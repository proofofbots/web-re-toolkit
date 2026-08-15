mod hub;
mod registry;
mod serve;

use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;

use wre_client::context::{Counters, MetricSink, Services};
use wre_core::paths::Workspace;

use hub::Hub;

#[derive(Debug, Parser)]
#[command(
    name = "wred",
    version,
    about = "Host process for compiled headless clients",
    long_about = "Runs the headless clients compiled into this bundle and answers the sidecar protocol on stdio or a unix socket."
)]
struct Args {
    #[arg(long, help = "Serve the protocol on stdin and stdout")]
    stdio: bool,

    #[arg(long, help = "Serve the protocol on a unix socket at this path")]
    socket: Option<PathBuf>,

    #[arg(long, help = "Print the bundle descriptor as json and exit")]
    describe: bool,

    #[arg(long, help = "Print the target ids in this bundle and exit")]
    targets: bool,

    #[arg(long, default_value_t = default_workers(), help = "Worker threads that own client sessions")]
    workers: usize,

    #[arg(long, default_value = "info", help = "Log level written to stderr")]
    log: String,

    #[arg(long, help = "Directory for per target client state")]
    state: Option<PathBuf>,

    #[arg(long, help = "Workspace root handed to clients")]
    root: Option<PathBuf>,
}

fn default_workers() -> usize {
    std::thread::available_parallelism()
        .map(|value| value.get().clamp(1, 8))
        .unwrap_or(4)
}

fn main() {
    let args = Args::parse();
    wre_core::init_logging(&args.log);

    if let Err(error) = run(args) {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run(args: Args) -> Result<(), String> {
    let registry = registry::build()?;

    if args.describe {
        let descriptor = hub::descriptor(&registry);
        println!(
            "{}",
            serde_json::to_string_pretty(&descriptor)
                .map_err(|error| format!("descriptor did not serialise: {error}"))?
        );
        return Ok(());
    }

    if args.targets {
        for id in registry.ids() {
            println!("{id}");
        }
        return Ok(());
    }

    if registry.is_empty() {
        return Err(
            "this bundle has no targets compiled in, build with a target feature enabled"
                .to_string(),
        );
    }

    let workspace = args
        .root
        .clone()
        .or_else(|| std::env::var("WRE_ROOT").ok().map(PathBuf::from))
        .or_else(|| Workspace::discover().ok().map(|found| found.root));

    let state = args.state.clone().unwrap_or_else(|| match &workspace {
        Some(root) => root.join("artifacts").join("clients"),
        None => std::env::temp_dir().join("wre-clients"),
    });

    let counters = Arc::new(Counters::default());
    let metrics: Arc<dyn MetricSink> = Arc::clone(&counters) as Arc<dyn MetricSink>;

    let services = Services::new(workspace, state, metrics)
        .map_err(|error| format!("host services did not start: {error}"))?;

    let hub = Hub::new(registry, services, counters, args.workers);

    tracing::info!(
        "wred {} bundle {} targets {} workers {}",
        env!("CARGO_PKG_VERSION"),
        registry::BUNDLE,
        hub.targets().join(","),
        args.workers
    );

    match &args.socket {
        Some(path) => serve::socket(&hub, path).map_err(|error| format!("socket failed: {error}")),
        None => {
            serve::stdio(&hub);
            Ok(())
        }
    }
}
