import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { Mic, Square, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { AudioMeterBar } from "@/components/AudioMeterBar";
import {
  api,
  type AudioDeviceInfo,
  type AudioMeter,
  type RecordingStarted,
  type RecordingStopped,
} from "@/lib/tauri";

export function RecorderPanel() {
  const { t } = useTranslation();
  const [devices, setDevices] = useState<AudioDeviceInfo[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [isRecording, setIsRecording] = useState(false);
  const [meter, setMeter] = useState<AudioMeter>({ rms_db: -160, peak_db: -160 });
  const [lastPath, setLastPath] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [hotkeyAction, setHotkeyAction] = useState<string | null>(null);
  const pollRef = useRef<number | null>(null);

  useEffect(() => {
    refreshDevices();
    const unlistenPromises = [
      listen<RecordingStarted>("recording:started", (e) => {
        setIsRecording(true);
        setLastPath(null);
        startPolling();
        void e;
      }),
      listen<RecordingStopped>("recording:stopped", (e) => {
        setIsRecording(false);
        stopPolling();
        setLastPath(e.payload.wav_path);
      }),
      listen("recording:cancelled", () => {
        setIsRecording(false);
        stopPolling();
      }),
      listen<string>("hotkey:action", (e) => {
        setHotkeyAction(e.payload);
        window.setTimeout(() => setHotkeyAction(null), 1500);
      }),
    ];
    return () => {
      stopPolling();
      Promise.all(unlistenPromises).then((arr) => arr.forEach((fn) => fn()));
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  function startPolling() {
    if (pollRef.current !== null) return;
    pollRef.current = window.setInterval(async () => {
      try {
        const m = await api.getAudioMeter();
        setMeter(m);
      } catch {
        // ignore
      }
    }, 50);
  }

  function stopPolling() {
    if (pollRef.current !== null) {
      window.clearInterval(pollRef.current);
      pollRef.current = null;
    }
    setMeter({ rms_db: -160, peak_db: -160 });
  }

  async function refreshDevices() {
    try {
      const [list, stored] = await Promise.all([
        api.listAudioDevices(),
        api.getSelectedInputDevice(),
      ]);
      setDevices(list);
      // Micro persiste (reglages / menu tray) s'il est toujours branche,
      // sinon defaut systeme (VoiceInk AudioDeviceManager fallback).
      if (stored && list.some((d) => d.name === stored)) {
        setSelected(stored);
      } else {
        setSelected(null);
      }
    } catch (e) {
      setError(String(e));
    }
  }

  async function chooseDevice(name: string | null) {
    setSelected(name);
    try {
      await api.setSelectedInputDevice(name);
    } catch (e) {
      setError(String(e));
    }
  }

  async function start() {
    setError(null);
    setLastPath(null);
    try {
      await api.startRecording(selected);
    } catch (e) {
      setError(String(e));
    }
  }

  async function stop() {
    try {
      await api.stopRecording();
    } catch (e) {
      setError(String(e));
    }
  }

  async function cancel() {
    try {
      await api.cancelRecording();
    } catch (e) {
      setError(String(e));
    }
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">{t("recorder.recordingTitle")}</CardTitle>
        <CardDescription>{t("recorder.recordingDescription")}</CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        <div>
          <label className="mb-1.5 block text-sm font-medium">{t("recorder.device")}</label>
          <div className="flex gap-2">
            <select
              value={selected ?? ""}
              onChange={(e) => chooseDevice(e.target.value || null)}
              disabled={isRecording}
              className="flex h-9 flex-1 rounded-md border border-input bg-background px-3 py-1 text-sm shadow-sm disabled:opacity-50"
            >
              {devices.length === 0 && <option value="">{t("recorder.noDevice")}</option>}
              {devices.length > 0 && (
                <option value="">{t("recorder.systemDefault")}</option>
              )}
              {devices.map((d) => (
                <option key={d.name} value={d.name}>
                  {d.name} {d.is_default ? t("recorder.defaultLabel") : ""} - {d.default_sample_rate} Hz /{" "}
                  {d.default_channels} ch
                </option>
              ))}
            </select>
            <Button variant="outline" size="sm" onClick={refreshDevices} disabled={isRecording}>
              {t("recorder.refresh")}
            </Button>
          </div>
        </div>

        <AudioMeterBar meter={meter} />

        <div className="flex items-center gap-2">
          {isRecording ? (
            <>
              <Button variant="destructive" onClick={stop}>
                <Square /> {t("recorder.stop")}
              </Button>
              <Button variant="outline" onClick={cancel}>
                <X /> {t("recorder.cancel")}
              </Button>
              <span className="ml-2 flex items-center gap-1.5 text-sm text-muted-foreground">
                <span className="h-2 w-2 animate-pulse rounded-full bg-red-500" /> {t("recorder.recording")}
              </span>
            </>
          ) : (
            <Button onClick={start}>
              <Mic /> {t("recorder.record")}
            </Button>
          )}
          {hotkeyAction && (
            <span className="ml-auto rounded-full bg-primary/10 px-3 py-1 text-xs font-medium text-primary">
              {t("recorder.hotkeyBadge", { action: hotkeyAction })}
            </span>
          )}
        </div>

        {lastPath && (
          <div className="rounded-md bg-muted p-3 text-xs">
            <p className="font-medium">{t("recorder.lastRecording")}</p>
            <code className="break-all text-muted-foreground">{lastPath}</code>
          </div>
        )}

        {error && (
          <div className="rounded-md bg-destructive/10 p-3 text-sm text-destructive">
            {t("recorder.errorPrefix", { message: error })}
          </div>
        )}
      </CardContent>
    </Card>
  );
}
