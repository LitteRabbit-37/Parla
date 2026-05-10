// Dashboard : hero + 4 metric cards (sessions, mots, WPM, frappes economisees).
//
// Reference VoiceInk Views/Metrics/MetricsContent.swift + MetricsView.swift :
// LazyVGrid .adaptive(minimum: 240) de 4 MetricCards (Sessions, Words,
// WPM, Keystrokes Saved). Hero gradient accent avec "time saved".
// Subtitle : "Dictated N words across K sessions."

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import {
  Gauge,
  Keyboard,
  Mic,
  Speaker,
  Type,
} from "lucide-react";
import { Card } from "@/components/ui/card";
import { ModelPerformancePanel } from "@/components/ModelPerformancePanel";
import { api, type TranscriptionRecord } from "@/lib/tauri";

type Metrics = {
  sessions: number;
  words: number;
  wpm: number;
  keystrokesSaved: number;
  timeSavedMinutes: number;
};

function computeMetrics(items: TranscriptionRecord[]): Metrics {
  let sessions = 0;
  let words = 0;
  let durationSec = 0;
  let charCount = 0;
  for (const it of items) {
    if (it.status !== "completed") continue;
    const txt = it.enhanced_text ?? it.text;
    if (!txt) continue;
    sessions += 1;
    const w = txt.trim().split(/\s+/).filter(Boolean).length;
    words += w;
    charCount += txt.length;
    durationSec += it.duration_sec ?? 0;
  }
  const wpm = durationSec > 0 ? Math.round(words / (durationSec / 60)) : 0;
  // Typing speed ref : 40 WPM soit ~200 cpm. Temps frappe estime =
  // charCount / 200 caracteres par minute.
  const typingMinutes = charCount / 200;
  const spokenMinutes = durationSec / 60;
  const timeSavedMinutes = Math.max(0, Math.round(typingMinutes - spokenMinutes));
  return {
    sessions,
    words,
    wpm,
    keystrokesSaved: charCount,
    timeSavedMinutes,
  };
}

export function DashboardPanel() {
  const { t } = useTranslation();
  const [metrics, setMetrics] = useState<Metrics | null>(null);
  const [count, setCount] = useState(0);

  useEffect(() => {
    load();
    const un1 = listen("history:updated", () => load());
    const un2 = listen("history:created", () => load());
    return () => {
      un1.then((fn) => fn());
      un2.then((fn) => fn());
    };
  }, []);

  async function load() {
    try {
      const [items, total] = await Promise.all([
        api.listHistory({ limit: 200 }),
        api.countHistory(),
      ]);
      setMetrics(computeMetrics(items));
      setCount(total);
    } catch (e) {
      console.error(e);
    }
  }

  return (
    <div className="space-y-6">
      <div className="rounded-2xl border bg-gradient-to-br from-primary/15 via-primary/5 to-transparent p-6 shadow-sm">
        <p className="text-xs uppercase tracking-wide text-muted-foreground">
          {t("dashboard.timeSaved")}
        </p>
        <p className="mt-1 text-4xl font-black leading-none">
          {metrics ? `${metrics.timeSavedMinutes} min` : "-"}
        </p>
        <p className="mt-2 text-sm text-muted-foreground">
          {metrics && metrics.sessions > 0
            ? t("dashboard.heroWithData", {
                words: metrics.words.toLocaleString(),
                sessions: metrics.sessions,
              })
            : t("dashboard.heroEmpty")}
        </p>
      </div>

      <div className="grid grid-cols-1 gap-3 md:grid-cols-2 lg:grid-cols-4">
        <MetricCard
          icon={Mic}
          color="text-purple-500 bg-purple-500/10"
          title={t("dashboard.sessions")}
          value={metrics?.sessions ?? 0}
          subtitle={t("dashboard.sessionsSubtitle")}
        />
        <MetricCard
          icon={Type}
          color="text-primary bg-primary/10"
          title={t("dashboard.wordsDictated")}
          value={metrics?.words ?? 0}
          subtitle={t("dashboard.wordsDictatedSubtitle")}
        />
        <MetricCard
          icon={Gauge}
          color="text-yellow-500 bg-yellow-500/10"
          title={t("dashboard.wordsPerMinute")}
          value={metrics?.wpm ?? 0}
          subtitle={t("dashboard.wordsPerMinuteSubtitle")}
        />
        <MetricCard
          icon={Keyboard}
          color="text-orange-500 bg-orange-500/10"
          title={t("dashboard.characters")}
          value={metrics?.keystrokesSaved ?? 0}
          subtitle={t("dashboard.charactersSubtitle")}
        />
      </div>

      <ModelPerformancePanel />

      <Card className="flex items-center gap-3 p-4">
        <Speaker className="h-4 w-4 text-muted-foreground" />
        <div>
          <p className="text-sm font-medium">
            {t("dashboard.historyCount", { count })}
          </p>
          <p className="text-xs text-muted-foreground">
            {t("dashboard.historyHint")}
          </p>
        </div>
      </Card>
    </div>
  );
}

function MetricCard({
  icon: Icon,
  color,
  title,
  value,
  subtitle,
}: {
  icon: React.ComponentType<{ className?: string }>;
  color: string;
  title: string;
  value: number;
  subtitle: string;
}) {
  return (
    <Card className="p-4">
      <div className="flex items-start gap-3">
        <div
          className={`flex h-9 w-9 items-center justify-center rounded-full ${color}`}
        >
          <Icon className="h-4 w-4" />
        </div>
        <div className="min-w-0 flex-1">
          <p className="text-xs text-muted-foreground">{title}</p>
          <p className="truncate text-2xl font-bold leading-tight">
            {value.toLocaleString()}
          </p>
          <p className="truncate text-[11px] text-muted-foreground">
            {subtitle}
          </p>
        </div>
      </div>
    </Card>
  );
}
