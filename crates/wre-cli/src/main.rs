mod args;
mod capture_cmd;
mod client_cmd;
mod env_cmd;
mod ident_cmd;
mod js_cmd;
mod misc_cmd;
mod sandbox_cmd;
mod target_cmd;
mod vm_cmd;
mod wire_cmd;

use clap::Parser;

use wre_core::error::Result;
use wre_core::paths::Workspace;

use args::{Cli, Command};

pub struct Context {
    pub workspace: Workspace,
    pub json: bool,
}

impl Context {
    pub fn emit(&self, value: &serde_json::Value, plain: &str) {
        if self.json {
            println!(
                "{}",
                serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
            );
        } else {
            print!("{plain}");
            if !plain.ends_with('\n') {
                println!();
            }
        }
    }

    pub fn note(&self, text: &str) {
        if !self.json {
            println!("{text}");
        }
    }
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    wre_core::init_logging(&cli.log);

    let workspace = match &cli.root {
        Some(root) => Workspace::at(root.clone()),
        None => match Workspace::discover() {
            Ok(workspace) => workspace,
            Err(_) => Workspace::at(std::env::current_dir().unwrap_or_default()),
        },
    };

    let context = Context { workspace, json: cli.json };

    if let Err(error) = run(&context, cli.command).await {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

async fn run(context: &Context, command: Command) -> Result<()> {
    match command {
        Command::Init { name, url, force } => target_cmd::init(context, &name, url, force),
        Command::Targets => target_cmd::list(context),
        Command::Check { target } => target_cmd::check(context, &target),
        Command::Discover { url, target, proxy, fingerprint } => {
            target_cmd::discover(context, &url, target, proxy, fingerprint).await
        }

        Command::Browser { port, status, stop, start, headless } => {
            capture_cmd::browser(context, port, status, stop, start, headless).await
        }
        Command::Capture {
            target,
            url,
            page,
            wait,
            headless,
            port,
            keep_storage,
            proxy,
            out,
            no_probe,
            scripts,
        } => {
            capture_cmd::capture(
                context,
                capture_cmd::CaptureArgs {
                    target,
                    url,
                    page,
                    wait,
                    headless,
                    port,
                    keep_storage,
                    proxy,
                    out,
                    no_probe,
                    scripts,
                },
            )
            .await
        }
        Command::Pin { from, name } => capture_cmd::pin(context, from, &name),
        Command::Show { path, requests, scripts, probes } => {
            capture_cmd::show(context, &path, requests, scripts, probes)
        }

        Command::Deobf {
            input,
            target,
            out,
            rename,
            no_infer,
            remove_unused,
            only,
            skip,
            sweeps,
            stats,
        } => js_cmd::deobf(
            context,
            js_cmd::DeobfArgs {
                input,
                target,
                out,
                rename,
                no_infer,
                remove_unused,
                only,
                skip,
                sweeps,
                stats,
            },
        ),
        Command::Beautify { input, out } => js_cmd::beautify(context, &input, out),
        Command::Passes => js_cmd::passes(context),
        Command::Surface { input, function, limit } => {
            js_cmd::surface(context, &input, function, limit)
        }
        Command::Mount { input, target, role, args, eval } => {
            js_cmd::mount(context, &input, target, role, &args, eval)
        }

        Command::Client(command) => client_cmd::run(context, command),
        Command::Vm(command) => vm_cmd::run(context, command),
        Command::Wire(command) => wire_cmd::run(context, command),
        Command::Env(command) => env_cmd::run(context, command).await,
        Command::Tls(command) => misc_cmd::tls(context, command),

        Command::Diff { before, after, raw } => misc_cmd::diff(context, &before, &after, raw),
        Command::Baseline { name, map } => misc_cmd::baseline(context, &name, &map),
        Command::Sweep { baseline, arm, pointer } => {
            misc_cmd::sweep(context, &baseline, &arm, pointer)
        }
        Command::Sandbox(command) => sandbox_cmd::run(context, command),
        Command::Markers { kind, group } => misc_cmd::markers(context, kind, group),

        Command::Locate { input, target, module, lock } => {
            ident_cmd::locate(context, &input, &target, module, lock)
        }
        Command::Drift { lock, input, module } => ident_cmd::drift(context, &lock, &input, module),
        Command::Builds { before, after, module, threshold } => {
            ident_cmd::builds(context, &before, &after, module, threshold)
        }
        Command::Integrity { input, target, resign, out } => {
            ident_cmd::integrity(context, &input, &target, resign, out)
        }
        Command::Equivalent { original, rewritten, module } => {
            ident_cmd::equivalent(context, &original, &rewritten, module)
        }
        Command::Grade { built, real } => ident_cmd::grade(context, &real, &built),
        Command::Align { before, after } => ident_cmd::align(context, &before, &after),
        Command::Pools { group, one_at_a_time } => misc_cmd::pools(context, group, one_at_a_time),
        Command::Verify { target, capture } => misc_cmd::verify(context, target, capture),
    }
}

pub fn read_text(path: &std::path::Path) -> Result<String> {
    std::fs::read_to_string(path).map_err(wre_core::error::io(path))
}

pub fn read_bytes(path: &std::path::Path) -> Result<Vec<u8>> {
    std::fs::read(path).map_err(wre_core::error::io(path))
}

pub fn write_text(path: &std::path::Path, text: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(wre_core::error::io(parent))?;
    }
    std::fs::write(path, text).map_err(wre_core::error::io(path))
}

pub fn read_json(path: &std::path::Path) -> Result<serde_json::Value> {
    let text = read_text(path)?;
    serde_json::from_str(&text).map_err(wre_core::error::json(path))
}
