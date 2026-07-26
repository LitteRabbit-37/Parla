// Module services - wrappers systemes reutilisables (keyring, etc.)

pub mod api_keys;
pub mod download;
pub mod proxy;
// WinHTTP is deliberately not available on other targets. Callers select the
// system route at runtime and receive a clear platform error there.
#[cfg(windows)]
pub mod winhttp;
#[cfg(windows)]
pub mod winhttp_download;
