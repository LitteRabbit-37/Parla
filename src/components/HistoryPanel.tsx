import { useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { useTranslation } from "react-i18next";
import {
  Check,
  ChevronDown,
  Copy,
  Download,
  History,
  Loader2,
  Search,
  Trash2,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { cn } from "@/lib/utils";
import { useCopyButton } from "@/hooks/useCopyButton";
import { translateError } from "@/lib/translateError";
import {
  api,
  type RetentionSettings,
  type TranscriptionRecord,
} from "@/lib/tauri";

/// Si le texte est une erreur formate "Transcription Failed: PARLA_ERR:...",
/// extrait le code et le traduit. Sinon retourne le texte tel quel.
function localizeFailureText(
  t: ReturnType<typeof useTranslation>["t"],
  raw: string,
): string {
  const prefix = "Transcription Failed: ";
  if (!raw.startsWith(prefix)) return raw;
  const code = raw.slice(prefix.length);
  return `${t("history.failed")}: ${translateError(t, code)}`;
}

const PAGE_SIZE = 20;

function formatDate(iso: string): string {
  try {
    const d = new Date(iso);
    return d.toLocaleString();
  } catch {
    return iso;
  }
}

function formatDuration(secs: number | null): string {
  if (!secs || secs <= 0) return "-";
  if (secs < 60) return `${secs.toFixed(1)}s`;
  const m = Math.floor(secs / 60);
  const s = Math.round(secs % 60);
  return `${m}m${s.toString().padStart(2, "0")}`;
}

export function HistoryPanel() {
  const { t } = useTranslation();
  const [items, setItems] = useState<TranscriptionRecord[]>([]);
  const [loading, setLoading] = useState(false);
  const [search, setSearch] = useState("");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [expanded, setExpanded] = useState<string | null>(null);
  const [count, setCount] = useState<number | null>(null);
  const [retention, setRetention] = useState<RetentionSettings>({
    transcription_cleanup: false,
    transcription_retention_minutes: 1440,
    audio_cleanup: false,
    audio_retention_days: 7,
  });
  const [hasMore, setHasMore] = useState(false);

  useEffect(() => {
    refresh();
    api
      .getRetentionSettings()
      .then(setRetention)
      .catch(console.error);

    const unCreated = listen<string>("history:created", () => refresh());
    const unUpdated = listen<string>("history:updated", () => refresh());
    const unCleaned = listen("history:cleaned", () => refresh());
    return () => {
      unCreated.then((fn) => fn());
      unUpdated.then((fn) => fn());
      unCleaned.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    const t = window.setTimeout(() => refresh(), 200);
    return () => window.clearTimeout(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [search]);

  async function refresh() {
    setLoading(true);
    try {
      const [rows, total] = await Promise.all([
        api.listHistory({ limit: PAGE_SIZE, search: search || null }),
        api.countHistory(),
      ]);
      setItems(rows);
      setCount(total);
      setHasMore(rows.length === PAGE_SIZE);
    } catch (e) {
      console.error(e);
    } finally {
      setLoading(false);
    }
  }

  async function loadMore() {
    if (items.length === 0) return;
    const last = items[items.length - 1];
    setLoading(true);
    try {
      const rows = await api.listHistory({
        limit: PAGE_SIZE,
        before: last.timestamp,
        search: search || null,
      });
      setItems((prev) => [...prev, ...rows]);
      setHasMore(rows.length === PAGE_SIZE);
    } catch (e) {
      console.error(e);
    } finally {
      setLoading(false);
    }
  }

  function toggleSelect(id: string) {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  function selectAllVisible() {
    setSelected(new Set(items.map((i) => i.id)));
  }

  function clearSelection() {
    setSelected(new Set());
  }

  async function deleteSelected() {
    if (selected.size === 0) return;
    if (!confirm(t("history.confirmDeleteCount", { count: selected.size }))) return;
    for (const id of selected) {
      try {
        await api.deleteHistoryItem(id);
      } catch (e) {
        console.error(e);
      }
    }
    clearSelection();
    await refresh();
  }

  async function exportSelected() {
    const ids = selected.size > 0 ? [...selected] : items.map((i) => i.id);
    try {
      await api.exportHistoryCsv(ids);
    } catch (e) {
      console.error(e);
    }
  }

  async function saveRetention(next: RetentionSettings) {
    setRetention(next);
    try {
      await api.setRetentionSettings(next);
    } catch (e) {
      console.error(e);
    }
  }

  const selectedCount = selected.size;
  const hasAny = useMemo(() => items.length > 0, [items]);

  return (
    <Card>
      <CardHeader>
        <div className="flex items-center gap-2">
          <History className="h-4 w-4 text-muted-foreground" />
          <CardTitle className="text-base">{t("history.title")}</CardTitle>
        </div>
        <CardDescription>
          {t("history.descriptionFull")}
          {count !== null && ` ${t("history.totalEntries", { count })}`}
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        <div className="flex items-center gap-2">
          <div className="relative flex-1">
            <Search className="pointer-events-none absolute left-2 top-2.5 h-4 w-4 text-muted-foreground" />
            <input
              type="text"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder={t("history.search")}
              className="h-9 w-full rounded-md border border-input bg-background pl-8 pr-3 text-sm"
            />
          </div>
          {selectedCount > 0 ? (
            <>
              <span className="text-xs text-muted-foreground">
                {t("history.selectedCount", { count: selectedCount })}
              </span>
              <Button size="sm" variant="ghost" onClick={clearSelection}>
                {t("history.deselect")}
              </Button>
            </>
          ) : (
            hasAny && (
              <Button size="sm" variant="ghost" onClick={selectAllVisible}>
                {t("history.selectAll")}
              </Button>
            )
          )}
          <Button
            size="sm"
            variant="outline"
            onClick={exportSelected}
            disabled={!hasAny}
          >
            <Download className="h-3.5 w-3.5" />
            {t("history.exportCsv")}
          </Button>
          <Button
            size="sm"
            variant="ghost"
            onClick={deleteSelected}
            disabled={selectedCount === 0}
          >
            <Trash2 className="h-3.5 w-3.5" />
            {t("history.delete")}
          </Button>
        </div>

        {loading && items.length === 0 && (
          <div className="flex justify-center py-4">
            <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" />
          </div>
        )}

        {!loading && items.length === 0 && (
          <p className="py-4 text-center text-sm text-muted-foreground">
            {t("history.emptyLong")}
          </p>
        )}

        <ul className="grid gap-2">
          {items.map((it) => {
            const isSelected = selected.has(it.id);
            const isExpanded = expanded === it.id;
            const rawDisplay = it.enhanced_text ?? it.text;
            const display =
              it.status === "failed"
                ? localizeFailureText(t, rawDisplay)
                : rawDisplay;
            return (
              <li
                key={it.id}
                className={cn(
                  "rounded-md border p-3 text-sm",
                  isSelected && "border-primary/60 bg-primary/5",
                  it.status === "failed" && "border-destructive/40",
                )}
              >
                <div className="flex items-start gap-2">
                  <input
                    type="checkbox"
                    checked={isSelected}
                    onChange={() => toggleSelect(it.id)}
                    className="mt-1 h-4 w-4"
                  />
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2 text-xs text-muted-foreground">
                      {it.power_mode_emoji && (
                        <span>{it.power_mode_emoji}</span>
                      )}
                      <span>{formatDate(it.timestamp)}</span>
                      <span>- {formatDuration(it.duration_sec)}</span>
                      {it.transcription_model_name && (
                        <span className="truncate">
                          {" - "}
                          <code>{it.transcription_model_name}</code>
                        </span>
                      )}
                      {it.status === "failed" && (
                        <span className="text-destructive">- {t("history.failed")}</span>
                      )}
                      {it.status === "pending" && (
                        <span>- {t("history.pending")}</span>
                      )}
                    </div>
                    <p
                      className={cn(
                        "mt-1 text-sm",
                        !isExpanded && "line-clamp-2",
                      )}
                    >
                      {display || t("history.emptyText")}
                    </p>
                  </div>
                  <div className="flex shrink-0 flex-col gap-1">
                    <Button
                      size="sm"
                      variant="ghost"
                      onClick={() =>
                        setExpanded(isExpanded ? null : it.id)
                      }
                    >
                      <ChevronDown
                        className={cn(
                          "h-3.5 w-3.5 transition-transform",
                          isExpanded && "rotate-180",
                        )}
                      />
                    </Button>
                    <CopyButton text={display} title={t("history.copy")} />
                  </div>
                </div>
                {isExpanded && (
                  <div className="mt-2 grid gap-2 text-xs">
                    {it.text && (
                      <div className="rounded bg-muted/40 p-2">
                        <div className="mb-1 flex items-center justify-between">
                          <span className="font-medium">{t("history.rawText")}</span>
                          <CopyButton text={it.text} small />
                        </div>
                        <p className="whitespace-pre-wrap">{it.text}</p>
                      </div>
                    )}
                    {it.enhanced_text && (
                      <div className="rounded bg-primary/5 p-2">
                        <div className="mb-1 flex items-center justify-between">
                          <span className="font-medium">{t("history.enhancedText")}</span>
                          <CopyButton text={it.enhanced_text} small />
                        </div>
                        <p className="whitespace-pre-wrap">
                          {it.enhanced_text}
                        </p>
                      </div>
                    )}
                    <dl className="grid grid-cols-2 gap-1 text-[11px] text-muted-foreground">
                      {it.prompt_name && (
                        <>
                          <dt>{t("history.labelPrompt")}</dt>
                          <dd>{it.prompt_name}</dd>
                        </>
                      )}
                      {it.ai_enhancement_model_name && (
                        <>
                          <dt>{t("history.labelLlm")}</dt>
                          <dd>{it.ai_enhancement_model_name}</dd>
                        </>
                      )}
                      {it.enhancement_duration_sec && (
                        <>
                          <dt>{t("history.labelEnhanceTime")}</dt>
                          <dd>{formatDuration(it.enhancement_duration_sec)}</dd>
                        </>
                      )}
                      {it.transcription_duration_sec && (
                        <>
                          <dt>{t("history.labelTranscribeTime")}</dt>
                          <dd>
                            {formatDuration(it.transcription_duration_sec)}
                          </dd>
                        </>
                      )}
                      {it.power_mode_name && (
                        <>
                          <dt>{t("history.labelPowerMode")}</dt>
                          <dd>
                            {it.power_mode_emoji} {it.power_mode_name}
                          </dd>
                        </>
                      )}
                      {it.language && (
                        <>
                          <dt>{t("history.labelLanguage")}</dt>
                          <dd>{it.language}</dd>
                        </>
                      )}
                    </dl>
                  </div>
                )}
              </li>
            );
          })}
        </ul>

        {hasMore && (
          <div className="flex justify-center">
            <Button
              size="sm"
              variant="outline"
              onClick={loadMore}
              disabled={loading}
            >
              {loading ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : null}
              {t("history.loadMore")}
            </Button>
          </div>
        )}

        <fieldset className="grid gap-2 rounded-md border p-3 text-xs">
          <legend className="px-1 font-medium">{t("history.retention")}</legend>
          <label className="flex items-center gap-2">
            <input
              type="checkbox"
              checked={retention.transcription_cleanup}
              onChange={(e) =>
                saveRetention({
                  ...retention,
                  transcription_cleanup: e.target.checked,
                })
              }
            />
            {t("history.retentionSentenceA")}
            <input
              type="number"
              min={0}
              value={retention.transcription_retention_minutes}
              onChange={(e) =>
                saveRetention({
                  ...retention,
                  transcription_retention_minutes: Number(e.target.value) || 0,
                })
              }
              className="h-7 w-20 rounded-md border border-input bg-background px-2 text-xs"
            />
            {t("history.retentionSentenceB")}
          </label>
          <label className="flex items-center gap-2">
            <input
              type="checkbox"
              checked={retention.audio_cleanup}
              disabled={retention.transcription_cleanup}
              onChange={(e) =>
                saveRetention({
                  ...retention,
                  audio_cleanup: e.target.checked,
                })
              }
            />
            {t("history.retentionSentenceC")}
            <input
              type="number"
              min={0}
              value={retention.audio_retention_days}
              onChange={(e) =>
                saveRetention({
                  ...retention,
                  audio_retention_days: Number(e.target.value) || 0,
                })
              }
              className="h-7 w-16 rounded-md border border-input bg-background px-2 text-xs"
              disabled={retention.transcription_cleanup}
            />
            {t("history.retentionSentenceD")}
          </label>
          <p className="text-muted-foreground">
            {t("history.retentionHelp")}
          </p>
        </fieldset>
      </CardContent>
    </Card>
  );
}

function CopyButton({
  text,
  small,
  title,
}: {
  text: string | null | undefined;
  small?: boolean;
  title?: string;
}) {
  const { t } = useTranslation();
  const { copied, copy } = useCopyButton();
  const Icon = copied ? Check : Copy;
  return (
    <Button
      size="sm"
      variant="ghost"
      onClick={() => copy(text)}
      title={title ?? t("history.copy")}
      className={cn(copied && "text-green-600 dark:text-green-400")}
    >
      <Icon className={small ? "h-3 w-3" : "h-3.5 w-3.5"} />
    </Button>
  );
}
