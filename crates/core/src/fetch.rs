use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

use crate::private_fs::{create_private, ensure_private_dir, write_private};

const MAX_FETCH_BYTES: u64 = 32 << 20;
const MAX_REQUEST_BYTES: usize = 8 << 20;
const DEFAULT_MAX_TIME: Duration = Duration::from_secs(60);

// Shelling out to curl avoids a TLS stack and a large dependency tree; every
// target distro ships it. Replace when the app needs connection reuse.
pub fn cache_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("coruframe")
}

// A body plus whatever the response said about throttling. `throttled` is set
// when a 429/509 was answered from the cache; `retry_after` is its requested
// pause, when the server sent one.
#[derive(Debug)]
pub struct Fetched {
    pub body: String,
    pub throttled: bool,
    pub retry_after: Option<u64>,
}

pub fn fetch(url: &str, name: &str, max_age: Duration) -> Result<String, String> {
    fetch_with_meta(url, name, max_age).map(|fetched| fetched.body)
}

// A cache name is one path component under the private cache dir; anything with
// a separator, a leading dot or an unexpected byte could escape or collide.
pub fn safe_cache_name(name: &str) -> Result<(), String> {
    let valid = !name.is_empty()
        && name.len() <= 200
        && !name.starts_with('.')
        && name != "."
        && name != ".."
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'));
    if valid {
        Ok(())
    } else {
        Err(format!("invalid cache name: {name:?}"))
    }
}

pub fn fetch_with_meta(url: &str, name: &str, max_age: Duration) -> Result<Fetched, String> {
    fetch_with_meta_timeout(url, name, max_age, DEFAULT_MAX_TIME)
}

// Reward labels run against a 20 s screen; a path that needs a shorter stall
// bound passes its own timeout here instead of changing every caller.
pub fn fetch_with_meta_timeout(
    url: &str,
    name: &str,
    max_age: Duration,
    timeout: Duration,
) -> Result<Fetched, String> {
    safe_cache_name(name)?;
    let dir = cache_dir();
    ensure_private_dir(&dir).map_err(|error| format!("cache dir: {error}"))?;
    let body = dir.join(name);
    let etag = dir.join(format!("{name}.etag"));
    let headers = dir.join(format!("{name}.headers"));

    if is_fresh(&body, max_age) {
        return read(&body).map(|body| Fetched {
            body,
            throttled: false,
            retry_after: None,
        });
    }

    let tmp = dir.join(format!("{name}.tmp"));
    // curl opens `-o`/`-D` with truncate, keeping the mode of an existing file.
    // Creating them 0600 first is what keeps the body and headers private.
    create_private(&tmp)
        .and_then(|_| create_private(&headers))
        .map_err(|error| format!("cache write: {error}"))?;
    let mut command = Command::new("curl");
    command.args([
        "-sS",
        "-L",
        "--proto",
        "=https",
        "--proto-redir",
        "=https",
        "--max-redirs",
        "3",
        "--max-filesize",
        "33554432",
        "-A",
        "coruframe/0.1",
        "-w",
        "%{http_code}",
    ]);
    command
        .arg("--max-time")
        .arg(timeout.as_secs().max(1).to_string());
    if let Ok(tag) = std::fs::read_to_string(&etag) {
        if !tag.trim().is_empty() {
            command
                .arg("-H")
                .arg(format!("If-None-Match: {}", tag.trim()));
        }
    }
    command.arg("-D").arg(&headers).arg("-o").arg(&tmp).arg(url);

    let output = command.output().map_err(|error| format!("curl: {error}"))?;
    let code = String::from_utf8_lossy(&output.stdout).trim().to_string();

    match code.as_str() {
        "304" => {
            let _ = std::fs::remove_file(&tmp);
            touch(&body);
            read(&body).map(|body| Fetched {
                body,
                throttled: false,
                retry_after: None,
            })
        }
        "200" => {
            let size = std::fs::metadata(&tmp).map(|meta| meta.len()).unwrap_or(0);
            if size > MAX_FETCH_BYTES {
                let _ = std::fs::remove_file(&tmp);
                return Err(format!("response too large: {size} bytes"));
            }
            std::fs::rename(&tmp, &body).map_err(|error| format!("cache write: {error}"))?;
            if let Ok(header_text) = std::fs::read_to_string(&headers) {
                if let Some(tag) = etag_of(&header_text) {
                    let _ = write_private(&etag, tag.as_bytes());
                }
            }
            read(&body).map(|body| Fetched {
                body,
                throttled: false,
                retry_after: None,
            })
        }
        other => {
            let _ = std::fs::remove_file(&tmp);
            let header_text = std::fs::read_to_string(&headers).unwrap_or_default();
            let throttled = other == "429" || other == "509";
            match read(&body) {
                // A throttled answer is served from cache, but the caller must
                // know so it can pause instead of treating it as fresh.
                Ok(stale) => Ok(Fetched {
                    body: stale,
                    throttled,
                    retry_after: retry_after_of(&header_text),
                }),
                Err(_) => Err(format!(
                    "curl HTTP {other}: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                )),
            }
        }
    }
}

pub struct Response {
    pub status: u16,
    pub body: String,
    pub headers: Vec<(String, String)>,
}

// Authenticated writes need no cache, no ETag and a method/body, so they cannot
// reuse `fetch`. The bearer goes in a 0600 curl config file instead of argv:
// process arguments are world-readable through ps and /proc.
pub fn request(
    method: &str,
    url: &str,
    bearer: Option<&str>,
    body: Option<&str>,
) -> Result<Response, String> {
    let authorization = bearer.map(|token| format!("Bearer {token}"));
    request_with_header(method, url, authorization.as_deref(), body)
}

// A raw Authorization value is needed for the v1 sign-in ("JWT"), whose token
// comes back in a response header rather than the body.
pub fn request_with_header(
    method: &str,
    url: &str,
    authorization: Option<&str>,
    body: Option<&str>,
) -> Result<Response, String> {
    let dir = cache_dir();
    ensure_private_dir(&dir).map_err(|error| format!("cache dir: {error}"))?;
    let stamp = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    let config_path = dir.join(format!("request-{}-{stamp}.conf", std::process::id()));
    let headers_path = dir.join(format!(
        "request-headers-{}-{stamp}.txt",
        std::process::id()
    ));

    if let Some(value) = authorization {
        let value = value.replace(['"', '\n', '\r'], "");
        let config = format!("header = \"Authorization: {value}\"\n");
        write_private(&config_path, config.as_bytes())
            .map_err(|error| format!("{}: {error}", config_path.display()))?;
    }
    write_private(&headers_path, b"")
        .map_err(|error| format!("{}: {error}", headers_path.display()))?;

    let mut command = Command::new("curl");
    command.args([
        "-sS",
        "-L",
        "--proto",
        "=https",
        "--proto-redir",
        "=https",
        "--max-redirs",
        "3",
        "--max-filesize",
        "8388608",
        "--max-time",
        "20",
        "-A",
        "coruframe/0.1",
        "-X",
        method,
        "-D",
    ]);
    command.arg(&headers_path);
    if authorization.is_some() {
        command.arg("-K").arg(&config_path);
    }
    // The body arrives on stdin, so a password never exists in argv or a file
    // that could survive a crash.
    command.stdin(Stdio::piped());
    if body.is_some() {
        command.args([
            "-H",
            "Content-Type: application/json",
            "--data-binary",
            "@-",
        ]);
    }
    command.args(["-w", "\n%{http_code}", url]);
    let output = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            if let Some(body) = body {
                if let Some(mut stdin) = child.stdin.take() {
                    stdin.write_all(body.as_bytes())?;
                }
            }
            child.wait_with_output()
        });

    let _ = std::fs::remove_file(&config_path);
    let headers = std::fs::read_to_string(&headers_path)
        .map(|text| parse_headers(&text))
        .unwrap_or_default();
    let _ = std::fs::remove_file(&headers_path);
    let output = output.map_err(|error| format!("curl: {error}"))?;
    if output.stdout.len() > MAX_REQUEST_BYTES {
        return Err(format!("response too large: {} bytes", output.stdout.len()));
    }

    let (body, code) = String::from_utf8_lossy(&output.stdout)
        .rsplit_once('\n')
        .map(|(body, code)| (body.to_string(), code.trim().to_string()))
        .unwrap_or_default();
    let status = code.parse::<u16>().unwrap_or(0);
    if status == 0 {
        return Err(format!(
            "curl: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(Response {
        status,
        body,
        headers,
    })
}

// Keeps only the final response block; `-L` makes curl log every redirect.
fn parse_headers(text: &str) -> Vec<(String, String)> {
    let mut headers = Vec::new();
    for line in text.lines() {
        if line.starts_with("HTTP/") {
            headers.clear();
            continue;
        }
        if line.starts_with([' ', '\t']) {
            continue;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
        }
    }
    headers
}

fn is_fresh(path: &Path, max_age: Duration) -> bool {
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .is_some_and(|age| age < max_age)
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))
}

fn touch(path: &Path) {
    if let Ok(file) = std::fs::OpenOptions::new().append(true).open(path) {
        let _ = file.set_modified(SystemTime::now());
    }
}

fn etag_of(headers: &str) -> Option<String> {
    headers.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.trim()
            .eq_ignore_ascii_case("etag")
            .then(|| value.trim().to_string())
    })
}

// Cloudflare sends seconds; an HTTP-date value is left to the caller's default.
fn retry_after_of(headers: &str) -> Option<u64> {
    headers.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.trim()
            .eq_ignore_ascii_case("retry-after")
            .then(|| value.trim().parse().ok())
            .flatten()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_etag_case_insensitively() {
        assert_eq!(
            etag_of("HTTP/1.1 200 OK\r\nETag: \"abc\"\r\n").as_deref(),
            Some("\"abc\"")
        );
        assert_eq!(etag_of("etag: w/\"x\"\n"), Some("w/\"x\"".to_string()));
        assert_eq!(etag_of("HTTP/1.1 304 Not Modified\n"), None);
    }

    #[test]
    fn reads_retry_after_seconds() {
        assert_eq!(
            retry_after_of("HTTP/2 429\r\nRetry-After: 120\r\n"),
            Some(120)
        );
        assert_eq!(retry_after_of("retry-after: 0\n"), Some(0));
        assert_eq!(
            retry_after_of("Retry-After: Wed, 21 Oct 2015 07:28:00 GMT\n"),
            None
        );
        assert_eq!(retry_after_of("HTTP/2 200\r\n"), None);
    }

    #[test]
    fn accepts_only_single_component_cache_names() {
        for name in [
            "items.json",
            "orders-ash_prime.json",
            "top-axi_a1_relic-3-radiant.json",
            "orders-recent.json",
        ] {
            assert!(safe_cache_name(name).is_ok(), "{name}");
        }
        for name in [
            "",
            ".",
            "..",
            ".hidden",
            "../escape.json",
            "sub/dir.json",
            "with space.json",
            "query?.json",
            "hash#.json",
            "control\u{7f}.json",
        ] {
            assert!(safe_cache_name(name).is_err(), "{name:?}");
        }
    }

    #[test]
    fn keeps_only_the_last_response_headers() {
        let text =
            "HTTP/2 302\r\nlocation: /next\r\n\r\nHTTP/2 200\r\nAuthorization: JWT abc.def\r\n";
        let headers = parse_headers(text);
        assert_eq!(
            headers,
            vec![("authorization".to_string(), "JWT abc.def".to_string())]
        );
    }
}
