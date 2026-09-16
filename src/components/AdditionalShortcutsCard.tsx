// Card "Additional shortcuts" : raccourcis globaux utilitaires.
//
// Reference VoiceInk Features/Settings/Views/SettingsView.swift
// Section("Additional Shortcuts") :
//   Paste Last Transcription (Original) / (Enhanced), Retry Last
//   Transcription, Cancel Recording (defaut double-Echap + bouton reset).
// Plus "Open History" (VoiceInk .openHistoryWindow) et un ajout Parla :
// "Copy last transcription" (VoiceInk n'a qu'un accelerateur de menu
// Shift+Cmd+C, actif menu ouvert seulement).
//
// Chaque ligne = libelle + bouton keycap (ouvre HotkeyRecorder) + bouton
// effacer. La config complete est relue avant chaque ecriture pour ne pas
// ecraser les slots primary/secondary geres par HotkeyCard.

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { RotateCcw, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { InfoTip } from "@/components/ui/info-tip";
import { formatCombo, HotkeyRecorder } from "@/components/HotkeyRecorder";
import {
  api,
  type HotkeyConfig,
  type HotkeyTrigger,
  type UtilityShortcuts,
} from "@/lib/tauri";
import { translateError } from "@/lib/translateError";
import { cn } from "@/lib/utils";

type ActionKey = keyof UtilityShortcuts;

const ROWS: Array<{ key: ActionKey; labelKey: string; hintKey?: string }> = [
  {
    key: "copy_last_transcription",
    labelKey: "hotkey.additional.copyLast",
    hintKey: "hotkey.additional.copyLastHint",
  },
  { key: "paste_last_transcription", labelKey: "hotkey.additional.pasteLast" },
  {
    key: "paste_last_enhancement",
    labelKey: "hotkey.additional.pasteLastEnhanced",
  },
  {
    key: "retry_last_transcription",
    labelKey: "hotkey.additional.retryLast",
    hintKey: "hotkey.additional.retryLastHint",
  },
  { key: "open_history", labelKey: "hotkey.additional.openHistory" },
  {
    key: "cancel_recording",
    labelKey: "hotkey.additional.cancelRecording",
    hintKey: "hotkey.additional.cancelHint",
  },
];

export function triggerLabel(
  t: (k: string) => string,
  trigger: HotkeyTrigger,
): string | null {
  switch (trigger.kind) {
    case "none":
      return null;
    case "modifier":
      return t(`hotkey.options.${trigger.option}`);
    case "combo":
      return formatCombo(trigger);
  }
}

export function AdditionalShortcutsCard() {
  const { t } = useTranslation();
  const [config, setConfig] = useState<HotkeyConfig | null>(null);
  const [target, setTarget] = useState<ActionKey | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.getHotkeyConfig().then(setConfig).catch(console.error);
  }, []);

  async function commitAction(key: ActionKey, trigger: HotkeyTrigger) {
    setError(null);
    try {
      // Relit la config courante : HotkeyCard peut avoir modifie les slots
      // d'enregistrement entre temps.
      const current = await api.getHotkeyConfig();
      const next: HotkeyConfig = {
        ...current,
        actions: { ...current.actions, [key]: trigger },
      };
      await api.setHotkeyConfig(next);
      setConfig(next);
    } catch (e) {
      setError(translateError(t, String(e)));
      api.getHotkeyConfig().then(setConfig).catch(console.error);
    }
  }

  async function resetCancel() {
    await commitAction("cancel_recording", { kind: "none" });
    try {
      await api.resetEscapeHint();
    } catch (e) {
      console.error(e);
    }
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">
          {t("hotkey.additional.cardTitle")}
        </CardTitle>
        <CardDescription>{t("hotkey.additional.cardDescription")}</CardDescription>
      </CardHeader>
      <CardContent className="space-y-2">
        {!config ? (
          <p className="text-sm text-muted-foreground">{t("common.loading")}</p>
        ) : (
          ROWS.map((row) => {
            const trigger = config.actions[row.key];
            const isCancel = row.key === "cancel_recording";
            const label = triggerLabel(t, trigger);
            return (
              <div
                key={row.key}
                className="flex flex-wrap items-center justify-between gap-2 rounded-md border p-3"
              >
                <div className="flex items-center gap-1.5">
                  <p className="text-sm font-medium">{t(row.labelKey)}</p>
                  {row.hintKey && <InfoTip>{t(row.hintKey)}</InfoTip>}
                </div>
                <div className="flex items-center gap-1.5">
                  <button
                    type="button"
                    onClick={() => setTarget(row.key)}
                    title={t("hotkey.additional.record")}
                    className={cn(
                      "flex h-8 min-w-[104px] items-center justify-center rounded-md border px-2 font-mono text-xs transition-colors hover:bg-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
                      label ? "bg-muted/30" : "text-muted-foreground",
                    )}
                  >
                    {label ??
                      (isCancel
                        ? t("hotkey.additional.cancelDefault")
                        : t("hotkey.additional.notSet"))}
                  </button>
                  {isCancel ? (
                    <Button
                      size="sm"
                      variant="ghost"
                      className="h-8 px-2 text-muted-foreground"
                      onClick={resetCancel}
                      title={t("hotkey.additional.reset")}
                    >
                      <RotateCcw className="h-3.5 w-3.5" />
                    </Button>
                  ) : (
                    <Button
                      size="sm"
                      variant="ghost"
                      className="h-8 px-2 text-muted-foreground"
                      disabled={trigger.kind === "none"}
                      onClick={() => commitAction(row.key, { kind: "none" })}
                      title={t("hotkey.additional.clear")}
                    >
                      <X className="h-3.5 w-3.5" />
                    </Button>
                  )}
                </div>
              </div>
            );
          })
        )}

        {error && (
          <p className="rounded-md bg-destructive/10 p-2 text-xs text-destructive">
            {error}
          </p>
        )}
      </CardContent>

      <HotkeyRecorder
        open={target !== null}
        onCancel={() => setTarget(null)}
        onCapture={(trigger) => {
          const key = target;
          setTarget(null);
          if (key) commitAction(key, trigger);
        }}
      />
    </Card>
  );
}
