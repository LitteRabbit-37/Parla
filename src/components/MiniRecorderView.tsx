// Mini-recorder : overlay 300x120 affichee dans la fenetre "recorder".
//
// Reference VoiceInk Features/Recording/Views/MiniRecorderView.swift +
// Components/RecorderComponents.swift + AudioVisualizerView.swift :
//   VStack [LiveTranscriptView 56pt (pendant l'enregistrement, si texte)]
//          [Divider]
//          HStack [RecordButton 21pt] Spacer [StatusDisplay] Spacer [ModeButton 22pt]
//   control bar 40pt epingle bas, background Color.black opaque,
//   largeur 184 (compact) / 300 (texte en direct), corner radius 20 / 14.
//   15 bars audio avec wave + center boost, 60 FPS.
//   Processing: "Transcribing" / "Enhancing" + 5 dots animes.
//   Texte en direct : reglage ShowLiveTranscript (defaut true), affiche
//   uniquement pendant recordingState == .recording (commit 42f7ec8).
//   Astuce Echap : "Press Esc again to cancel" au premier Echap, une seule
//   fois (RecorderPanelShortcutManager.showEscapeConfirmationHintIfNeeded).
//
// Boutons (VoiceInk 2.x) :
//   - Gauche : RecorderRecordButton, cercle 21 pt gris au repos, rouge en
//     enregistrement, translucide et inactif pendant le traitement. Clic =
//     demarrer / arreter. Le selecteur de prompt de VoiceInk 1.x a ete retire
//     de la bulle (commit d59dfde), le prompt est porte par le mode.
//   - Droite : RecorderModeButton, popover ouvert au survol ou au clic,
//     ferme 250 ms apres avoir quitte le bouton ET le popover
//     (syncPopoverVisibility). Le popover est une fenetre separee
//     (NSPopover chez VoiceInk, fenetre "recorder-popover" ici) : la bulle
//     n'est plus redimensionnee.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { emit, listen } from "@tauri-apps/api/event";
import { LayoutGrid } from "lucide-react";
import { api, type AudioMeter, type PowerModeConfig, type PowerSession } from "@/lib/tauri";
import { cn } from "@/lib/utils";

/// Duree d'affichage de l'astuce Echap = fenetre du double-Echap (1.5 s).
const ESCAPE_HINT_MS = 1500;
/// Delai de grace avant fermeture du popover (VoiceInk 0.25 s).
const POPOVER_GRACE_MS = 250;
const BARS = 15;

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

type PopoverHoverEvent = { hovering: boolean };

export function MiniRecorderView() {
  const { t } = useTranslation();
  const [stage, setStage] = useState<Stage>("recording");
  const [meterDb, setMeterDb] = useState<number>(-160);
  // Texte en direct : les providers streaming envoient a chaque event le
  // transcript cumule (segments confirmes + hypothese), on affiche donc
  // toujours le dernier recu (VoiceInk StreamingTranscriptionService
  // committed + partial).
  const [liveText, setLiveText] = useState<string>("");
  const [showLiveTranscript, setShowLiveTranscript] = useState(true);
  const [escapeHint, setEscapeHint] = useState(false);
  const [powerSession, setPowerSession] = useState<PowerSession | null>(null);
  const [powerConfigs, setPowerConfigs] = useState<PowerModeConfig[]>([]);
  const [style, setStyle] = useState<"mini" | "notch">("mini");

  // Popover Mode : survol du bouton (cette fenetre) + survol du popover
  // (remonte par l'autre fenetre via "recorder:popover-hover").
  const [hoverButton, setHoverButton] = useState(false);
  const [hoverPopover, setHoverPopover] = useState(false);
  const popoverOpenRef = useRef(false);
  const closeTimerRef = useRef<number | null>(null);
  const modeButtonRef = useRef<HTMLButtonElement>(null);
  const pillRef = useRef<HTMLDivElement>(null);

  const openPopover = useCallback(async () => {
    const btn = modeButtonRef.current;
    const pill = pillRef.current;
    if (!btn || !pill) return;
    const b = btn.getBoundingClientRect();
    const p = pill.getBoundingClientRect();
    popoverOpenRef.current = true;
    try {
      await emit("recorder:popover-refresh", {});
      await api.openRecorderPopover(b.left + b.width / 2, p.top, p.bottom);
    } catch (e) {
      console.error(e);
    }
  }, []);

  const closePopover = useCallback(() => {
    popoverOpenRef.current = false;
    api.closeRecorderPopover().catch(console.error);
  }, []);

  const wantPopover = hoverButton || hoverPopover;
  useEffect(() => {
    if (closeTimerRef.current !== null) {
      window.clearTimeout(closeTimerRef.current);
      closeTimerRef.current = null;
    }
    if (wantPopover) {
      if (!popoverOpenRef.current) openPopover();
    } else if (popoverOpenRef.current) {
      closeTimerRef.current = window.setTimeout(closePopover, POPOVER_GRACE_MS);
    }
    return () => {
      if (closeTimerRef.current !== null) {
        window.clearTimeout(closeTimerRef.current);
        closeTimerRef.current = null;
      }
    };
  }, [wantPopover, openPopover, closePopover]);

  useEffect(() => {
    // Charge une fois au mount - la fenetre mini-recorder est recreee a
    // chaque start donc pas besoin de re-fetch frequent.
    Promise.all([
      api.listPowerConfigs(),
      api.getRecorderStyle(),
      api.getShowLiveTranscript(),
    ])
      .then(([pcs, rs, live]) => {
        setPowerConfigs(pcs);
        setStyle(rs === "notch" ? "notch" : "mini");
        setShowLiveTranscript(live);
      })
      .catch(console.error);

    let hintTimer: number | null = null;
    const unlistens = [
      listen("recording:stopped", () => {
        setStage("transcribing");
        setEscapeHint(false);
      }),
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
          setLiveText((prev) => (prev === s.text ? prev : s.text));
        }
      }),
      listen("recorder:escape-hint", () => {
        setEscapeHint(true);
        if (hintTimer !== null) window.clearTimeout(hintTimer);
        hintTimer = window.setTimeout(() => setEscapeHint(false), ESCAPE_HINT_MS);
      }),
      listen<PowerSession | null>("power_mode:active", (e) => {
        setPowerSession(e.payload);
      }),
      listen<PopoverHoverEvent>("recorder:popover-hover", (e) => {
        setHoverPopover(!!e.payload?.hovering);
      }),
    ];
    return () => {
      if (hintTimer !== null) window.clearTimeout(hintTimer);
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

  // VoiceInk hasLiveTranscript : showLiveTranscript && state == .recording
  // && !partialTranscript.isEmpty. Le texte disparait a l'arret, remplace
  // par le badge "Transcribing".
  const hasLiveText =
    showLiveTranscript && stage === "recording" && liveText.trim().length > 0;
  const expanded = hasLiveText;
  const isNotch = style === "notch";
  const hasEnabledConfigs = powerConfigs.some((c) => c.is_enabled);

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
        ref={pillRef}
        className={cn(
          "flex flex-col bg-black text-white shadow-lg transition-all duration-300 ease-in-out",
          expanded ? "w-[300px]" : "w-[184px]",
          shape,
        )}
        style={spacingStyle}
      >
        {hasLiveText && !isNotch && (
          <>
            <LiveTranscript text={liveText} />
            <div className="h-px bg-white/15" />
          </>
        )}
        <div className="flex h-10 items-center">
          <RecorderRecordButton
            stage={stage}
            onClick={() => api.toggleRecordingFromUi().catch(console.error)}
          />

          <div className="flex flex-1 items-center justify-center overflow-hidden px-1">
            {stage === "recording" && escapeHint && (
              <span className="truncate text-[11px] font-medium text-white/90">
                {t("miniRecorder.escapeHint")}
              </span>
            )}
            {stage === "recording" && !escapeHint && (
              <AudioVisualizer meterDb={meterDb} />
            )}
            {stage === "transcribing" && (
              <ProcessingStatusDisplay label="Transcribing" intervalMs={180} />
            )}
            {stage === "enhancing" && (
              <ProcessingStatusDisplay label="Enhancing" intervalMs={220} />
            )}
            {stage === "idle" && <StaticVisualizer />}
          </div>

          <button
            ref={modeButtonRef}
            type="button"
            onMouseEnter={() => setHoverButton(true)}
            onMouseLeave={() => setHoverButton(false)}
            onClick={() => {
              if (popoverOpenRef.current) closePopover();
              else openPopover();
            }}
            title={powerSession?.config_name ?? t("miniRecorder.powerMode")}
            className={cn(
              "mr-3 flex h-[22px] w-[22px] shrink-0 items-center justify-center rounded-full transition-colors hover:bg-white/10",
              !hasEnabledConfigs && "opacity-60",
            )}
          >
            {powerSession ? (
              <span className="text-[14px] leading-none">{powerSession.emoji}</span>
            ) : (
              // VoiceInk : icone "square.grid.2x2" tant qu'aucun mode n'est
              // effectif.
              <LayoutGrid className="h-3.5 w-3.5 text-white/85" />
            )}
          </button>
        </div>
        {hasLiveText && isNotch && (
          <>
            <div className="h-px bg-white/15" />
            <LiveTranscript text={liveText} />
          </>
        )}
      </div>
    </div>
  );
}

// -- RecordButton (gauche) : VoiceInk RecorderRecordButton -----------------

const RECORD_COLORS = {
  ready: { surface: "#4D4D52", border: "#6B6B70", mark: "#C7C7CC" },
  recording: {
    surface: "rgba(229, 72, 77, 0.92)",
    border: "rgba(229, 72, 77, 0.98)",
    mark: "#FFFFFF",
  },
  processing: {
    surface: "rgba(255, 255, 255, 0.13)",
    border: "rgba(255, 255, 255, 0.18)",
    mark: "rgba(255, 255, 255, 0.86)",
  },
} as const;

function RecorderRecordButton({
  stage,
  onClick,
}: {
  stage: Stage;
  onClick: () => void;
}) {
  const { t } = useTranslation();
  const visual =
    stage === "recording" ? "recording" : stage === "idle" ? "ready" : "processing";
  const colors = RECORD_COLORS[visual];
  const disabled = visual === "processing";
  const title =
    visual === "recording"
      ? t("miniRecorder.stopRecording")
      : visual === "ready"
        ? t("miniRecorder.startRecording")
        : t("miniRecorder.processing");
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      title={title}
      className="ml-2.5 flex h-[21px] w-[21px] shrink-0 items-center justify-center rounded-full transition-colors duration-150 disabled:cursor-default"
      style={{
        background: colors.surface,
        boxShadow: `inset 0 0 0 0.6px ${colors.border}`,
      }}
    >
      <span
        className="block h-[7px] w-[7px] rounded-[2.2px]"
        style={{ background: colors.mark }}
      />
    </button>
  );
}

// -- Audio visualizer (15 bars, wave + center boost) -----------------------

function AudioVisualizer({ meterDb }: { meterDb: number }) {
  // Normalisation -60dB..0dB -> 0..1 (aligne VoiceInk Recorder.swift)
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

// -- LiveTranscript (VoiceInk LiveTranscriptView) ---------------------------
//
// Zone de 54 px (VoiceInk 56 pt, reduite d'un rien pour tenir dans la
// fenetre de 120 px avec la marge basse de 24 px), texte 12 pt blanc 80 %,
// masque degrade en haut, defilement automatique vers le bas a chaque mise
// a jour, sans animation pour que les glyphes ne glissent pas.

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
      className="h-[54px] overflow-hidden px-4 py-1.5 text-left text-[12px] leading-snug text-white/80"
      style={{
        maskImage:
          "linear-gradient(to bottom, transparent 0%, black 18%, black 100%)",
        WebkitMaskImage:
          "linear-gradient(to bottom, transparent 0%, black 18%, black 100%)",
      }}
    >
      {text}
    </div>
  );
}
