use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use url::Url;

use wre_core::error::{Error, Result};

static NEXT: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    pub secure: bool,
    pub http_only: bool,
}

#[derive(Clone)]
pub struct Jar {
    id: String,
    store: Arc<wreq::cookie::Jar>,
}

impl std::fmt::Debug for Jar {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Jar")
            .field("id", &self.id)
            .field("cookies", &self.all().len())
            .finish()
    }
}

impl Default for Jar {
    fn default() -> Self {
        Self::new()
    }
}

impl Jar {
    pub fn new() -> Self {
        let id = format!("jar-{}", NEXT.fetch_add(1, Ordering::Relaxed));
        Self { id, store: Arc::new(wreq::cookie::Jar::default()) }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn store(&self) -> Arc<wreq::cookie::Jar> {
        Arc::clone(&self.store)
    }

    pub fn add(&self, url: &str, line: &str) -> Result<()> {
        let parsed = Url::parse(url).map_err(|error| Error::msg(format!("{url}: {error}")))?;
        self.store.add(line, parsed.as_str());
        Ok(())
    }

    pub fn add_all(&self, url: &str, lines: &[&str]) -> Result<()> {
        for line in lines {
            self.add(url, line)?;
        }
        Ok(())
    }

    pub fn all(&self) -> Vec<Cookie> {
        self.store
            .get_all()
            .map(|cookie| Cookie {
                name: cookie.name().to_string(),
                value: cookie.value().to_string(),
                domain: cookie.domain().unwrap_or_default().to_string(),
                path: cookie.path().unwrap_or("/").to_string(),
                secure: cookie.secure(),
                http_only: cookie.http_only(),
            })
            .collect()
    }

    pub fn matching(&self, url: &str) -> Vec<Cookie> {
        let Ok(parsed) = Url::parse(url) else {
            return Vec::new();
        };

        let host = parsed.host_str().unwrap_or_default().trim_start_matches('.').to_lowercase();
        let path = parsed.path();
        let secure = parsed.scheme() == "https";

        let mut out: Vec<Cookie> = self
            .all()
            .into_iter()
            .filter(|cookie| domain_matches(&host, &cookie.domain))
            .filter(|cookie| path_matches(path, &cookie.path))
            .filter(|cookie| secure || !cookie.secure)
            .collect();

        out.sort_by(|first, second| {
            second
                .path
                .len()
                .cmp(&first.path.len())
                .then_with(|| first.name.cmp(&second.name))
        });

        out
    }

    pub fn header(&self, url: &str) -> String {
        render(&self.matching(url))
    }

    pub fn script_header(&self, url: &str) -> String {
        let readable: Vec<Cookie> = self
            .matching(url)
            .into_iter()
            .filter(|cookie| !cookie.http_only)
            .collect();

        render(&readable)
    }

    pub fn get(&self, url: &str, name: &str) -> Option<String> {
        self.matching(url)
            .into_iter()
            .find(|cookie| cookie.name == name)
            .map(|cookie| cookie.value)
    }

    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.all().into_iter().map(|cookie| cookie.name).collect();
        names.sort();
        names.dedup();
        names
    }

    pub fn is_empty(&self) -> bool {
        self.all().is_empty()
    }

    pub fn clear(&self) {
        self.store.clear();
    }
}

fn render(cookies: &[Cookie]) -> String {
    cookies
        .iter()
        .map(|cookie| format!("{}={}", cookie.name, cookie.value))
        .collect::<Vec<_>>()
        .join("; ")
}

fn domain_matches(host: &str, domain: &str) -> bool {
    let domain = domain.trim_start_matches('.').to_lowercase();

    if domain.is_empty() || host == domain {
        return true;
    }

    host.ends_with(&format!(".{domain}"))
}

fn path_matches(request: &str, cookie: &str) -> bool {
    if cookie.is_empty() || cookie == "/" || request == cookie {
        return true;
    }

    if !request.starts_with(cookie) {
        return false;
    }

    cookie.ends_with('/') || request.as_bytes().get(cookie.len()) == Some(&b'/')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cookie_set_on_the_page_comes_back_for_that_host() {
        let jar = Jar::new();
        jar.add("https://www.example.com/tracking", "_abck=abc~-1~x; Path=/; Domain=.example.com")
            .unwrap();

        assert_eq!(jar.get("https://www.example.com/", "_abck").as_deref(), Some("abc~-1~x"));
        assert_eq!(jar.header("https://www.example.com/"), "_abck=abc~-1~x");
        assert!(jar.get("https://other.test/", "_abck").is_none());
    }

    #[test]
    fn http_only_cookies_stay_out_of_the_script_view() {
        let jar = Jar::new();
        jar.add("https://www.example.com/", "ak_bmsc=secret; Path=/; HttpOnly").unwrap();
        jar.add("https://www.example.com/", "bm_sz=readable; Path=/").unwrap();

        assert_eq!(jar.script_header("https://www.example.com/"), "bm_sz=readable");
        assert!(jar.header("https://www.example.com/").contains("ak_bmsc=secret"));
    }

    #[test]
    fn a_deeper_path_wins_the_order_a_browser_sends() {
        let jar = Jar::new();
        jar.add("https://www.example.com/", "one=root; Path=/").unwrap();
        jar.add("https://www.example.com/deep/page", "two=deep; Path=/deep").unwrap();

        assert_eq!(jar.header("https://www.example.com/deep/page"), "two=deep; one=root");
        assert_eq!(jar.header("https://www.example.com/"), "one=root");
    }

    #[test]
    fn a_secure_cookie_is_held_back_from_plain_http() {
        let jar = Jar::new();
        jar.add("https://www.example.com/", "bm_sv=value; Path=/; Secure").unwrap();

        assert!(jar.header("http://www.example.com/").is_empty());
        assert_eq!(jar.header("https://www.example.com/"), "bm_sv=value");
    }
}
