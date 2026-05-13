// Card "Recording shortcut" pour Settings : permet de configurer le
// raccourci primary + secondary qui declenche l'enregistrement.
//
// Reference VoiceInk Views/Settings/SettingsView.swift L40-79 :
//   Section("Shortcuts")
//     LabeledContent("Shortcut 1") + hotkeyModePicker + hotkeyPicker + custom
//     [optional] LabeledContent("Shortcut 2") + meme
//     [if no secondary] Button("Add Second Shortcut")
//
// Trois niveaux de configuration :
//   1. Modifier picker  (Right Alt, Left Ctrl, etc.)
//   2. Mode picker      (Toggle / Push-to-talk / Hybrid)
//   3. Custom combo     (Ctrl+Alt+R, F13, etc.) via HotkeyRecorder dialog
//
// Reset to defaults wipe la config user et remet Right Alt + Hybrid.

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { MinusCircle, RotateCcw } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { formatCombo, HotkeyRecorder } from "@/components/HotkeyRecorder";
import {
  api,
  type HotkeyConfig,
  type HotkeyMode,
  type HotkeyOptionId,
  type HotkeySlotConfig,
  type HotkeyTrigger,
} from "@/lib/tauri";
import { cn } from "@/lib/utils";

const MODIFIER_OPTIONS: HotkeyOptionId[] = [
  "rightAlt",
  "leftAlt",
  "rightCtrl",
  "leftCtrl",
  "rightWin",
  "rightShift",
  "leftShift",
];

const MODES: HotkeyMode[] = ["toggle", "pushToTalk", "hybrid"];

type SelectValue =
  | { kind: "none" }
  | { kind: "modifier"; option: HotkeyOptionId }
  | { kind: "custom" };

function selectValueOf(trigger: HotkeyTrigger): SelectValue {
  switch (trigger.kind) {
    case "none":
      return { kind: "none" };
    case "modifier":
      return { kind: "modifier", option: trigger.option };
    case "combo":
      return { kind: "custom" };
  }
}

function selectValueToString(v: SelectValue): string {
  if (v.kind === "none") return "none";
  if (v.kind === "custom") return "custom";
  return `mod:${v.option}`;
}

function parseSelectValue(s: string): SelectValue {
  if (s === "none") return { kind: "none" };
  if (s === "custom") return { kind: "custom" };
  if (s.startsWith("mod:")) {
    return { kind: "modifier", option: s.slice(4) as HotkeyOptionId };
  }
  return { kind: "none" };
}

export function HotkeyCard() {
  const { t } = useTranslation();
  const [config, setConfig] = useState<HotkeyConfig | null>(null);
  const [showSecondary, setShowSecondary] = useState(false);
  const [recorderTarget, setRecorderTarget] = useState<"primary" | "secondary" | null>(null);

  useEffect(() => {
    api
      .getHotkeyConfig()
      .then((c) => {
        setConfig(c);
        setShowSecondary(c.secondary.trigger.kind !== "none");
      })
      .catch(console.error);
  }, []);

  async function commit(next: HotkeyConfig) {
    setConfig(next);
    try {
      await api.setHotkeyConfig(next);
    } catch (e) {
      console.error(e);
    }
  }

  async function reset() {
    try {
      const c = await api.resetHotkeyConfig();
      setConfig(c);
      setShowSecondary(false);
    } catch (e) {
      console.error(e);
    }
  }

  function updateSlot(slot: "primary" | "secondary", patch: Partial<HotkeySlotConfig>) {
    if (!config) return;
    const next = {
      ...config,
      [slot]: { ...config[slot], ...patch },
    };
    commit(next);
  }

  function onTriggerChange(slot: "primary" | "secondary", raw: string) {
    if (!config) return;
    const value = parseSelectValue(raw);
    if (value.kind === "custom") {
      // Open recorder, will commit when capture finishes.
      setRecorderTarget(slot);
      return;
    }
    if (value.kind === "none") {
      updateSlot(slot, { trigger: { kind: "none" } });
    } else {
      updateSlot(slot, {
        trigger: { kind: "modifier", option: value.option },
      });
    }
  }

  function onComboCaptured(trigger: Extract<HotkeyTrigger, { kind: "combo" }>) {
    if (!config || !recorderTarget) {
      setRecorderTarget(null);
      return;
    }
    updateSlot(recorderTarget, { trigger });
    setRecorderTarget(null);
  }

  function addSecondary() {
    if (!config) return;
    setShowSecondary(true);
    commit({
      ...config,
      secondary: {
        trigger: { kind: "modifier", option: "rightWin" },
        mode: config.primary.mode,
      },
    });
  }

  function removeSecondary() {
    if (!config) return;
    setShowSecondary(false);
    commit({
      ...config,
      secondary: { trigger: { kind: "none" }, mode: config.secondary.mode },
    });
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">{t("hotkey.cardTitle")}</CardTitle>
        <CardDescription>{t("hotkey.cardDescription")}</CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        {!config ? (
          <p className="text-sm text-muted-foreground">{t("common.loading")}</p>
        ) : (
          <>
            <SlotEditor
              label={t("hotkey.primary")}
              slot={config.primary}
              onTriggerSelect={(v) => onTriggerChange("primary", v)}
              onModeSelect={(m) => updateSlot("primary", { mode: m })}
              onEditCombo={() => setRecorderTarget("primary")}
            />

            {showSecondary && (
              <div className="relative">
                <SlotEditor
                  label={t("hotkey.secondary")}
                  slot={config.secondary}
                  onTriggerSelect={(v) => onTriggerChange("secondary", v)}
                  onModeSelect={(m) => updateSlot("secondary", { mode: m })}
                  onEditCombo={() => setRecorderTarget("secondary")}
                />
                <Button
                  size="sm"
                  variant="ghost"
                  className="absolute right-0 top-0 text-muted-foreground"
                  onClick={removeSecondary}
                  title={t("hotkey.removeSecondary")}
                >
                  <MinusCircle className="h-4 w-4" />
                </Button>
              </div>
            )}

            <div className="flex flex-wrap gap-2">
              {!showSecondary && (
                <Button size="sm" variant="outline" onClick={addSecondary}>
                  {t("hotkey.addSecondary")}
                </Button>
              )}
              <Button size="sm" variant="ghost" onClick={reset}>
                <RotateCcw className="h-3.5 w-3.5" />
                {t("hotkey.resetDefaults")}
              </Button>
            </div>

            {config.primary.trigger.kind === "modifier" &&
              config.primary.trigger.option === "rightAlt" && (
                <p className="text-[11px] leading-relaxed text-muted-foreground">
                  {t("hotkey.altGrTip")}
                </p>
              )}
          </>
        )}
      </CardContent>

      <HotkeyRecorder
        open={recorderTarget !== null}
        onCancel={() => setRecorderTarget(null)}
        onCapture={onComboCaptured}
      />
    </Card>
  );
}

function SlotEditor({
  label,
  slot,
  onTriggerSelect,
  onModeSelect,
  onEditCombo,
}: {
  label: string;
  slot: HotkeySlotConfig;
  onTriggerSelect: (v: string) => void;
  onModeSelect: (m: HotkeyMode) => void;
  onEditCombo: () => void;
}) {
  const { t } = useTranslation();
  const value = selectValueOf(slot.trigger);
  const valueStr = selectValueToString(value);

  return (
    <div className="rounded-md border p-3">
      <p className="mb-2 text-sm font-medium">{label}</p>
      <div className="grid grid-cols-1 gap-2 sm:grid-cols-[1fr,1fr,auto]">
        <select
          value={valueStr}
          onChange={(e) => onTriggerSelect(e.target.value)}
          className="h-9 rounded-md border border-input bg-background px-2 text-sm"
        >
          <option value="none">{t("hotkey.options.none")}</option>
          {MODIFIER_OPTIONS.map((opt) => (
            <option key={opt} value={`mod:${opt}`}>
              {t(`hotkey.options.${opt}`)}
            </option>
          ))}
          <option value="custom">{t("hotkey.options.custom")}</option>
        </select>

        <select
          value={slot.mode}
          onChange={(e) => onModeSelect(e.target.value as HotkeyMode)}
          disabled={slot.trigger.kind === "none"}
          className={cn(
            "h-9 rounded-md border border-input bg-background px-2 text-sm",
            slot.trigger.kind === "none" && "opacity-50",
          )}
        >
          {MODES.map((m) => (
            <option key={m} value={m}>
              {t(`hotkey.modes.${m}`)}
            </option>
          ))}
        </select>

        {slot.trigger.kind === "combo" && (
          <button
            type="button"
            onClick={onEditCombo}
            title={t("hotkey.editCombo")}
            className="flex h-9 items-center rounded-md border bg-muted/30 px-2 font-mono text-xs transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
          >
            {formatCombo(slot.trigger)}
          </button>
        )}
        {slot.trigger.kind === "modifier" && (
          <span className="flex h-9 items-center px-2 text-xs text-muted-foreground">
            {`VK ${vkOf(slot.trigger.option) ?? "-"}`}
          </span>
        )}
      </div>
      <p className="mt-2 text-[11px] leading-relaxed text-muted-foreground">
        {t(`hotkey.modeHints.${slot.mode}`)}
      </p>
    </div>
  );
}

function vkOf(option: HotkeyOptionId): string | null {
  const TABLE: Record<HotkeyOptionId, number | null> = {
    none: null,
    rightAlt: 0xa5,
    leftAlt: 0xa4,
    rightCtrl: 0xa3,
    leftCtrl: 0xa2,
    rightWin: 0x5c,
    rightShift: 0xa1,
    leftShift: 0xa0,
  };
  const n = TABLE[option];
  return n === null ? null : `0x${n.toString(16).toUpperCase()}`;
}
