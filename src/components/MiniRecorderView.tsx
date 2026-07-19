// Mini-recorder : overlay 300x120 affichee dans la fenetre "recorder".
//
// Reference VoiceInk MiniRecorderView.swift + RecorderComponents.swift +
// AudioVisualizerView.swift :
//   HStack [PromptButton 22pt] Spacer [StatusDisplay] Spacer [PowerModeButton 22pt]
//   content-height 40pt epingle bas, background Color.black opaque,
//   corner radius 20 (collapse) / 14 (expanded live).
//   15 bars audio avec wave + center boost, 60 FPS.
//   Processing: "Transcribing" / "Enhancing" + 5 dots animes.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { Check, Settings as SettingsIcon, Sparkles } from "lucide-react";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import {
  api,
  type AudioMeter,
  type CustomPrompt,
  type PowerModeConfig,
  type PowerSession,
} from "@/lib/tauri";
import { cn, powerShortcutLabel } from "@/lib/utils";

const COLLAPSED_HEIGHT = 120;
const POPOVER_HEIGHT = 400;

type Stage = "idle" | "recording" | "transcribing" | "enhancing";

type PipelineEvent = {
  state: "transcribing" | "enhancing" | "pasting" | "done" | "failed";
  message: string | null;
  text: string | null;
  duration_ms: number | null;
};

type StreamingEvent =
  | { kind: "session_started" }
  | { kind: "partial"; text: string }
  | { kind: "committed"; text: string }
  | { kind: "error"; message: string };

const BARS = 15;

export function MiniRecorderView() {
  const [stage, setStage] = useState<Stage>("recording");
  const [meterDb, setMeterDb] = useState<number>(-160);
  const [partial, setPartial] = useState<string | null>(null);
  const [powerSession, setPowerSession] = useState<PowerSession | null>(null);

  // Power Mode / Prompts state (pour les popovers).
  const [prompts, setPrompts] = useState<CustomPrompt[]>([]);
  const [activePromptId, setActivePromptId] = useState<string | null>(null);
  const [enhancementEnabled, setEnhancementEnabled] = useState(false);
  const [powerConfigs, setPowerConfigs] = useState<PowerModeConfig[]>([]);
  const [style, setStyle] = useState<"mini" | "notch">("mini");

  // Active popover id. The mini recorder window is only 120 px tall, so a
  // DOM popover has no room to render without being clipped. We track which
  // popover is open and ask the Tauri backend to temporarily grow the window
  // (keeping the pill anchored to its bottom or top edge). Restored when
  // the popover closes.
  //
  // Open sequence :
  //   1. User clicks button -> Radix asks onOpenChange(true).
  //   2. We resize the OS window to 400 px BEFORE rendering the popover
  //      content, wait a frame for layout, then flip our state open=true.
  //      Radix then measures the (now-larger) window and places the
  //      content without flashing in the clipped area.
  //   3. On close, we let Radix render the close animation, then shrink
  //      the window back.
  const [popoverOpen, setPopoverOpen] = useState<"prompt" | "power" | null>(
    null,
  );
  const handlePopoverChange = useCallback(
    (id: "prompt" | "power") => async (open: boolean) => {
      if (open) {
        await api.resizeRecorderWindow(POPOVER_HEIGHT).catch(console.error);
        // One frame so the OS actually repaints the new window size before
        // Radix positions the PopoverContent against the trigger.
        await new Promise((r) => setTimeout(r, 32));
        setPopoverOpen(id);
      } else {
        setPopoverOpen(null);
        await api
          .resizeRecorderWindow(COLLAPSED_HEIGHT)
          .catch(console.error);
      }
    },
    [],
  );
  // Helper the PowerMode popover calls when the user clicks "Ouvrir
  // Power Mode" - closes the popover + resizes back so we don't leave
  // an inflated invisible window.
  const closePopover = useCallback(async () => {
    setPopoverOpen(null);
    await api.resizeRecorderWindow(COLLAPSED_HEIGHT).catch(console.error);
  }, []);

  useEffect(() => {
    // Charge une fois au mount - la fenetre mini-recorder est recreee a
    // chaque start donc pas besoin de re-fetch frequent.
    Promise.all([
      api.listPrompts(),
      api.getActivePromptId(),
      api.getEnhancementEnabled(),
      api.listPowerConfigs(),
      api.getRecorderStyle(),
    ])
      .then(([ps, aid, en, pcs, rs]) => {
        setPrompts(ps);
        setActivePromptId(aid);
        setEnhancementEnabled(en);
        setPowerConfigs(pcs);
        setStyle(rs === "notch" ? "notch" : "mini");
      })
      .catch(console.error);

    const unlistens = [
      listen("recording:stopped", () => setStage("transcribing")),
      listen("recording:cancelled", () => setStage("idle")),
      listen<PipelineEvent>("pipeline:state", (e) => {
        const p = e.payload;
        if (p.state === "transcribing") setStage("transcribing");
        else if (p.state === "enhancing") setStage("enhancing");
        // pasting / done / failed -> VoiceInk dismiss directement sans
        // afficher de badge. On laisse le stage precedent jusqu'au close.
      }),
      listen<StreamingEvent>("streaming:event", (e) => {
        const s = e.payload;
        if (s.kind === "partial" || s.kind === "committed") {
          setPartial((prev) => (prev === s.text ? prev : s.text));
        }
      }),
      listen<PowerSession | null>("power_mode:active", (e) => {
        setPowerSession(e.payload);
      }),
    ];
    return () => {
      Promise.all(unlistens).then((arr) => arr.forEach((fn) => fn()));
    };
  }, []);

  // Polling du meter pendant l'enregistrement uniquement.
  useEffect(() => {
    if (stage !== "recording") return;
    let raf = 0;
    const tick = async () => {
      try {
        const m: AudioMeter = await api.getAudioMeter();
        setMeterDb(m.peak_db);
      } catch {
        // ignore
      }
      raf = window.requestAnimationFrame(tick);
    };
    raf = window.requestAnimationFrame(tick);
    return () => window.cancelAnimationFrame(raf);
  }, [stage]);

  const hasLiveText = partial !== null && partial.length > 0;
  const expanded = stage === "transcribing" && hasLiveText;
  const isNotch = style === "notch";

  // Notch : content aligne en haut, pill qui descend du bord superieur
  // avec top-flat + bottom-rounded (VoiceInk NotchShape). Mini : content
  // aligne en bas, pill pleinement arrondi.
  const containerAlign = isNotch ? "items-start" : "items-end";
  const spacingStyle = isNotch
    ? ({ marginTop: 0 } as const)
    : ({ marginBottom: 24 } as const);
  const shape = isNotch
    ? expanded
      ? "rounded-b-[22px] rounded-t-none"
      : "rounded-b-[16px] rounded-t-none"
    : expanded
      ? "rounded-[14px]"
      : "rounded-[20px]";

  return (
    <div
      className={cn(
        "flex h-screen w-screen justify-center bg-transparent",
        containerAlign,
      )}
    >
      <div
        className={cn(
          "flex h-10 items-center bg-black text-white shadow-lg transition-all duration-300 ease-in-out",
          expanded ? "w-[300px]" : "w-[184px]",
          shape,
        )}
        style={spacingStyle}
      >
        <RecorderPromptButton
          open={popoverOpen === "prompt"}
          onOpenChange={handlePopoverChange("prompt")}
          popoverSide={isNotch ? "bottom" : "top"}
          enabled={enhancementEnabled}
          prompts={prompts}
          activeId={activePromptId}
          onToggleEnhancement={async () => {
            const next = !enhancementEnabled;
            setEnhancementEnabled(next);
            try {
              await api.setEnhancementEnabled(next);
            } catch (e) {
              console.error(e);
            }
          }}
          onSelectPrompt={async (id) => {
            setActivePromptId(id);
            try {
              await api.setActivePromptId(id);
              if (!enhancementEnabled) {
                setEnhancementEnabled(true);
                await api.setEnhancementEnabled(true);
              }
            } catch (e) {
              console.error(e);
            }
          }}
        />

        <div className="flex flex-1 items-center justify-center overflow-hidden px-1">
          {stage === "recording" && (
            <AudioVisualizer meterDb={meterDb} />
          )}
          {stage === "transcribing" && !hasLiveText && (
            <ProcessingStatusDisplay label="Transcribing" intervalMs={180} />
          )}
          {stage === "transcribing" && hasLiveText && (
            <LiveTranscript text={partial ?? ""} />
          )}
          {stage === "enhancing" && (
            <ProcessingStatusDisplay label="Enhancing" intervalMs={220} />
          )}
          {stage === "idle" && <StaticVisualizer />}
        </div>

        <RecorderPowerModeButton
          open={popoverOpen === "power"}
          onOpenChange={handlePopoverChange("power")}
          popoverSide={isNotch ? "bottom" : "top"}
          onClosePopover={closePopover}
          session={powerSession}
          configs={powerConfigs}
        />
      </div>
    </div>
  );
}

// -- PromptButton (gauche) --------------------------------------------------

function RecorderPromptButton({
  open,
  onOpenChange,
  popoverSide,
  enabled,
  prompts,
  activeId,
  onToggleEnhancement,
  onSelectPrompt,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  popoverSide: "top" | "bottom";
  enabled: boolean;
  prompts: CustomPrompt[];
  activeId: string | null;
  onToggleEnhancement: () => void;
  onSelectPrompt: (id: string) => void;
}) {
  const { t } = useTranslation();
  return (
    <Popover open={open} onOpenChange={onOpenChange}>
      <PopoverTrigger asChild>
        <button
          className={cn(
            "ml-3 flex h-[22px] w-[22px] items-center justify-center rounded-full transition-colors hover:bg-white/10",
            !enabled && "opacity-60",
          )}
          title={
            enabled
              ? t("miniRecorder.promptActive")
              : t("miniRecorder.enhancementDisabled")
          }
          onDoubleClick={onToggleEnhancement}
        >
          <Sparkles className="h-3 w-3 text-white" />
        </button>
      </PopoverTrigger>
      <PopoverContent
        align="start"
        side={popoverSide}
        sideOffset={8}
        className="w-64 max-h-[320px] overflow-auto p-2"
      >
        <div className="grid gap-1">
          <button
            type="button"
            onClick={onToggleEnhancement}
            className={cn(
              "flex items-center justify-between rounded px-2 py-1.5 text-left text-xs hover:bg-accent",
              enabled && "bg-accent",
            )}
          >
            <span className="font-medium">{t("miniRecorder.enhancementLlm")}</span>
            <span className="text-[10px] text-muted-foreground">
              {enabled ? t("miniRecorder.active") : t("miniRecorder.off")}
            </span>
          </button>
          <div className="h-px bg-border" />
          {prompts.length === 0 && (
            <p className="px-2 py-1 text-xs text-muted-foreground">
              {t("miniRecorder.noPrompts")}
            </p>
          )}
          {prompts.map((p) => (
            <button
              key={p.id}
              type="button"
              onClick={() => onSelectPrompt(p.id)}
              className={cn(
                "flex items-center justify-between rounded px-2 py-1.5 text-left text-xs hover:bg-accent",
                p.id === activeId && "bg-accent",
              )}
            >
              <span className="truncate">{p.title}</span>
              {p.id === activeId && <Check className="h-3 w-3" />}
            </button>
          ))}
        </div>
      </PopoverContent>
    </Popover>
  );
}

// -- PowerModeButton (droite) ----------------------------------------------

function RecorderPowerModeButton({
  open,
  onOpenChange,
  popoverSide,
  onClosePopover,
  session,
  configs,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  popoverSide: "top" | "bottom";
  onClosePopover: () => void;
  session: PowerSession | null;
  configs: PowerModeConfig[];
}) {
  const { t } = useTranslation();
  const hasSession = !!session;
  const isEmpty = configs.length === 0;
  return (
    <Popover open={open} onOpenChange={onOpenChange}>
      <PopoverTrigger asChild>
        <button
          className={cn(
            "mr-3 flex h-[22px] w-[22px] items-center justify-center rounded-full transition-colors hover:bg-white/10",
            isEmpty && "opacity-60",
          )}
          title={session?.config_name ?? t("miniRecorder.powerMode")}
        >
          {hasSession ? (
            <span className="text-[14px] leading-none">{session.emoji}</span>
          ) : (
            // VoiceInk-style idle indicator : small dot. Colored green
            // when at least one profile is configured (just not active
            // right now), muted gray when no profiles exist.
            <span
              className={cn(
                "h-[6px] w-[6px] rounded-full",
                isEmpty ? "bg-white/40" : "bg-emerald-400",
              )}
            />
          )}
        </button>
      </PopoverTrigger>
      <PopoverContent
        align="end"
        side={popoverSide}
        sideOffset={8}
        className="w-64 max-h-[320px] overflow-auto p-2"
      >
        <div className="grid gap-1">
          {isEmpty ? (
            <div className="px-2 py-2 text-xs">
              <p className="mb-1 font-medium">{t("miniRecorder.powerMode")}</p>
              <p className="text-muted-foreground">
                {t("miniRecorder.noConfigs")}
              </p>
              <button
                type="button"
                onClick={async () => {
                  try {
                    await api.showMainWindow("powermode");
                    onClosePopover();
                  } catch (e) {
                    console.error(e);
                  }
                }}
                className="mt-2 inline-flex items-center gap-1.5 rounded-md bg-accent px-2 py-1 text-[11px] font-medium hover:bg-accent/80"
              >
                <SettingsIcon className="h-3 w-3" />
                {t("miniRecorder.openPowerMode")}
              </button>
            </div>
          ) : (
            <>
              {configs.map((c) => {
                // Meme mapping que le backend : le raccourci cible le Nieme
                // profil ACTIVE dans l'ordre stocke.
                const enabledIdx = c.is_enabled
                  ? configs.filter((x) => x.is_enabled).indexOf(c)
                  : -1;
                const shortcut = powerShortcutLabel(enabledIdx);
                const isActive = c.id === session?.config_id;
                return (
                  <div
                    key={c.id}
                    className={cn(
                      "flex items-center gap-2 rounded px-2 py-1.5 text-xs",
                      isActive && "bg-accent",
                      !c.is_enabled && "opacity-50",
                    )}
                  >
                    <span className="text-lg leading-none">{c.emoji}</span>
                    <span className="flex-1 truncate">{c.name}</span>
                    {shortcut && (
                      <kbd className="rounded border bg-muted px-1 py-0.5 font-mono text-[9px] font-medium text-muted-foreground">
                        {shortcut}
                      </kbd>
                    )}
                    {isActive && <Check className="h-3 w-3" />}
                  </div>
                );
              })}
              <p className="mt-1 px-2 text-[10px] text-muted-foreground">
                {t("miniRecorder.autoSwitch")}
              </p>
            </>
          )}
        </div>
      </PopoverContent>
    </Popover>
  );
}

// -- Audio visualizer (15 bars, wave + center boost) -----------------------

function AudioVisualizer({ meterDb }: { meterDb: number }) {
  // Normalisation -60dB..0dB -> 0..1 (aligne VoiceInk Recorder.swift L218+)
  const power = normalizePower(meterDb);
  // Curve perceptuelle ^0.7 (VoiceInk AudioVisualizerView amplitude clamp).
  const amplitude = Math.pow(power, 0.7);

  const startTime = useRef<number>(performance.now());
  const [tick, setTick] = useState(0);

  useEffect(() => {
    let raf = 0;
    const loop = () => {
      setTick(performance.now() - startTime.current);
      raf = requestAnimationFrame(loop);
    };
    raf = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(raf);
  }, []);

  const bars = useMemo(() => {
    const t = tick / 1000;
    return Array.from({ length: BARS }, (_, i) => {
      const phase = i * 0.4;
      const wave = Math.sin(t * 8 + phase) * 0.5 + 0.5;
      const distanceFromCenter = Math.abs(i - (BARS - 1) / 2) / ((BARS - 1) / 2);
      const centerBoost = 1.0 - distanceFromCenter * 0.4;
      const height = 4 + amplitude * wave * centerBoost * 24;
      return Math.max(4, Math.min(28, height));
    });
  }, [tick, amplitude]);

  return (
    <div className="flex h-7 items-center gap-[2px]">
      {bars.map((h, i) => (
        <span
          key={i}
          className="w-[3px] rounded-[1.5px] bg-white/85"
          style={{ height: `${h}px` }}
        />
      ))}
    </div>
  );
}

function StaticVisualizer() {
  return (
    <div className="flex h-7 items-center gap-[2px]">
      {Array.from({ length: BARS }).map((_, i) => (
        <span
          key={i}
          className="w-[3px] rounded-[1.5px] bg-white/50"
          style={{ height: "4px" }}
        />
      ))}
    </div>
  );
}

function normalizePower(db: number): number {
  if (!isFinite(db) || db <= -60) return 0;
  const clamped = Math.max(-60, Math.min(0, db));
  return (clamped + 60) / 60;
}

// -- Processing display ("Transcribing" / "Enhancing" + 5 dots) ------------

function ProcessingStatusDisplay({
  label,
  intervalMs,
}: {
  label: string;
  intervalMs: number;
}) {
  const [step, setStep] = useState(0);
  useEffect(() => {
    const id = window.setInterval(() => setStep((s) => (s + 1) % 6), intervalMs);
    return () => window.clearInterval(id);
  }, [intervalMs]);
  return (
    <div className="flex items-center gap-2 text-[11px] font-medium text-white">
      <span>{label}</span>
      <span className="flex items-center gap-[3px]">
        {Array.from({ length: 5 }).map((_, i) => (
          <span
            key={i}
            className={cn(
              "h-[3px] w-[3px] rounded-full transition-opacity",
              i < step ? "bg-white" : "bg-white/30",
            )}
          />
        ))}
      </span>
    </div>
  );
}

// -- LiveTranscript (mode expanded pour streaming partiel) -----------------

function LiveTranscript({ text }: { text: string }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (ref.current) {
      ref.current.scrollTop = ref.current.scrollHeight;
    }
  }, [text]);
  return (
    <div
      ref={ref}
      className="max-h-10 overflow-hidden px-2 text-[12px] leading-tight text-white/80"
    >
      {text}
    </div>
  );
}
