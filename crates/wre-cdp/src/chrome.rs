use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use wre_core::error::{Error, Result, io};

use crate::connection::Connection;
use crate::session::Session;

pub const DEFAULT_PORT: u16 = 9333;

#[derive(Debug, Clone)]
pub struct LaunchOptions {
    pub port: u16,
    pub headless: bool,
    pub profile: PathBuf,
    pub binary: Option<PathBuf>,
    pub window: (u32, u32),
    pub offscreen: bool,
    pub proxy: Option<String>,
    pub extra_flags: Vec<String>,
    pub keep_open: bool,
    pub startup_timeout: Duration,
}

impl Default for LaunchOptions {
    fn default() -> Self {
        Self {
            port: DEFAULT_PORT,
            headless: false,
            profile: std::env::temp_dir().join("wre-chrome"),
            binary: None,
            window: (1512, 982),
            offscreen: true,
            proxy: None,
            extra_flags: Vec::new(),
            keep_open: true,
            startup_timeout: Duration::from_secs(30),
        }
    }
}

impl LaunchOptions {
    pub fn port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    pub fn headless(mut self, headless: bool) -> Self {
        self.headless = headless;
        self
    }

    pub fn profile(mut self, profile: impl Into<PathBuf>) -> Self {
        self.profile = profile.into();
        self
    }

    pub fn proxy(mut self, proxy: Option<String>) -> Self {
        self.proxy = proxy;
        self
    }

    pub fn flag(mut self, flag: impl Into<String>) -> Self {
        self.extra_flags.push(flag.into());
        self
    }
}

pub const STEALTH_FLAGS: [&str; 14] = [
    "--disable-blink-features=AutomationControlled",
    "--no-first-run",
    "--no-default-browser-check",
    "--disable-background-timer-throttling",
    "--disable-backgrounding-occluded-windows",
    "--disable-renderer-backgrounding",
    "--disable-features=Translate,BackForwardCache,AcceptCHFrame,MediaRouter,OptimizationHints",
    "--disable-ipc-flooding-protection",
    "--password-store=basic",
    "--use-mock-keychain",
    "--enable-features=NetworkService,NetworkServiceInProcess",
    "--disable-hang-monitor",
    "--disable-prompt-on-repost",
    "--metrics-recording-only",
];

pub fn find_binary() -> Result<PathBuf> {
    if let Ok(explicit) = std::env::var("WRE_CHROME") {
        let path = PathBuf::from(explicit);
        if path.exists() {
            return Ok(path);
        }
    }

    let candidates: &[&str] = if cfg!(target_os = "macos") {
        &[
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Google Chrome Canary.app/Contents/MacOS/Google Chrome Canary",
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
            "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
        ]
    } else if cfg!(target_os = "windows") {
        &[
            r"C:\Program Files\Google\Chrome\Application\chrome.exe",
            r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
            r"C:\Program Files\Chromium\Application\chrome.exe",
        ]
    } else {
        &[
            "/usr/bin/google-chrome",
            "/usr/bin/google-chrome-stable",
            "/usr/bin/chromium",
            "/usr/bin/chromium-browser",
            "/snap/bin/chromium",
            "/opt/google/chrome/chrome",
        ]
    };

    for candidate in candidates {
        let path = PathBuf::from(candidate);
        if path.exists() {
            return Ok(path);
        }
    }

    Err(Error::msg(
        "no Chrome binary found, set WRE_CHROME to an explicit path",
    ))
}

pub fn free_port(preferred: u16) -> u16 {
    if TcpListener::bind(("127.0.0.1", preferred)).is_ok() {
        return preferred;
    }

    for port in preferred + 1..preferred + 64 {
        if TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return port;
        }
    }

    TcpListener::bind(("127.0.0.1", 0))
        .and_then(|listener| listener.local_addr())
        .map(|address| address.port())
        .unwrap_or(preferred)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserVersion {
    #[serde(default, rename = "Browser")]
    pub browser: String,
    #[serde(default, rename = "Protocol-Version")]
    pub protocol_version: String,
    #[serde(default, rename = "User-Agent")]
    pub user_agent: String,
    #[serde(default, rename = "webSocketDebuggerUrl")]
    pub websocket: String,
}

pub struct Chrome {
    pub port: u16,
    pub version: BrowserVersion,
    pub connection: Connection,
    pub headless: bool,
    pub profile: PathBuf,
    process: Option<Child>,
    keep_open: bool,
}

impl Chrome {
    pub async fn connect_existing(port: u16) -> Result<Self> {
        let version = probe_version(port).await?;
        let connection = Connection::connect(&version.websocket).await?;

        Ok(Self {
            port,
            headless: version.browser.contains("Headless"),
            profile: PathBuf::new(),
            version,
            connection,
            process: None,
            keep_open: true,
        })
    }

    pub async fn launch(options: LaunchOptions) -> Result<Self> {
        if let Ok(existing) = Self::connect_existing(options.port).await {
            tracing::debug!("reusing chrome on port {}", options.port);
            return Ok(existing);
        }

        let binary = match &options.binary {
            Some(path) => path.clone(),
            None => find_binary()?,
        };

        std::fs::create_dir_all(&options.profile).map_err(io(&options.profile))?;

        let mut command = Command::new(&binary);
        command
            .arg(format!("--remote-debugging-port={}", options.port))
            .arg(format!("--user-data-dir={}", options.profile.display()))
            .arg(format!("--window-size={},{}", options.window.0, options.window.1));

        if options.headless {
            command.arg("--headless=new");
        } else if options.offscreen {
            command.arg("--window-position=-3200,-3200");
        }

        if let Some(proxy) = &options.proxy {
            command.arg(format!("--proxy-server={proxy}"));
        }

        for flag in STEALTH_FLAGS {
            command.arg(flag);
        }

        for flag in &options.extra_flags {
            command.arg(flag);
        }

        command.arg("about:blank");

        let process = command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(io(&binary))?;

        let deadline = Instant::now() + options.startup_timeout;
        let version = loop {
            match probe_version(options.port).await {
                Ok(version) => break version,
                Err(error) => {
                    if Instant::now() > deadline {
                        return Err(Error::msg(format!(
                            "chrome did not open a debugging port on {}: {error}",
                            options.port
                        )));
                    }
                    tokio::time::sleep(Duration::from_millis(120)).await;
                }
            }
        };

        let connection = Connection::connect(&version.websocket).await?;

        Ok(Self {
            port: options.port,
            headless: options.headless,
            profile: options.profile,
            version,
            connection,
            process: Some(process),
            keep_open: options.keep_open,
        })
    }

    pub async fn targets(&self) -> Result<Vec<TargetInfo>> {
        let result = self.connection.call("Target.getTargets", Value::Null).await?;
        let infos = result
            .get("targetInfos")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        Ok(infos
            .into_iter()
            .filter_map(|value| serde_json::from_value(value).ok())
            .collect())
    }

    pub async fn attach(&self, target_id: &str) -> Result<Session> {
        let result = self
            .connection
            .call(
                "Target.attachToTarget",
                serde_json::json!({ "targetId": target_id, "flatten": true }),
            )
            .await?;

        let session_id = result
            .get("sessionId")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::msg("attachToTarget returned no sessionId"))?
            .to_string();

        Ok(Session::new(self.connection.clone(), session_id, target_id.to_string()))
    }

    pub async fn new_page(&self, url: &str) -> Result<Session> {
        let result = self
            .connection
            .call("Target.createTarget", serde_json::json!({ "url": url }))
            .await?;

        let target_id = result
            .get("targetId")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::msg("createTarget returned no targetId"))?
            .to_string();

        self.attach(&target_id).await
    }

    pub async fn reuse_page(&self) -> Result<Session> {
        let targets = self.targets().await?;
        let existing = targets
            .iter()
            .find(|target| target.kind == "page" && !target.url.starts_with("devtools://"));

        match existing {
            Some(target) => self.attach(&target.target_id).await,
            None => self.new_page("about:blank").await,
        }
    }

    pub async fn close_target(&self, target_id: &str) -> Result<()> {
        self.connection
            .call(
                "Target.closeTarget",
                serde_json::json!({ "targetId": target_id }),
            )
            .await?;
        Ok(())
    }

    pub async fn shutdown(&mut self) -> Result<()> {
        let _ = self.connection.call("Browser.close", Value::Null).await;
        if let Some(process) = &mut self.process {
            let _ = process.wait();
        }
        self.process = None;
        Ok(())
    }
}

impl Drop for Chrome {
    fn drop(&mut self) {
        if self.keep_open {
            return;
        }
        if let Some(process) = &mut self.process {
            let _ = process.kill();
            let _ = process.wait();
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetInfo {
    #[serde(rename = "targetId")]
    pub target_id: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub attached: bool,
}

pub async fn probe_version(port: u16) -> Result<BrowserVersion> {
    let url = format!("http://127.0.0.1:{port}/json/version");
    let body = fetch_local(&url).await?;
    serde_json::from_str(&body)
        .map_err(|error| Error::msg(format!("chrome version endpoint not understood: {error}")))
}

pub async fn is_running(port: u16) -> bool {
    probe_version(port).await.is_ok()
}

async fn fetch_local(url: &str) -> Result<String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let parsed = url::Url::parse(url).map_err(|error| Error::msg(error.to_string()))?;
    let host = parsed.host_str().unwrap_or("127.0.0.1");
    let port = parsed.port().unwrap_or(80);
    let path = parsed.path();

    let mut stream = tokio::time::timeout(
        Duration::from_millis(1500),
        tokio::net::TcpStream::connect((host, port)),
    )
    .await
    .map_err(|_| Error::msg(format!("connect to {host}:{port} timed out")))?
    .map_err(|error| Error::msg(format!("connect to {host}:{port} failed: {error}")))?;

    let request = format!("GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .await
        .map_err(|error| Error::msg(error.to_string()))?;

    let mut response = Vec::new();
    let mut chunk = [0u8; 4096];

    loop {
        let read = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut chunk))
            .await
            .map_err(|_| Error::msg("chrome http read timed out"))?
            .map_err(|error| Error::msg(error.to_string()))?;

        if read == 0 {
            break;
        }
        response.extend_from_slice(&chunk[..read]);

        if let Some(length) = complete_body_length(&response) {
            if length.0 + length.1 <= response.len() {
                break;
            }
        }
    }

    let text = String::from_utf8_lossy(&response).into_owned();
    let body = text
        .split_once("\r\n\r\n")
        .map(|(_, body)| body.to_string())
        .unwrap_or(text);

    Ok(body)
}

fn complete_body_length(response: &[u8]) -> Option<(usize, usize)> {
    let text = String::from_utf8_lossy(response);
    let (head, _) = text.split_once("\r\n\r\n")?;
    let start = head.len() + 4;

    let length = head
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse::<usize>().ok())?;

    Some((start, length))
}

pub fn profile_dir(root: &Path, port: u16) -> PathBuf {
    root.join(format!("profile-{port}"))
}
