use std::collections::HashMap;
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use serde_json::{Value, json};

use crate::error::{ClientError, ClientResult};
use crate::proto::{
    DiagReply, Envelope, Frame, HealthReply, OpenReply, OpenRequest, ops, read_frame, write_frame,
};
use crate::spec::{BundleDescriptor, Hello, PROTOCOL_VERSION};

pub type EventHandler = Arc<dyn Fn(u64, &str, &Value) + Send + Sync>;

pub struct SidecarOptions {
    pub binary: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub workspace: Option<PathBuf>,
    pub events: Option<EventHandler>,
    pub startup_timeout: Duration,
    pub inherit_stderr: bool,
}

impl SidecarOptions {
    pub fn new(binary: impl Into<PathBuf>) -> Self {
        Self {
            binary: binary.into(),
            args: Vec::new(),
            env: Vec::new(),
            workspace: None,
            events: None,
            startup_timeout: Duration::from_secs(30),
            inherit_stderr: true,
        }
    }

    pub fn discover() -> ClientResult<Self> {
        Ok(Self::new(resolve_binary()?))
    }

    pub fn workspace(mut self, root: impl Into<PathBuf>) -> Self {
        self.workspace = Some(root.into());
        self
    }

    pub fn on_event(mut self, handler: EventHandler) -> Self {
        self.events = Some(handler);
        self
    }

    pub fn arg(mut self, value: impl Into<String>) -> Self {
        self.args.push(value.into());
        self
    }
}

pub fn resolve_binary() -> ClientResult<PathBuf> {
    if let Ok(explicit) = std::env::var("WRE_WRED") {
        let path = PathBuf::from(explicit);
        if path.is_file() {
            return Ok(path);
        }
        return Err(ClientError::resource(format!(
            "WRE_WRED points at {}, which is not a file",
            path.display()
        )));
    }

    let name = if cfg!(windows) { "wred.exe" } else { "wred" };
    let mut roots = Vec::new();

    if let Ok(cwd) = std::env::current_dir() {
        let mut cursor = Some(cwd.as_path());
        while let Some(dir) = cursor {
            roots.push(dir.join("target").join("release").join(name));
            roots.push(dir.join("target").join("debug").join(name));
            cursor = dir.parent();
        }
    }

    for candidate in roots {
        if candidate.is_file() {
            return Ok(candidate);
        }
    }

    if let Ok(path) = std::env::var("PATH") {
        for entry in std::env::split_paths(&path) {
            let candidate = entry.join(name);
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }

    Err(ClientError::resource(
        "no wred binary found, build one with cargo build -p wre-clientd or set WRE_WRED",
    ))
}

type Pending = Arc<Mutex<HashMap<u64, Sender<ClientResult<(Value, Vec<u8>)>>>>>;

pub struct Sidecar {
    child: Mutex<Child>,
    stdin: Mutex<BufWriter<ChildStdin>>,
    pending: Pending,
    next_id: AtomicU64,
    hello: OnceLock<Hello>,
    binary: PathBuf,
}

impl Sidecar {
    pub fn spawn(options: SidecarOptions) -> ClientResult<Arc<Self>> {
        let mut command = Command::new(&options.binary);
        command.arg("--stdio");

        for arg in &options.args {
            command.arg(arg);
        }
        for (name, value) in &options.env {
            command.env(name, value);
        }
        if let Some(root) = &options.workspace {
            command.env("WRE_ROOT", root);
        }

        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(if options.inherit_stderr { Stdio::inherit() } else { Stdio::null() });

        let mut child = command.spawn().map_err(|error| {
            ClientError::resource(format!("{} did not start: {error}", options.binary.display()))
        })?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| ClientError::resource("sidecar stdin was not captured"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ClientError::resource("sidecar stdout was not captured"))?;

        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let reader_pending = Arc::clone(&pending);
        let handler = options.events.clone();

        std::thread::Builder::new()
            .name("wred-reader".to_string())
            .spawn(move || {
                let mut stream = BufReader::new(stdout);
                loop {
                    match read_frame(&mut stream) {
                        Ok(Some(frame)) => route(&frame, &reader_pending, handler.as_ref()),
                        Ok(None) => break,
                        Err(error) => {
                            fail_all(
                                &reader_pending,
                                ClientError::protocol(format!("sidecar stream broke: {error}")),
                            );
                            return;
                        }
                    }
                }
                fail_all(&reader_pending, ClientError::protocol("sidecar closed the stream"));
            })
            .map_err(|error| ClientError::resource(format!("reader thread failed: {error}")))?;

        let sidecar = Arc::new(Self {
            child: Mutex::new(child),
            stdin: Mutex::new(BufWriter::new(stdin)),
            pending,
            next_id: AtomicU64::new(1),
            hello: OnceLock::new(),
            binary: options.binary.clone(),
        });

        let greeting = sidecar.request_with(
            ops::HELLO,
            None,
            json!({}),
            Some(options.startup_timeout),
            Vec::new(),
        )?;

        let hello: Hello = serde_json::from_value(greeting.0)
            .map_err(|error| ClientError::protocol(format!("hello rejected: {error}")))?;

        if hello.protocol != PROTOCOL_VERSION {
            let _ = sidecar.kill();
            return Err(ClientError::protocol(format!(
                "sidecar speaks protocol {} and this build speaks {PROTOCOL_VERSION}",
                hello.protocol
            )));
        }

        let _ = sidecar.hello.set(hello);

        Ok(sidecar)
    }

    pub fn hello(&self) -> Hello {
        self.hello.get().cloned().unwrap_or_else(|| Hello {
            protocol: PROTOCOL_VERSION,
            bundle: String::new(),
            binary_version: String::new(),
            toolkit_version: String::new(),
            schema_hash: String::new(),
            targets: Vec::new(),
            workers: 0,
            pid: 0,
        })
    }

    pub fn binary(&self) -> &Path {
        &self.binary
    }

    pub fn describe(&self) -> ClientResult<BundleDescriptor> {
        let (value, _) = self.request_with(ops::DESCRIBE, None, json!({}), None, Vec::new())?;
        serde_json::from_value(value)
            .map_err(|error| ClientError::protocol(format!("describe rejected: {error}")))
    }

    pub fn targets(&self) -> ClientResult<Vec<String>> {
        let (value, _) = self.request_with(ops::TARGETS, None, json!({}), None, Vec::new())?;
        serde_json::from_value(value)
            .map_err(|error| ClientError::protocol(format!("targets rejected: {error}")))
    }

    pub fn metrics(&self) -> ClientResult<Value> {
        let (value, _) = self.request_with(ops::METRICS, None, json!({}), None, Vec::new())?;
        Ok(value)
    }

    pub fn open(self: &Arc<Self>, target: &str, config: Value) -> ClientResult<Session> {
        self.open_with_diag(target, config, Value::Null)
    }

    pub fn open_with_diag(
        self: &Arc<Self>,
        target: &str,
        config: Value,
        diag: Value,
    ) -> ClientResult<Session> {
        let request = OpenRequest { target: target.to_string(), config, diag };
        let (value, _) = self.request_with(
            ops::OPEN,
            None,
            serde_json::to_value(request).unwrap_or(Value::Null),
            None,
            Vec::new(),
        )?;

        let reply: OpenReply = serde_json::from_value(value)
            .map_err(|error| ClientError::protocol(format!("open rejected: {error}")))?;

        Ok(Session { sidecar: Arc::clone(self), id: reply.session, target: reply.target, ops: reply.ops })
    }

    pub fn shutdown(&self) -> ClientResult<()> {
        let _ = self.request_with(ops::SHUTDOWN, None, json!({}), Some(Duration::from_secs(5)), Vec::new());
        self.kill()
    }

    pub fn kill(&self) -> ClientResult<()> {
        let mut child = self.child.lock().unwrap_or_else(|error| error.into_inner());
        let _ = child.kill();
        let _ = child.wait();
        Ok(())
    }

    pub fn call(
        &self,
        session: Option<&str>,
        op: &str,
        params: Value,
        deadline: Option<Duration>,
    ) -> ClientResult<Value> {
        Ok(self.request_with(op, session, params, deadline, Vec::new())?.0)
    }

    pub fn call_binary(
        &self,
        session: Option<&str>,
        op: &str,
        params: Value,
        bin: Vec<u8>,
        deadline: Option<Duration>,
    ) -> ClientResult<(Value, Vec<u8>)> {
        self.request_with(op, session, params, deadline, bin)
    }

    fn request_with(
        &self,
        op: &str,
        session: Option<&str>,
        params: Value,
        deadline: Option<Duration>,
        bin: Vec<u8>,
    ) -> ClientResult<(Value, Vec<u8>)> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = channel();

        {
            let mut pending = self.pending.lock().unwrap_or_else(|error| error.into_inner());
            pending.insert(id, sender);
        }

        let envelope = Envelope::Req {
            v: PROTOCOL_VERSION,
            id,
            op: op.to_string(),
            session: session.map(str::to_string),
            params,
            deadline_ms: deadline.map(|value| value.as_millis() as u64),
        };

        if let Err(error) = self.send(Frame::from_envelope(&envelope)?.with_bin(bin)) {
            self.forget(id);
            return Err(error);
        }

        let outcome = match deadline {
            Some(limit) => wait(&receiver, limit + Duration::from_secs(5)),
            None => receiver.recv().map_err(|_| ClientError::protocol("sidecar went away")),
        };

        self.forget(id);

        match outcome {
            Ok(inner) => inner,
            Err(error) => {
                if error.kind == crate::error::ErrorKind::Timeout {
                    let _ = self.send(Frame::from_envelope(&Envelope::Cancel {
                        v: PROTOCOL_VERSION,
                        id,
                    })?);
                }
                Err(error.with_op(op))
            }
        }
    }

    fn send(&self, frame: Frame) -> ClientResult<()> {
        let mut stdin = self.stdin.lock().unwrap_or_else(|error| error.into_inner());
        write_frame(&mut *stdin, &frame)
            .map_err(|error| ClientError::protocol(format!("write to sidecar failed: {error}")))
    }

    fn forget(&self, id: u64) {
        let mut pending = self.pending.lock().unwrap_or_else(|error| error.into_inner());
        pending.remove(&id);
    }
}

impl Drop for Sidecar {
    fn drop(&mut self) {
        let mut child = self.child.lock().unwrap_or_else(|error| error.into_inner());
        let _ = child.kill();
        let _ = child.wait();
    }
}

pub struct Session {
    sidecar: Arc<Sidecar>,
    id: String,
    target: String,
    ops: Vec<String>,
}

impl Session {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn target(&self) -> &str {
        &self.target
    }

    pub fn ops(&self) -> &[String] {
        &self.ops
    }

    pub fn call(&self, op: &str, params: Value) -> ClientResult<Value> {
        self.sidecar.call(Some(&self.id), op, params, None)
    }

    pub fn call_within(&self, op: &str, params: Value, deadline: Duration) -> ClientResult<Value> {
        self.sidecar.call(Some(&self.id), op, params, Some(deadline))
    }

    pub fn call_binary(
        &self,
        op: &str,
        params: Value,
        bin: Vec<u8>,
    ) -> ClientResult<(Value, Vec<u8>)> {
        self.sidecar.call_binary(Some(&self.id), op, params, bin, None)
    }

    pub fn warmup(&self) -> ClientResult<Value> {
        self.sidecar.call(Some(&self.id), ops::WARMUP, json!({}), None)
    }

    pub fn health(&self) -> ClientResult<HealthReply> {
        let value = self.sidecar.call(Some(&self.id), ops::HEALTH, json!({}), None)?;
        serde_json::from_value(value)
            .map_err(|error| ClientError::protocol(format!("health rejected: {error}")))
    }

    pub fn diag(&self, write: bool) -> ClientResult<DiagReply> {
        let value = self.sidecar.call(
            Some(&self.id),
            ops::DIAG,
            json!({ "write": write, "events": true }),
            None,
        )?;

        serde_json::from_value(value)
            .map_err(|error| ClientError::protocol(format!("diag rejected: {error}")))
    }

    pub fn close(&self) -> ClientResult<()> {
        self.sidecar.call(Some(&self.id), ops::CLOSE, json!({}), None).map(|_| ())
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.sidecar.call(Some(&self.id), ops::CLOSE, json!({}), None);
    }
}

fn wait(
    receiver: &Receiver<ClientResult<(Value, Vec<u8>)>>,
    limit: Duration,
) -> ClientResult<ClientResult<(Value, Vec<u8>)>> {
    match receiver.recv_timeout(limit) {
        Ok(value) => Ok(value),
        Err(RecvTimeoutError::Timeout) => {
            Err(ClientError::timeout("the sidecar did not answer in time"))
        }
        Err(RecvTimeoutError::Disconnected) => Err(ClientError::protocol("sidecar went away")),
    }
}

fn route(frame: &Frame, pending: &Pending, handler: Option<&EventHandler>) {
    let envelope = match frame.envelope() {
        Ok(value) => value,
        Err(error) => {
            tracing::warn!("sidecar sent a frame that did not parse: {error}");
            return;
        }
    };

    match envelope {
        Envelope::Res { id, ok, result, error, .. } => {
            let sender = {
                let mut map = pending.lock().unwrap_or_else(|inner| inner.into_inner());
                map.remove(&id)
            };

            if let Some(sender) = sender {
                let outcome = if ok {
                    Ok((result, frame.bin.clone()))
                } else {
                    Err(error.unwrap_or_else(|| {
                        ClientError::internal("the sidecar failed without saying why")
                    }))
                };
                let _ = sender.send(outcome);
            }
        }
        Envelope::Evt { id, event, data, .. } => {
            if let Some(handler) = handler {
                handler(id, &event, &data);
            }
        }
        other => tracing::debug!("ignoring {:?} from the sidecar", other),
    }
}

fn fail_all(pending: &Pending, error: ClientError) {
    let mut map = pending.lock().unwrap_or_else(|inner| inner.into_inner());
    for (_, sender) in map.drain() {
        let _ = sender.send(Err(error.clone()));
    }
}
