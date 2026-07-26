// Module services - wrappers systemes reutilisables (keyring, etc.)

pub mod api_keys;
pub mod download;
pub mod proxy;
#[cfg(windows)]
pub mod winhttp_download;
