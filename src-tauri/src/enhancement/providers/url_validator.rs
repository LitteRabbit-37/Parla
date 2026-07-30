// Validation des endpoints user-configurables (custom OpenAI-compat, ollama).
//
// Objectif : empecher un utilisateur (ou une injection store) de poser une
// URL malformee qui exposerait la cle API en clair (http non chiffre) ou
// qui casserait silencieusement la resolution.
//
// Regles :
// - Le scheme doit etre https, sauf si `allow_plain_http_loopback` est vrai
//   ET que l'host est localhost / 127.0.0.1 / [::1].
// - L'host est requis (une URL sans host est rejetee).
// - Le path peut etre vide. Si present, le trailing slash est strip a la
//   normalisation.
// - Les URL unicode / IDN sont acceptees (reqwest les supporte).

use anyhow::{anyhow, Result};

/// Valide et normalise une URL d'endpoint. Retourne la forme canonique (sans
/// trailing slash) en cas de succes.
pub fn validate_endpoint(url: &str, allow_plain_http_loopback: bool) -> Result<String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("URL vide"));
    }

    let parsed =
        reqwest::Url::parse(trimmed).map_err(|e| anyhow!("URL invalide '{trimmed}': {e}"))?;

    let scheme = parsed.scheme();
    let host = parsed
        .host_str()
        .ok_or_else(|| anyhow!("URL sans host : '{trimmed}'"))?;

    match scheme {
        "https" => { /* OK toujours */ }
        "http" => {
            if !allow_plain_http_loopback {
                return Err(anyhow!(
                    "scheme http non autorise ('{trimmed}') - utilise https"
                ));
            }
            if !is_loopback(host) {
                return Err(anyhow!(
                    "http autorise uniquement sur loopback (localhost / 127.0.0.1 / ::1), recu '{host}'"
                ));
            }
        }
        other => {
            return Err(anyhow!(
                "scheme non supporte '{other}' (seulement http/https)"
            ))
        }
    }

    let canonical = parsed.as_str().trim_end_matches('/').to_string();
    Ok(canonical)
}

fn is_loopback(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1") || host.starts_with("127.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty() {
        assert!(validate_endpoint("", false).is_err());
        assert!(validate_endpoint("   ", false).is_err());
    }

    #[test]
    fn rejects_malformed() {
        assert!(validate_endpoint("not a url", false).is_err());
        assert!(validate_endpoint("ftp://example.com", false).is_err());
        assert!(validate_endpoint("https://", false).is_err());
    }

    #[test]
    fn accepts_https() {
        let got = validate_endpoint("https://api.example.com/v1", false).unwrap();
        assert_eq!(got, "https://api.example.com/v1");
    }

    #[test]
    fn strips_trailing_slash() {
        let got = validate_endpoint("https://api.example.com/v1/", false).unwrap();
        assert_eq!(got, "https://api.example.com/v1");
    }

    #[test]
    fn rejects_http_when_disallowed() {
        assert!(validate_endpoint("http://api.example.com", false).is_err());
        assert!(validate_endpoint("http://localhost:11434", false).is_err());
    }

    #[test]
    fn accepts_http_loopback_when_allowed() {
        assert!(validate_endpoint("http://localhost:11434", true).is_ok());
        assert!(validate_endpoint("http://127.0.0.1:11434", true).is_ok());
        assert!(validate_endpoint("http://127.0.0.1", true).is_ok());
    }

    #[test]
    fn rejects_http_non_loopback_even_when_loopback_allowed() {
        assert!(validate_endpoint("http://api.example.com", true).is_err());
        assert!(validate_endpoint("http://192.168.1.1", true).is_err());
    }
}
