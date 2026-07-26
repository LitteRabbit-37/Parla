//! Application proxy configuration and backend-only credential storage.
use anyhow::{Context, Result};
use keyring::Entry;
#[cfg(test)]
use parking_lot::Mutex;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;
use url::Url;

const CREDENTIAL_SERVICE: &str = "Parla.ApplicationProxy";
const CREDENTIAL_KEY: &str = "credentials";
const STORE_FILE: &str = "parla.settings.json";
const SETTINGS_KEY: &str = "proxy_settings";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProxyRoute {
    Direct,
    System,
    Explicit {
        url: String,
        credentials: Option<ProxyCredentials>,
    },
}

#[derive(Debug, Clone)]
pub struct RouteDiagnostic {
    pub kind: &'static str,
    pub scheme: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
}

/// Application-owned NO_PROXY matcher. Kept separate from reqwest so every
/// transport (including WebSocket) makes the same routing decision.
pub fn no_proxy_matches(host: &str, port: Option<u16>, entries: &[String]) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let ip = host.parse::<std::net::IpAddr>().ok();
    entries.iter().any(|raw| {
        let Ok(mut entry) = parse_no_proxy_entry(raw) else {
            return false;
        };
        if entry == "*" {
            return true;
        }
        let wildcard = raw.trim_start().starts_with("*.");
        let entry_port =
            if entry.parse::<std::net::IpAddr>().is_ok() || entry.parse::<ipnet::IpNet>().is_ok() {
                None
            } else {
                entry
                    .rsplit_once(':')
                    .and_then(|(_, p)| p.parse::<u16>().ok())
            };
        if let Some(p) = entry_port {
            if port != Some(p) {
                return false;
            }
            entry = entry[..entry.rfind(':').unwrap()].to_string();
        }
        let entry = entry.trim_start_matches('[').trim_end_matches(']');
        if let (Some(ip), Ok(net)) = (ip, entry.parse::<ipnet::IpNet>()) {
            return net.contains(&ip);
        }
        let mut entry = entry.to_owned();
        if entry.starts_with('.') {
            entry = entry[1..].to_string();
        }
        if wildcard {
            host != entry && host.ends_with(&format!(".{entry}"))
        } else {
            host == entry || host.ends_with(&format!(".{entry}"))
        }
    })
}

#[derive(Debug, Clone, Default)]
struct RuntimeProxy {
    route: Option<ProxyRoute>,
    no_proxy: Vec<String>,
}

static RUNTIME: OnceLock<RwLock<RuntimeProxy>> = OnceLock::new();
fn runtime() -> &'static RwLock<RuntimeProxy> {
    RUNTIME.get_or_init(|| RwLock::new(RuntimeProxy::default()))
}

#[cfg(test)]
pub(crate) struct TestRuntimeGuard {
    previous: RuntimeProxy,
    _lock: parking_lot::MutexGuard<'static, ()>,
}

#[cfg(test)]
static TEST_RUNTIME_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[cfg(test)]
impl Drop for TestRuntimeGuard {
    fn drop(&mut self) {
        *runtime().write() = self.previous.clone();
    }
}

#[cfg(test)]
pub(crate) fn test_runtime_guard(
    route: Option<ProxyRoute>,
    no_proxy: Vec<String>,
) -> TestRuntimeGuard {
    let lock = TEST_RUNTIME_LOCK.get_or_init(|| Mutex::new(())).lock();
    let mut state = runtime().write();
    let previous = state.clone();
    state.route = route;
    state.no_proxy = no_proxy;
    TestRuntimeGuard {
        previous,
        _lock: lock,
    }
}

/// Load persisted settings and credentials into backend-only runtime state.
/// Credentials never leave this module or enter a frontend-serializable type.
pub fn configure(app: &AppHandle) -> Result<()> {
    let settings = app
        .store(STORE_FILE)
        .ok()
        .and_then(|s| s.get(SETTINGS_KEY))
        .and_then(|v| serde_json::from_value::<StoredSettings>(v).ok())
        .unwrap_or_default();
    let credentials = if settings.enabled
        && settings
            .url
            .as_deref()
            .is_some_and(|u| !u.trim().is_empty())
    {
        settings
            .url
            .as_deref()
            .and_then(|url| proxy_identity(url).ok())
            .and_then(|identity| get_credentials_for(&identity).ok().flatten())
    } else {
        let _ = delete_credentials();
        None
    };
    let mut state = runtime().write();
    state.route = Some(route(
        settings.enabled,
        settings.url.as_deref(),
        credentials,
    ));
    state.no_proxy = settings.no_proxy_entries;
    Ok(())
}

#[derive(Debug, Default, Deserialize)]
struct StoredSettings {
    enabled: bool,
    url: Option<String>,
    #[serde(default)]
    no_proxy_entries: Vec<String>,
}

/// Apply policy after resolving destination. This is required for System mode:
/// reqwest proxy callbacks cannot return resolver errors and would otherwise
/// silently turn a failed WinHTTP lookup into a direct connection.
pub fn apply_for_url(builder: reqwest::ClientBuilder, url: &Url) -> Result<reqwest::ClientBuilder> {
    let route = route_for_url(url)?;
    apply_route(builder, route)
}

/// Applies routing and returns metadata safe for diagnostics. Never includes
/// proxy userinfo, credentials, PAC data, or the destination URL.
pub fn apply_for_url_with_diagnostic(
    builder: reqwest::ClientBuilder,
    url: &Url,
) -> Result<(reqwest::ClientBuilder, RouteDiagnostic)> {
    let route = route_for_url(url)?;
    let diagnostic = route_diagnostic(&route);
    Ok((apply_route(builder, route)?, diagnostic))
}

fn route_diagnostic(route: &ProxyRoute) -> RouteDiagnostic {
    match route {
        ProxyRoute::Direct => RouteDiagnostic {
            kind: "direct",
            scheme: None,
            host: None,
            port: None,
        },
        ProxyRoute::System => RouteDiagnostic {
            kind: "system",
            scheme: None,
            host: None,
            port: None,
        },
        ProxyRoute::Explicit { url, .. } => {
            let parsed = Url::parse(url).ok();
            RouteDiagnostic {
                kind: "explicit",
                scheme: parsed.as_ref().map(|value| value.scheme().to_owned()),
                host: parsed
                    .as_ref()
                    .and_then(|value| value.host_str().map(str::to_owned)),
                port: parsed
                    .as_ref()
                    .and_then(|value| value.port_or_known_default()),
            }
        }
    }
}

fn apply_route(
    builder: reqwest::ClientBuilder,
    route: ProxyRoute,
) -> Result<reqwest::ClientBuilder> {
    let builder = match route {
        ProxyRoute::Direct => builder.no_proxy(),
        ProxyRoute::System => anyhow::bail!("unresolved system proxy route"),
        ProxyRoute::Explicit { url, credentials } => {
            let mut proxy = reqwest::Proxy::all(&url).map_err(|e| {
                let diagnostic = crate::services::download::sanitize_message(&e.to_string());
                anyhow::anyhow!("proxy: {diagnostic}")
            })?;
            if let Some(c) = credentials {
                proxy = proxy.basic_auth(&c.username, &c.password);
            }
            let no_proxy = runtime().read().no_proxy.clone();
            if !no_proxy.is_empty() {
                proxy = proxy.no_proxy(reqwest::NoProxy::from_string(&no_proxy.join(",")));
            }
            builder.proxy(proxy)
        }
    };
    Ok(builder)
}

pub fn route_for_url(url: &Url) -> Result<ProxyRoute> {
    let state = runtime().read().clone();
    select_route(state.route, &state.no_proxy, url)
}

fn select_route(route: Option<ProxyRoute>, no_proxy: &[String], url: &Url) -> Result<ProxyRoute> {
    match route {
        Some(ProxyRoute::Explicit { .. })
            if no_proxy_matches(
                url.host_str().unwrap_or_default(),
                url.port_or_known_default(),
                no_proxy,
            ) =>
        {
            Ok(ProxyRoute::Direct)
        }
        Some(ProxyRoute::System) => resolve_system(url),
        Some(route) => Ok(route),
        None => anyhow::bail!("proxy policy not configured"),
    }
}

/// Resolve Windows Internet proxy settings for one destination. No environment
/// fallback: system mode is deliberately WinHTTP-only.
#[cfg(windows)]
fn resolve_system(_url: &Url) -> Result<ProxyRoute> {
    // Keep System as a transport decision. Resolving here would turn a
    // system result into Direct or Explicit, causing callers to use reqwest
    // instead of WinHTTP and potentially bypass the Windows proxy policy.
    Ok(ProxyRoute::System)
}

#[cfg(not(windows))]
fn resolve_system(_url: &Url) -> Result<ProxyRoute> {
    anyhow::bail!("Windows system proxy is unsupported on this platform")
}

fn parse_windows_proxy(url: &Url, value: Option<&str>) -> Result<ProxyRoute> {
    let value = value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("Windows system proxy settings contain no usable route"))?;
    if value.eq_ignore_ascii_case("DIRECT") || value.is_empty() {
        return Ok(ProxyRoute::Direct);
    }
    let wanted = match url.scheme() {
        "https" | "wss" => ["https", "http", "socks"],
        _ => ["http", "https", "socks"],
    };
    let mut candidates = Vec::new();
    for item in value.split(';').map(str::trim).filter(|x| !x.is_empty()) {
        // WinHTTP returns PAC results as `PROXY host:port; DIRECT`, while
        // static IE settings use `http=host:port`. Accept both grammars.
        let (kind, address) = if let Some((kind, address)) = item.split_once('=') {
            (kind.trim(), address.trim())
        } else if let Some((kind, address)) = item.split_once(char::is_whitespace) {
            (kind.trim(), address.trim())
        } else {
            ("", item)
        };
        let kind_lower = kind.to_ascii_lowercase();
        if kind_lower == "direct" {
            continue;
        }
        if kind.is_empty() || kind_lower == "proxy" || wanted.contains(&kind_lower.as_str()) {
            candidates.push((kind_lower, address));
        }
    }
    for wanted_kind in wanted {
        let Some((kind, address)) = candidates.iter().find(|(kind, _)| {
            kind.as_str() == wanted_kind || (wanted_kind == "http" && kind == "proxy")
        }) else {
            continue;
        };
        let address = address.trim();
        let scheme = match kind.as_str() {
            "proxy" => "http",
            "socks" => "socks5",
            "https" => "https",
            _ => "http",
        };
        let normalized = if address.contains("://") {
            address.to_owned()
        } else {
            format!("{scheme}://{address}")
        };
        if Url::parse(&normalized)
            .ok()
            .and_then(|p| p.host_str().map(|_| ()))
            .is_some()
        {
            return Ok(ProxyRoute::Explicit {
                url: normalized,
                credentials: None,
            });
        }
    }
    Err(anyhow::anyhow!(
        "could not parse Windows proxy settings for {}",
        url.scheme()
    ))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ProxyCredentials {
    pub username: String,
    pub password: String,
}

pub fn validate_settings(
    _enabled: bool,
    url: Option<&str>,
    entries: &[String],
) -> Result<(), String> {
    for entry in entries {
        parse_no_proxy_entry(entry)
            .map_err(|reason| format!("invalid No-Proxy Entry {entry:?}: {reason}"))?;
    }
    let Some(value) = url.filter(|v| !v.trim().is_empty()) else {
        return Ok(());
    };
    let parsed = Url::parse(value).map_err(|e| format!("invalid proxy URL: {e}"))?;
    if !matches!(parsed.scheme(), "http" | "https" | "socks5") {
        return Err("proxy URL scheme must be http, https, or socks5".into());
    }
    if parsed.host_str().is_none() {
        return Err("proxy URL must contain a host".into());
    }
    if parsed.password().is_some() && parsed.username().is_empty() {
        return Err("proxy URL credentials require a username".into());
    }
    Ok(())
}

/// Remove URL userinfo before persistence and return credentials for vault storage.
pub fn sanitize_proxy_url(value: &str) -> Result<(String, Option<(String, String)>), String> {
    let mut parsed = Url::parse(value).map_err(|e| format!("invalid proxy URL: {e}"))?;
    let credentials = if !parsed.username().is_empty() || parsed.password().is_some() {
        Some((
            parsed.username().to_owned(),
            parsed.password().unwrap_or_default().to_owned(),
        ))
    } else {
        None
    };
    if credentials.as_ref().is_some_and(|(u, _)| u.is_empty()) {
        return Err("proxy URL credentials require a username".into());
    }
    parsed
        .set_username("")
        .map_err(|_| "could not sanitize proxy username".to_owned())?;
    parsed
        .set_password(None)
        .map_err(|_| "could not sanitize proxy password".to_owned())?;
    Ok((parsed.to_string(), credentials))
}

/// Stable origin used to bind vault credentials to one proxy endpoint.
pub fn proxy_identity(value: &str) -> Result<String, String> {
    let parsed = Url::parse(value).map_err(|e| format!("invalid proxy URL: {e}"))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| "proxy URL must contain a host".to_owned())?;
    let port = parsed
        .port_or_known_default()
        .map(|p| p.to_string())
        .unwrap_or_default();
    Ok(format!(
        "{}://{}:{}",
        parsed.scheme().to_ascii_lowercase(),
        host.to_ascii_lowercase(),
        port
    ))
}

/// Parse and normalize NO_PROXY grammar. Returned value has no surrounding
/// brackets and uses `host:port` only for a real numeric port.
fn parse_no_proxy_entry(raw: &str) -> Result<String, &'static str> {
    let value = raw.trim();
    if value.is_empty() || value.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err("must be non-empty and contain no whitespace");
    }
    if value == "*" {
        return Ok(value.into());
    }
    let (host, port) = if value.starts_with('[') {
        let end = value.find(']').ok_or("missing closing IPv6 bracket")?;
        let host = &value[1..end];
        let rest = &value[end + 1..];
        let port = if rest.is_empty() {
            None
        } else {
            Some(
                rest.strip_prefix(':')
                    .ok_or("unexpected text after IPv6 bracket")?,
            )
        };
        (host, port)
    } else if let Some((h, p)) = value.rsplit_once(':') {
        if h.contains(':') {
            (value, None)
        } else {
            (h, Some(p))
        }
    } else {
        (value, None)
    };
    if let Some(p) = port {
        if p.parse::<u16>().is_err() {
            return Err("port must be 0-65535");
        }
    }
    let host = host
        .trim_start_matches("*.")
        .trim_start_matches('.')
        .to_ascii_lowercase();
    if host.is_empty() || host.contains('[') || host.contains(']') {
        return Err("invalid host pattern");
    }
    if host.parse::<std::net::IpAddr>().is_err() && host.parse::<ipnet::IpNet>().is_err() {
        if host.contains('*') && !raw.trim_start().starts_with("*.") {
            return Err("wildcards are allowed only as *.<domain>");
        }
        if host.starts_with("*") {
            return Err("wildcards are allowed only as *.<domain>");
        }
        if !host.contains('.') && host != "localhost" {
            return Err("invalid host pattern");
        }
    }
    Ok(match port {
        Some(p) if host.contains(':') => format!("[{host}]:{p}"),
        Some(p) => format!("{host}:{p}"),
        None => host,
    })
}

pub fn route(
    enabled: bool,
    url: Option<&str>,
    credentials: Option<ProxyCredentials>,
) -> ProxyRoute {
    if !enabled {
        ProxyRoute::Direct
    } else if let Some(url) = url.filter(|v| !v.trim().is_empty()) {
        ProxyRoute::Explicit {
            url: url.to_owned(),
            credentials,
        }
    } else {
        ProxyRoute::System
    }
}

fn entry() -> Result<Entry> {
    Entry::new(CREDENTIAL_SERVICE, CREDENTIAL_KEY).context("proxy credential keyring entry")
}
pub fn get_credentials() -> Result<Option<ProxyCredentials>> {
    match entry()?.get_password() {
        Ok(value) => Ok(Some(
            serde_json::from_str::<StoredCredentials>(&value)
                .map(|v| v.credentials)
                .or_else(|_| serde_json::from_str(&value))
                .context("invalid proxy credentials")?,
        )),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(anyhow::anyhow!("keyring get: {e}")),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredCredentials {
    credentials: ProxyCredentials,
    #[serde(default)]
    identity: Option<String>,
}

fn get_credentials_for(identity: &str) -> Result<Option<ProxyCredentials>> {
    match entry()?.get_password() {
        Ok(value) => {
            let stored: StoredCredentials =
                serde_json::from_str(&value).context("invalid proxy credentials")?;
            Ok((stored.identity.as_deref() == Some(identity)).then_some(stored.credentials))
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(anyhow::anyhow!("keyring get: {e}")),
    }
}

pub fn set_credentials_for(username: &str, password: &str, identity: &str) -> Result<()> {
    if username.is_empty() {
        anyhow::bail!("proxy username cannot be empty");
    }
    entry()?
        .set_password(&serde_json::to_string(&StoredCredentials {
            credentials: ProxyCredentials {
                username: username.into(),
                password: password.into(),
            },
            identity: Some(identity.into()),
        })?)
        .map_err(|e| anyhow::anyhow!("keyring set: {e}"))
}
pub fn delete_credentials() -> Result<()> {
    match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(anyhow::anyhow!("keyring delete: {e}")),
    }
}
pub fn has_credentials() -> bool {
    matches!(get_credentials(), Ok(Some(_)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn routing_modes() {
        assert_eq!(route(false, Some("http://proxy"), None), ProxyRoute::Direct);
        assert_eq!(route(true, None, None), ProxyRoute::System);
        assert!(matches!(
            route(true, Some("socks5://proxy:1080"), None),
            ProxyRoute::Explicit { .. }
        ));
    }
    #[test]
    fn config_validation() {
        assert!(validate_settings(
            true,
            Some("https://proxy.example:8080"),
            &["*.example.com".into(), "10.0.0.0/8".into()]
        )
        .is_ok());
        assert!(validate_settings(true, Some("ftp://proxy.example"), &[]).is_err());
        assert!(validate_settings(true, Some("http://user:pass@proxy.example"), &[]).is_ok());
    }
    #[test]
    fn windows_proxy_selection() {
        let https = Url::parse("https://example.test").unwrap();
        assert!(matches!(
            parse_windows_proxy(&https, Some("DIRECT")),
            Ok(ProxyRoute::Direct)
        ));
        assert!(matches!(
            parse_windows_proxy(&https, Some("http=proxy:8080; https=secure:8443")),
            Ok(ProxyRoute::Explicit { url, .. }) if url == "https://secure:8443"
        ));
        assert!(matches!(
            parse_windows_proxy(&https, Some("socks=socks:1080")),
            Ok(ProxyRoute::Explicit { url, .. }) if url == "socks5://socks:1080"
        ));
        assert!(matches!(
            parse_windows_proxy(&https, Some("PROXY proxy:3129; DIRECT")),
            Ok(ProxyRoute::Explicit { url, .. }) if url == "http://proxy:3129"
        ));
        assert!(matches!(
            parse_windows_proxy(&https, Some("HTTPS proxy:8443; DIRECT")),
            Ok(ProxyRoute::Explicit { url, .. }) if url == "https://proxy:8443"
        ));
        assert!(matches!(
            parse_windows_proxy(&https, Some("SOCKS proxy:1080; DIRECT")),
            Ok(ProxyRoute::Explicit { url, .. }) if url == "socks5://proxy:1080"
        ));
        assert!(matches!(
            parse_windows_proxy(&https, Some("DIRECT")),
            Ok(ProxyRoute::Direct)
        ));
    }
    #[test]
    fn no_proxy_matching_is_destination_aware() {
        let e = [
            "exact.example:443",
            ".suffix.example",
            "*.wild.example",
            "10.0.0.0/8",
            "2001:db8::/32",
        ]
        .into_iter()
        .map(String::from)
        .collect::<Vec<_>>();
        assert!(no_proxy_matches("exact.example", Some(443), &e));
        assert!(!no_proxy_matches("exact.example", Some(80), &e));
        assert!(no_proxy_matches("child.suffix.example", Some(80), &e));
        assert!(no_proxy_matches("child.wild.example", Some(80), &e));
        assert!(!no_proxy_matches("wild.example", Some(80), &e));
        assert!(no_proxy_matches("10.2.3.4", Some(80), &e));
        assert!(no_proxy_matches("2001:db8::1", Some(80), &e));
        assert!(!no_proxy_matches("other.example", Some(80), &e));
        assert!(no_proxy_matches(
            "2001:db8::1",
            Some(443),
            &["[2001:db8::1]:443".into()]
        ));
        assert!(!no_proxy_matches(
            "2001:db8::1",
            Some(80),
            &["[2001:db8::1]:443".into()]
        ));
        assert!(no_proxy_matches(
            "2001:db8::2",
            Some(80),
            &["2001:db8::/64".into()]
        ));
    }

    #[test]
    #[cfg(windows)]
    fn application_no_proxy_preserves_system_route() {
        assert_eq!(
            select_route(
                Some(ProxyRoute::System),
                &["internal.example".into()],
                &Url::parse("https://internal.example/api").unwrap(),
            )
            .unwrap(),
            ProxyRoute::System
        );
    }

    #[test]
    #[cfg(not(windows))]
    fn system_route_is_explicitly_unsupported() {
        let error = select_route(
            Some(ProxyRoute::System),
            &[],
            &Url::parse("https://example.test").unwrap(),
        )
        .unwrap_err();
        assert!(error
            .to_string()
            .contains("Windows system proxy is unsupported"));
    }

    #[test]
    fn malformed_system_proxy_is_not_treated_as_direct() {
        let url = Url::parse("https://example.test").unwrap();
        let error = parse_windows_proxy(&url, Some("HTTPS not a valid proxy")).unwrap_err();
        assert!(error.to_string().contains("could not parse Windows proxy"));
    }
}
