// Panneau de performance par modele.
//
// Reference VoiceInk : Views/Metrics/ModelPerformancePanel.swift.
// Filtre temporel (7j / 30j / cette annee / tout) + 2 sections (modeles
// de transcription / d'enhancement) en grille de tuiles. Les metriques
// sont agregees cote backend a partir de la table transcriptions.

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { BarChart3 } from "lucide-react";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import {
  api,
  type EnhancementModelMetric,
  type MetricsPeriod,
  type ModelPerformanceMetrics,
  type TranscriptionModelMetric,
} from "@/lib/tauri";

function formatDurationShort(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds <= 0) return "0s";
  if (seconds < 60) return `${seconds.toFixed(0)}s`;
  const m = Math.floor(seconds / 60);
  const s = Math.round(seconds % 60);
  return s === 0 ? `${m}m` : `${m}m ${s}s`;
}

export function ModelPerformancePanel() {
  const { t } = useTranslation();
  const [period, setPeriod] = useState<MetricsPeriod>("last_7_days");
  const [data, setData] = useState<ModelPerformanceMetrics | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    refresh();
    const un1 = listen("history:updated", () => refresh());
    const un2 = listen("history:created", () => refresh());
    return () => {
      un1.then((fn) => fn());
      un2.then((fn) => fn());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [period]);

  async function refresh() {
    try {
      const result = await api.getModelPerformanceMetrics(period);
      setData(result);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }

  const isEmpty =
    !data ||
    (data.transcription_models.length === 0 && data.enhancement_models.length === 0);

  return (
    <Card>
      <CardHeader>
        <div className="flex items-start justify-between gap-3">
          <div>
            <div className="flex items-center gap-2">
              <BarChart3 className="h-4 w-4 text-muted-foreground" />
              <CardTitle className="text-base">
                {t("modelPerformance.title")}
              </CardTitle>
            </div>
            <CardDescription className="mt-1">
              {t("modelPerformance.description")}
            </CardDescription>
          </div>
          <select
            value={period}
            onChange={(e) => setPeriod(e.target.value as MetricsPeriod)}
            className="h-8 shrink-0 rounded-md border border-input bg-background px-2 text-sm shadow-sm"
          >
            <option value="last_7_days">
              {t("modelPerformance.period.last7Days")}
            </option>
            <option value="last_30_days">
              {t("modelPerformance.period.last30Days")}
            </option>
            <option value="this_year">
              {t("modelPerformance.period.thisYear")}
            </option>
            <option value="all_time">
              {t("modelPerformance.period.allTime")}
            </option>
          </select>
        </div>
      </CardHeader>
      <CardContent>
        {error && (
          <p className="text-xs text-destructive">
            {t("common.errorPrefix", { message: error })}
          </p>
        )}
        {isEmpty ? (
          <p className="text-sm text-muted-foreground">
            {t("modelPerformance.empty")}
          </p>
        ) : (
          <div className="space-y-6">
            {data!.transcription_models.length > 0 && (
              <section>
                <h3 className="mb-3 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
                  {t("modelPerformance.transcriptionModels")}
                </h3>
                <div className="grid gap-3 sm:grid-cols-2">
                  {data!.transcription_models.map((m) => (
                    <TranscriptionTile key={m.name} metric={m} />
                  ))}
                </div>
              </section>
            )}
            {data!.enhancement_models.length > 0 && (
              <section>
                <h3 className="mb-3 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
                  {t("modelPerformance.enhancementModels")}
                </h3>
                <div className="grid gap-3 sm:grid-cols-2">
                  {data!.enhancement_models.map((m) => (
                    <EnhancementTile key={m.name} metric={m} />
                  ))}
                </div>
              </section>
            )}
          </div>
        )}
      </CardContent>
    </Card>
  );
}

function TranscriptionTile({ metric }: { metric: TranscriptionModelMetric }) {
  const { t } = useTranslation();
  const speedFactor =
    metric.total_processing_sec > 0
      ? metric.total_audio_sec / metric.total_processing_sec
      : 0;
  const avgAudio =
    metric.session_count > 0 ? metric.total_audio_sec / metric.session_count : 0;
  const avgProcessing =
    metric.session_count > 0
      ? metric.total_processing_sec / metric.session_count
      : 0;
  const fasterThanRealtime = speedFactor >= 1.0;

  return (
    <div className="rounded-lg border p-3">
      <div className="text-center">
        <p className="truncate text-sm font-semibold">{metric.name}</p>
        <p className="text-[11px] text-muted-foreground">
          {t("modelPerformance.sessionCount", { count: metric.session_count })}
        </p>
      </div>
      <div className="my-3 text-center">
        <p className="text-3xl font-bold leading-none text-emerald-500">
          {speedFactor.toFixed(1)}x
        </p>
        <p className="mt-1 text-[11px] text-muted-foreground">
          {fasterThanRealtime
            ? t("modelPerformance.fasterRealtime")
            : t("modelPerformance.slowerRealtime")}
        </p>
      </div>
      <div className="grid grid-cols-2 gap-1 border-t pt-2 text-center">
        <div>
          <p className="font-mono text-xs font-semibold text-indigo-500">
            {formatDurationShort(avgAudio)}
          </p>
          <p className="text-[10px] text-muted-foreground">
            {t("modelPerformance.avgAudio")}
          </p>
        </div>
        <div>
          <p className="font-mono text-xs font-semibold text-teal-500">
            {avgProcessing.toFixed(2)}s
          </p>
          <p className="text-[10px] text-muted-foreground">
            {t("modelPerformance.avgProcessing")}
          </p>
        </div>
      </div>
    </div>
  );
}

function EnhancementTile({ metric }: { metric: EnhancementModelMetric }) {
  const { t } = useTranslation();
  const avgDuration =
    metric.session_count > 0
      ? metric.total_duration_sec / metric.session_count
      : 0;
  return (
    <div className="rounded-lg border p-3 text-center">
      <p className="truncate text-sm font-semibold">{metric.name}</p>
      <p className="text-[11px] text-muted-foreground">
        {t("modelPerformance.sessionCount", { count: metric.session_count })}
      </p>
      <p className="mt-3 text-3xl font-bold leading-none text-indigo-500">
        {avgDuration.toFixed(2)}s
      </p>
      <p className="mt-1 text-[11px] text-muted-foreground">
        {t("modelPerformance.avgEnhancement")}
      </p>
    </div>
  );
}
