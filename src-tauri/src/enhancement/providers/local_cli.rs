// Local CLI provider : lance un outil en ligne de commande local avec le
// prompt transcription + systeme.
//
// Reference VoiceInk : VoiceInk/Services/AIEnhancement/LocalCLIService.swift.
// VoiceInk utilise /bin/zsh -lc. Sous Windows, on passe par
// `powershell.exe -NoProfile -Command` qui est present partout (pwsh.exe
// pourrait etre absent).
//
// Templates reproduits (argument passe a -Command) :
//   - pi     : & pi -ne -ns -p --no-tools --system-prompt $env:PARLA_SYSTEM_PROMPT $env:PARLA_USER_PROMPT
//   - claude : & claude -p $env:PARLA_FULL_PROMPT
//   - codex  : $t = New-TemporaryFile; & codex exec --skip-git-repo-check --output-last-message $t.FullName $env:PARLA_FULL_PROMPT *> $null; Get-Content $t.FullName; Remove-Item $t
//   - custom : commande libre tapee par l'utilisateur.
//
// Le modele (champ "model") est le nom du template (pi/claude/codex/custom).
// La commande custom est stockee dans parla.settings.json:llm_localcli_custom_cmd.
// Le timeout est configurable dans parla.settings.json:llm_localcli_timeout_secs
// (defaut 45s).

use std::process::Stdio;
use std::time::Duration;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::enhancement::provider::{EnhancementRequest, EnhancementResponse, LLMProvider};

pub struct LocalCLIProvider;

const STORE_FILE: &str = "parla.settings.json";
const KEY_CUSTOM_CMD: &str = "llm_localcli_custom_cmd";
const KEY_TIMEOUT: &str = "llm_localcli_timeout_secs";
const DEFAULT_TIMEOUT: u64 = 45;

const TEMPLATES: &[&str] = &["pi", "claude", "codex", "custom"];

pub fn get_custom_cmd(app: &AppHandle) -> Option<String> {
    app.store(STORE_FILE)
        .ok()
        .and_then(|s| {
            s.get(KEY_CUSTOM_CMD)
                .and_then(|v| v.as_str().map(String::from))
        })
        .filter(|s| !s.is_empty())
}

pub fn set_custom_cmd(app: &AppHandle, cmd: &str) -> Result<()> {
    let store = app.store(STORE_FILE).map_err(|e| anyhow!("store: {e}"))?;
    if cmd.is_empty() {
        store.delete(KEY_CUSTOM_CMD);
    } else {
        store.set(KEY_CUSTOM_CMD, serde_json::Value::String(cmd.into()));
    }
    store.save().map_err(|e| anyhow!("store save: {e}"))?;
    Ok(())
}

pub fn get_timeout_secs(app: &AppHandle) -> u64 {
    app.store(STORE_FILE)
        .ok()
        .and_then(|s| s.get(KEY_TIMEOUT).and_then(|v| v.as_u64()))
        .unwrap_or(DEFAULT_TIMEOUT)
        .max(5)
}

pub fn set_timeout_secs(app: &AppHandle, secs: u64) -> Result<()> {
    let store = app.store(STORE_FILE).map_err(|e| anyhow!("store: {e}"))?;
    store.set(
        KEY_TIMEOUT,
        serde_json::Value::Number(serde_json::Number::from(secs.max(5))),
    );
    store.save().map_err(|e| anyhow!("store save: {e}"))?;
    Ok(())
}

fn template_script(template: &str, custom_cmd: Option<&str>) -> Result<String> {
    match template {
        "pi" => Ok(String::from(
            "& pi -ne -ns -p --no-tools --system-prompt $env:PARLA_SYSTEM_PROMPT $env:PARLA_USER_PROMPT",
        )),
        "claude" => Ok(String::from("& claude -p $env:PARLA_FULL_PROMPT")),
        "codex" => Ok(String::from(concat!(
            "$t = New-TemporaryFile; ",
            "& codex exec --skip-git-repo-check --output-last-message $t.FullName $env:PARLA_FULL_PROMPT *> $null; ",
            "Get-Content -Raw $t.FullName; ",
            "Remove-Item $t"
        ))),
        "custom" => {
            let cmd = custom_cmd.ok_or_else(|| {
                anyhow!("LocalCLI custom: definissez llm_localcli_custom_cmd")
            })?;
            Ok(cmd.to_string())
        }
        other => Err(anyhow!("LocalCLI template inconnu: {other}")),
    }
}

fn full_prompt(system: &str, user: &str) -> String {
    format!("<SYSTEM_PROMPT>\n{system}\n</SYSTEM_PROMPT>\n\n<USER_PROMPT>\n{user}\n</USER_PROMPT>")
}

#[async_trait]
impl LLMProvider for LocalCLIProvider {
    fn id(&self) -> &'static str {
        "localcli"
    }
    fn label(&self) -> &'static str {
        "Local CLI"
    }
    fn default_models(&self) -> &'static [&'static str] {
        TEMPLATES
    }
    fn default_model(&self) -> &'static str {
        "pi"
    }
    fn endpoint(&self) -> &'static str {
        "powershell.exe -NoProfile -Command"
    }
    fn requires_api_key(&self) -> bool {
        false
    }
    fn rate_limited(&self) -> bool {
        false
    }

    async fn chat_completion(
        &self,
        _api_key: &str,
        req: &EnhancementRequest,
    ) -> Result<EnhancementResponse> {
        // endpoint_override sert ici a passer la commande custom si template=custom.
        let custom_cmd = req.endpoint_override.as_deref();
        let script = template_script(&req.model, custom_cmd)?;
        let fp = full_prompt(&req.system_prompt, &req.user_message);

        // Note : on utilise powershell.exe (v5, installe partout sur Windows 10+).
        // pwsh.exe (PowerShell Core) peut etre absent.
        let mut child = Command::new("powershell.exe")
            .args(["-NoProfile", "-Command", &script])
            .env("PARLA_SYSTEM_PROMPT", &req.system_prompt)
            .env("PARLA_USER_PROMPT", &req.user_message)
            .env("PARLA_FULL_PROMPT", &fp)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| anyhow!("spawn powershell.exe: {e}"))?;

        // Ecrit le full prompt dans stdin (comme VoiceInk) puis ferme.
        if let Some(stdin) = child.stdin.as_mut() {
            let _ = stdin.write_all(fp.as_bytes()).await;
            let _ = stdin.shutdown().await;
        }

        let timeout = Duration::from_secs(req.timeout.as_secs().max(5));
        let output = match tokio::time::timeout(timeout, child.wait_with_output()).await {
            Ok(r) => r.map_err(|e| anyhow!("wait: {e}"))?,
            Err(_) => {
                return Err(anyhow!(
                    "timeout: LocalCLI template {} > {:?}",
                    req.model,
                    timeout
                ))
            }
        };

        let code = output.status.code().unwrap_or(-1);
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if code == 127 || stderr.to_lowercase().contains("is not recognized") {
            return Err(anyhow!(
                "LocalCLI: commande introuvable (template {}) : {stderr}",
                req.model
            ));
        }
        if !output.status.success() {
            return Err(anyhow!(
                "LocalCLI template {} code {} : {stderr}",
                req.model,
                code
            ));
        }
        if stdout.is_empty() {
            return Err(anyhow!(
                "LocalCLI template {} : sortie vide (stderr : {stderr})",
                req.model
            ));
        }
        Ok(EnhancementResponse { text: stdout })
    }
}
