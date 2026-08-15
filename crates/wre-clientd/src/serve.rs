use std::collections::HashMap;
use std::io::{BufReader, BufWriter, Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};

use wre_client::proto::{read_frame, write_frame};

use crate::hub::{Action, Cancels, Hub, Outgoing};

static CONNECTIONS: AtomicU64 = AtomicU64::new(1);

pub fn stdio(hub: &Arc<Hub>) {
    let reader = BufReader::new(std::io::stdin());
    let writer = BufWriter::new(std::io::stdout());
    let _ = run(hub, reader, writer);
}

#[cfg(unix)]
pub fn socket(hub: &Arc<Hub>, path: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::net::UnixListener;

    if path.exists() {
        std::fs::remove_file(path)?;
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let listener = UnixListener::bind(path)?;
    tracing::info!("listening on {}", path.display());

    for incoming in listener.incoming() {
        let stream = match incoming {
            Ok(stream) => stream,
            Err(error) => {
                tracing::warn!("accept failed: {error}");
                continue;
            }
        };

        let writer = match stream.try_clone() {
            Ok(clone) => clone,
            Err(error) => {
                tracing::warn!("connection could not be split: {error}");
                continue;
            }
        };

        let worker_hub = Arc::clone(hub);
        std::thread::spawn(move || {
            let stop = run(&worker_hub, BufReader::new(stream), BufWriter::new(writer));
            if stop {
                worker_hub.stop();
                std::process::exit(0);
            }
        });

        if hub.stopping() {
            break;
        }
    }

    let _ = std::fs::remove_file(path);
    Ok(())
}

#[cfg(not(unix))]
pub fn socket(_hub: &Arc<Hub>, _path: &std::path::Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "socket mode needs a unix platform, use --stdio",
    ))
}

fn run<R, W>(hub: &Arc<Hub>, mut reader: R, mut writer: W) -> bool
where
    R: Read,
    W: Write + Send + 'static,
{
    let connection = CONNECTIONS.fetch_add(1, Ordering::Relaxed);
    let cancels: Cancels = Arc::new(Mutex::new(HashMap::new()));
    let (out, outbox) = channel::<Outgoing>();

    let pump = std::thread::Builder::new()
        .name(format!("wred-writer-{connection}"))
        .spawn(move || {
            while let Ok(message) = outbox.recv() {
                match message {
                    Outgoing::Frame(frame) => {
                        if let Err(error) = write_frame(&mut writer, &frame) {
                            tracing::debug!("write failed, dropping connection: {error}");
                            return;
                        }
                    }
                    Outgoing::Stop => return,
                }
            }
        })
        .expect("writer thread");

    let mut shutdown = false;

    loop {
        let frame = match read_frame(&mut reader) {
            Ok(Some(frame)) => frame,
            Ok(None) => break,
            Err(error) => {
                tracing::debug!("read failed: {error}");
                break;
            }
        };

        let envelope = match frame.envelope() {
            Ok(envelope) => envelope,
            Err(error) => {
                tracing::warn!("frame rejected: {error}");
                continue;
            }
        };

        match hub.handle(connection, envelope, frame.bin, &out, &cancels) {
            Action::Continue => {}
            Action::Shutdown => {
                shutdown = true;
                break;
            }
        }
    }

    hub.close_connection(connection);
    let _ = out.send(Outgoing::Stop);
    let _ = pump.join();

    shutdown
}
