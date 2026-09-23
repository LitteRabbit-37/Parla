// Types et trait partages pour les sessions de streaming.
//
// Flux global :
//   1. Le frontend demande "start_cloud_streaming" -> on cree un StreamingHandle.
//   2. Le recorder audio pousse des chunks Int16 via handle.push_audio().
//   3. Chaque chunk passe par un task tokio specifique au provider qui
//      convertit + envoie au WebSocket.
//   4. Le provider emet StreamingEvent::Partial / Committed au fur et a mesure.
//   5. Le frontend demande "finalize_cloud_streaming" -> commit WebSocket,
//      on recupere le texte final.

use std::time::Duration;

use anyhow::anyhow;
use async_trait::async_trait;
use serde::Serialize;
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::handshake::client::Request as WsRequest;

/// Timeout pour l'etablissement d'une connexion WebSocket streaming.
/// Une fois connecte, le flux peut durer indefiniment (pas de timeout global).
pub const WS_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Etablit une connexion WebSocket avec un timeout de handshake.
/// Remplace l'appel direct a `tokio_tungstenite::connect_async` pour eviter
/// les hangs indefinis si le handshake bloque (firewall, proxy, DNS lent).
pub async fn connect_ws(
    req: WsRequest,
) -> anyhow::Result<
    tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
> {
    let url_for_err = req.uri().to_string();
    match tokio::time::timeout(WS_CONNECT_TIMEOUT, tokio_tungstenite::connect_async(req)).await {
        Ok(Ok((stream, _))) => Ok(stream),
        Ok(Err(e)) => Err(anyhow!("ws connect {url_for_err}: {e}")),
        Err(_) => Err(anyhow!(
            "timeout: ws connect {url_for_err} (>{}s)",
            WS_CONNECT_TIMEOUT.as_secs()
        )),
    }
}

/// Variante prenant directement une string URL, pour les providers qui n'ont
/// pas besoin d'ajouter de headers custom avant le connect.
pub async fn connect_ws_url(
    url: &str,
) -> anyhow::Result<
    tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
> {
    let req = url
        .into_client_request()
        .map_err(|e| anyhow!("ws url parse {url}: {e}"))?;
    connect_ws(req).await
}

/// Evenements emis par une session de streaming vers le frontend.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StreamingEvent {
    /// Connexion etablie et handshake reussi.
    SessionStarted,
    /// Texte partiel (non final) en cours de reconnaissance.
    Partial { text: String },
    /// Morceau final commite. Les morceaux commit s'accumulent.
    Committed { text: String },
    /// Erreur remontee par le provider (non fatale ou fatale).
    Error { message: String },
}

/// Configuration de la session.
#[derive(Debug, Clone, Default)]
pub struct StreamingConfig {
    pub model: String,
    pub language: Option<String>,
    pub custom_vocabulary: Vec<String>,
}

/// Handle utilise par le pipeline pour pousser de l'audio et demander
/// la finalisation.
pub struct StreamingHandle {
    audio_tx: mpsc::UnboundedSender<Vec<i16>>,
    finalize_tx: Option<oneshot::Sender<()>>,
    done_rx: oneshot::Receiver<anyhow::Result<String>>,
}

impl StreamingHandle {
    pub fn new(
        audio_tx: mpsc::UnboundedSender<Vec<i16>>,
        finalize_tx: oneshot::Sender<()>,
        done_rx: oneshot::Receiver<anyhow::Result<String>>,
    ) -> Self {
        Self {
            audio_tx,
            finalize_tx: Some(finalize_tx),
            done_rx,
        }
    }

    /// Retourne un sender clone pour que le recorder puisse pousser l'audio
    /// directement sans passer par le state Tauri (hot-path a 30-100 Hz).
    pub fn audio_sender(&self) -> mpsc::UnboundedSender<Vec<i16>> {
        self.audio_tx.clone()
    }

    /// Envoie le signal de finalisation et attend le texte final.
    pub async fn finalize(mut self) -> anyhow::Result<String> {
        if let Some(tx) = self.finalize_tx.take() {
            let _ = tx.send(());
        }
        match self.done_rx.await {
            Ok(r) => r,
            Err(_) => Err(anyhow::anyhow!("streaming task a panic")),
        }
    }
}

/// Canaux internes passes aux run() de chaque provider.
pub struct StreamingChannels {
    pub audio_rx: mpsc::UnboundedReceiver<Vec<i16>>,
    pub finalize_rx: oneshot::Receiver<()>,
}

/// Trait implemente par chaque provider streaming.
#[async_trait]
pub trait StreamingProvider: Send + Sync {
    fn id(&self) -> &'static str;

    /// Execute la session du debut a la fin. Emet les evenements via on_event.
    /// Retourne le texte final concatene a la cloture du WebSocket.
    async fn run(
        &self,
        api_key: String,
        config: StreamingConfig,
        channels: StreamingChannels,
        on_event: Box<dyn Fn(StreamingEvent) + Send + Sync>,
    ) -> anyhow::Result<String>;
}

/// Draine les messages WebSocket jusqu'a un timeout ou fermeture.
/// Utilise apres envoi du commit pour recuperer les derniers transcripts.
/// Chaque message texte est passe au callback fourni.
pub async fn drain_ws_messages<S, F>(read: &mut S, timeout: std::time::Duration, mut on_text: F)
where
    S: futures_util::StreamExt<Item = Result<tokio_tungstenite::tungstenite::Message, tokio_tungstenite::tungstenite::Error>>
        + Unpin,
    F: FnMut(&str),
{
    use tokio_tungstenite::tungstenite::Message;
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        let remaining = deadline - tokio::time::Instant::now();
        match tokio::time::timeout(remaining, read.next()).await {
            Ok(Some(Ok(Message::Text(t)))) => on_text(&t),
            Ok(Some(Ok(Message::Close(_)))) | Ok(None) | Err(_) => break,
            _ => {}
        }
    }
}

/// Formatte un buffer i16 en bytes little-endian.
pub fn i16_to_le_bytes(chunk: &[i16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(chunk.len() * 2);
    for &s in chunk {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

/// Encode un chunk i16 LE en base64 (pour les providers JSON).
pub fn i16_to_base64(chunk: &[i16]) -> String {
    use base64::{engine::general_purpose::STANDARD as B64, Engine};
    B64.encode(i16_to_le_bytes(chunk))
}
