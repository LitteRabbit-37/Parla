// Preferences globales Parla (equivalent VoiceInk Settings page).
//
// Reference VoiceInk Views/Settings/SettingsView.swift : Form(.grouped)
// avec sections Shortcuts / Additional Shortcuts / Power Mode /
// Recording Feedback / Interface / Experimental / General / Privacy /
// Backup / Diagnostics. Sur Parla on regroupe le minimum vital en
// attendant un decoupage plus fin.

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { HotkeyCard } from "@/components/HotkeyCard";
import { Button } from "@/components/ui/button";
import { InfoTip } from "@/components/ui/info-tip";
import {
  LANGUAGE_LABELS,
  SUPPORTED_LANGUAGES,
  type SupportedLanguage,
} from "@/i18n";
import { api } from "@/lib/tauri";
import { cn } from "@/lib/utils";

export function SettingsPanel() {
  const { t, i18n } = useTranslation();
  const [recorderStyle, setRecorderStyle] = useState<"mini" | "notch">("mini");
  const [autostart, setAutostart] = useState(false);
  const [closeToTray, setCloseToTray] = useState(true);
  const [systemMute, setSystemMute] = useState(false);
  const [resumeDelay, setResumeDelay] = useState(0.2);
  const [soundFeedback, setSoundFeedback] = useState(true);
  useEffect(() => {
    api
      .getRecorderStyle()
      .then((s) => setRecorderStyle(s === "notch" ? "notch" : "mini"))
      .catch(console.error);
    api
      .checkPermissions()
      .then((p) => setAutostart(p.autostart.ok))
      .catch(console.error);
    api.getCloseToTray().then(setCloseToTray).catch(console.error);
    api.getSystemMuteEnabled().then(setSystemMute).catch(console.error);
    api.getAudioResumptionDelay().then(setResumeDelay).catch(console.error);
    api.getSoundFeedbackEnabled().then(setSoundFeedback).catch(console.error);
  }, []);

  async function changeStyle(next: "mini" | "notch") {
    setRecorderStyle(next);
    try {
      await api.setRecorderStyle(next);
    } catch (e) {
      console.error(e);
    }
  }

  async function changeLanguage(lng: SupportedLanguage) {
    await i18n.changeLanguage(lng);
  }

  async function toggleAutostart(next: boolean) {
    setAutostart(next);
    try {
      await api.setAutostartEnabled(next);
    } catch (e) {
      console.error(e);
      setAutostart(!next);
    }
  }

  async function toggleCloseToTray(next: boolean) {
    setCloseToTray(next);
    try {
      await api.setCloseToTray(next);
    } catch (e) {
      console.error(e);
      setCloseToTray(!next);
    }
  }

  async function toggleSystemMute(next: boolean) {
    setSystemMute(next);
    try {
      await api.setSystemMuteEnabled(next);
    } catch (e) {
      console.error(e);
      setSystemMute(!next);
    }
  }

  async function toggleSoundFeedback(next: boolean) {
    setSoundFeedback(next);
    try {
      await api.setSoundFeedbackEnabled(next);
    } catch (e) {
      console.error(e);
      setSoundFeedback(!next);
    }
  }

  async function saveResumeDelay(secs: number) {
    const clamped = Math.max(0, Math.min(10, secs));
    setResumeDelay(clamped);
    try {
      await api.setAudioResumptionDelay(clamped);
    } catch (e) {
      console.error(e);
    }
  }

  return (
    <div className="space-y-4">
      <Card>
        <CardHeader>
          <CardTitle className="text-base">{t("settings.language")}</CardTitle>
          <CardDescription>{t("settings.languageDescription")}</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="flex flex-wrap gap-2">
            {SUPPORTED_LANGUAGES.map((lng) => {
              const active = i18n.resolvedLanguage === lng;
              return (
                <button
                  key={lng}
                  type="button"
                  onClick={() => changeLanguage(lng)}
                  className={cn(
                    "rounded-md border px-3 py-1.5 text-sm transition-colors",
                    active
                      ? "border-primary bg-primary/10 text-foreground"
                      : "text-muted-foreground hover:bg-accent/60 hover:text-foreground",
                  )}
                >
                  {LANGUAGE_LABELS[lng]}
                </button>
              );
            })}
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-base">{t("settings.general")}</CardTitle>
          <CardDescription>{t("settings.generalDescription")}</CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          <label className="flex items-center justify-between rounded-md border p-3">
            <div>
              <p className="text-sm font-medium">{t("settings.autostart")}</p>
              <p className="text-xs text-muted-foreground">
                {t("settings.autostartDescription")}
              </p>
            </div>
            <input
              type="checkbox"
              checked={autostart}
              onChange={(e) => toggleAutostart(e.target.checked)}
              className="h-5 w-5"
            />
          </label>

          <label className="flex items-center justify-between rounded-md border p-3">
            <div>
              <p className="text-sm font-medium">{t("settings.closeToTray")}</p>
              <p className="text-xs text-muted-foreground">
                {t("settings.closeToTrayDescription")}
              </p>
            </div>
            <input
              type="checkbox"
              checked={closeToTray}
              onChange={(e) => toggleCloseToTray(e.target.checked)}
              className="h-5 w-5"
            />
          </label>
        </CardContent>
      </Card>

      <HotkeyCard />

      <Card>
        <CardHeader>
          <CardTitle className="text-base">{t("settings.recording")}</CardTitle>
          <CardDescription>
            {t("settings.recordingDescription")}
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          <label className="flex items-center justify-between rounded-md border p-3">
            <div>
              <p className="text-sm font-medium">
                {t("settings.soundFeedback")}
              </p>
              <p className="text-xs text-muted-foreground">
                {t("settings.soundFeedbackDescription")}
              </p>
            </div>
            <input
              type="checkbox"
              checked={soundFeedback}
              onChange={(e) => toggleSoundFeedback(e.target.checked)}
              className="h-5 w-5"
            />
          </label>

          <label className="flex items-center justify-between rounded-md border p-3">
            <div>
              <p className="text-sm font-medium">
                {t("settings.systemMute")}
              </p>
              <p className="text-xs text-muted-foreground">
                {t("settings.systemMuteDescription")}
              </p>
            </div>
            <input
              type="checkbox"
              checked={systemMute}
              onChange={(e) => toggleSystemMute(e.target.checked)}
              className="h-5 w-5"
            />
          </label>

          <div
            className={cn(
              "rounded-md border p-3",
              !systemMute && "opacity-50",
            )}
          >
            <p className="text-sm font-medium">
              {t("settings.resumeDelay")}
            </p>
            <p className="text-xs text-muted-foreground">
              {t("settings.resumeDelayDescription")}
            </p>
            <div className="mt-2 flex items-center gap-2">
              <input
                type="number"
                min={0}
                max={10}
                step={0.1}
                value={resumeDelay}
                onChange={(e) =>
                  setResumeDelay(
                    Number.isFinite(e.target.valueAsNumber)
                      ? e.target.valueAsNumber
                      : 0,
                  )
                }
                onBlur={(e) => saveResumeDelay(e.target.valueAsNumber || 0)}
                disabled={!systemMute}
                className="h-9 w-24 rounded-md border border-input bg-background px-3 text-sm"
              />
              <span className="text-xs text-muted-foreground">
                {t("common.seconds")}
              </span>
            </div>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <div className="flex items-center gap-2">
            <CardTitle className="text-base">
              {t("settings.recorderStyle")}
            </CardTitle>
            <InfoTip>{t("settings.recorderStyleInfo")}</InfoTip>
          </div>
          <CardDescription>
            {t("settings.recorderStyleDescription")}
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-2">
          <div className="grid grid-cols-2 gap-2">
            <StyleTile
              active={recorderStyle === "mini"}
              label={t("settings.recorderStyleMini")}
              caption={t("settings.recorderStyleMiniCaption")}
              onClick={() => changeStyle("mini")}
              orientation="bottom"
            />
            <StyleTile
              active={recorderStyle === "notch"}
              label={t("settings.recorderStyleNotch")}
              caption={t("settings.recorderStyleNotchCaption")}
              onClick={() => changeStyle("notch")}
              orientation="top"
            />
          </div>
        </CardContent>
      </Card>

    </div>
  );
}

export function ApplicationProxyCard() {
  const [proxy, setProxy] = useState({ enabled: false, url: "", noProxyEntries: "" });
  const [proxyCredentials, setProxyCredentials] = useState({ username: "", password: "" });
  const [hasProxyCredentials, setHasProxyCredentials] = useState(false);
  const [proxyLoading, setProxyLoading] = useState(true);
  const [proxySaving, setProxySaving] = useState(false);
  const [credentialsSaving, setCredentialsSaving] = useState(false);
  const [proxyError, setProxyError] = useState<string | null>(null);
  const [proxyNotice, setProxyNotice] = useState<string | null>(null);

  useEffect(() => {
    Promise.all([api.getProxySettings(), api.hasProxyCredentials()])
      .then(([settings, credentials]) => {
        setProxy({ enabled: settings.enabled, url: settings.url ?? "", noProxyEntries: settings.no_proxy_entries.join(", ") });
        setHasProxyCredentials(credentials);
      })
      .catch(() => setProxyError("Could not load Application Proxy settings."))
      .finally(() => setProxyLoading(false));
  }, []);

  function validateProxy(): string | null {
    const url = proxy.url.trim();
    if (!proxy.enabled || url.length === 0) return null;
    try {
      const parsed = new URL(url);
      if (!["http:", "https:", "socks5:"].includes(parsed.protocol)) return "Application Proxy URL must use http, https, or socks5.";
      if (!parsed.hostname) return "Enter a valid Application Proxy URL.";
      if (parsed.username || parsed.password) return "Enter Application Proxy credentials in credential fields, not in URL.";
    } catch {
      return "Enter a valid Application Proxy URL.";
    }
    return null;
  }

  function normalizedNoProxyEntries(): string[] {
    return proxy.noProxyEntries.split(",").map((entry) => entry.trim()).filter(Boolean);
  }

  async function saveProxy() {
    setProxyError(null);
    setProxyNotice(null);
    const validationError = validateProxy();
    if (validationError) { setProxyError(validationError); return; }
    setProxySaving(true);
    try {
      await api.setProxySettings({ enabled: proxy.enabled, url: proxy.url.trim() || null, no_proxy_entries: normalizedNoProxyEntries() });
      setProxy({ ...proxy, noProxyEntries: normalizedNoProxyEntries().join(", ") });
      setProxyNotice("Application Proxy settings saved.");
    } catch { setProxyError("Could not save Application Proxy settings."); }
    finally { setProxySaving(false); }
  }

  async function saveProxyCredentials() {
    setProxyError(null);
    setProxyNotice(null);
    if (!proxyCredentials.username.trim() || !proxyCredentials.password) { setProxyError("Enter both username and password to save credentials."); return; }
    setCredentialsSaving(true);
    try {
      await api.setProxyCredentials({ username: proxyCredentials.username.trim(), password: proxyCredentials.password });
      setProxyCredentials({ username: "", password: "" });
      setHasProxyCredentials(true);
      setProxyNotice("Application Proxy credentials saved securely.");
    } catch { setProxyError("Could not save Application Proxy credentials."); }
    finally { setCredentialsSaving(false); }
  }

  async function removeProxyCredentials() {
    setProxyError(null);
    setProxyNotice(null);
    try {
      await api.deleteProxyCredentials();
      setHasProxyCredentials(false);
      setProxyNotice("Application Proxy credentials removed.");
    } catch { setProxyError("Could not remove Application Proxy credentials."); }
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">Application Proxy</CardTitle>
        <CardDescription>
          Route Parla outbound HTTP(S) traffic through an Application Proxy. Leave URL blank to use Windows system proxy settings.
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        {proxyLoading ? <p className="text-sm text-muted-foreground">Loading Application Proxy settings…</p> : (
          <>
            <label className="flex items-center justify-between rounded-md border p-3"><div><p className="text-sm font-medium">Enable Application Proxy</p><p className="text-xs text-muted-foreground">No destinations bypass it unless listed as No-Proxy Entries.</p></div><input type="checkbox" checked={proxy.enabled} onChange={(event) => setProxy({ ...proxy, enabled: event.target.checked })} className="h-5 w-5" /></label>
            <div className={cn("space-y-1", !proxy.enabled && "opacity-60")}><label htmlFor="application-proxy-url" className="text-sm font-medium">Application Proxy URL</label><input id="application-proxy-url" type="url" value={proxy.url} onChange={(event) => setProxy({ ...proxy, url: event.target.value })} placeholder="https://proxy.example:8080" disabled={!proxy.enabled} className="h-9 w-full rounded-md border border-input bg-background px-3 text-sm" /><p className="text-xs text-muted-foreground">Supported schemes: http, https, socks5.</p></div>
            <div className={cn("space-y-1", !proxy.enabled && "opacity-60")}><label htmlFor="no-proxy-entries" className="text-sm font-medium">No-Proxy Entries</label><textarea id="no-proxy-entries" value={proxy.noProxyEntries} onChange={(event) => setProxy({ ...proxy, noProxyEntries: event.target.value })} placeholder="localhost, 127.0.0.1, .example.com, 10.0.0.0/8" disabled={!proxy.enabled} rows={3} className="w-full resize-y rounded-md border border-input bg-background px-3 py-2 text-sm" /><p className="text-xs text-muted-foreground">Comma-separated NO_PROXY-compatible destination patterns.</p></div>
            <div className="rounded-md border p-3"><div className="flex flex-wrap items-start justify-between gap-3"><div><p className="text-sm font-medium">Application Proxy credentials</p><p className="text-xs text-muted-foreground">{hasProxyCredentials ? "Credentials saved in the operating-system credential vault." : "No credentials saved."}</p></div>{hasProxyCredentials && <Button type="button" variant="outline" size="sm" onClick={removeProxyCredentials}>Remove credentials</Button>}</div><div className="mt-3 grid gap-2 sm:grid-cols-2"><input aria-label="Application Proxy username" value={proxyCredentials.username} onChange={(event) => setProxyCredentials({ ...proxyCredentials, username: event.target.value })} placeholder="Username" autoComplete="username" className="h-9 rounded-md border border-input bg-background px-3 text-sm" /><input aria-label="Application Proxy password" type="password" value={proxyCredentials.password} onChange={(event) => setProxyCredentials({ ...proxyCredentials, password: event.target.value })} placeholder="Password" autoComplete="new-password" className="h-9 rounded-md border border-input bg-background px-3 text-sm" /></div><Button type="button" variant="outline" size="sm" className="mt-3" onClick={saveProxyCredentials} disabled={credentialsSaving}>{credentialsSaving ? "Saving…" : "Save credentials"}</Button></div>
            {(proxyError || proxyNotice) && <p role="status" className={cn("text-sm", proxyError ? "text-destructive" : "text-muted-foreground")}>{proxyError ?? proxyNotice}</p>}
            <Button type="button" onClick={saveProxy} disabled={proxySaving}>{proxySaving ? "Saving…" : "Save Application Proxy settings"}</Button>
          </>
        )}
      </CardContent>
    </Card>
  );
}

function StyleTile({
  active,
  label,
  caption,
  orientation,
  onClick,
}: {
  active: boolean;
  label: string;
  caption: string;
  orientation: "top" | "bottom";
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "rounded-lg border p-3 text-left transition-colors",
        active
          ? "border-primary bg-primary/5"
          : "hover:border-accent hover:bg-accent/30",
      )}
    >
      <div className="flex h-16 items-center justify-center rounded-md border border-dashed bg-muted/40">
        <span
          className={cn(
            "h-3 w-16 bg-black",
            orientation === "top"
              ? "self-start rounded-b-md rounded-t-none"
              : "self-end rounded-md",
          )}
          style={
            orientation === "top"
              ? { marginTop: 0 }
              : { marginBottom: 4 }
          }
        />
      </div>
      <p className="mt-2 text-sm font-medium">{label}</p>
      <p className="text-xs text-muted-foreground">{caption}</p>
    </button>
  );
}
