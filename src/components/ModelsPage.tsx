// Page AI Models - refonte fidele a VoiceInk ModelManagementView :
// header "Default Model", langue de dictee, filtres pills purement visuels
// (Recommended / Local / Cloud) et liste unifiee de cards de modeles.
//
// L'activation est toujours un geste explicite ("Set as Default" sur une
// card) ; configurer/verifier une cle API ne change jamais la source
// active (issue #6, comportement VoiceInk). Changer de filtre ne change
// jamais la source non plus - seul le modele actif compte, il est rappele
// dans le header et surligne dans la liste.

import { useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { useTranslation } from "react-i18next";
import { DictationLanguagePanel } from "@/components/DictationLanguagePanel";
import { CloudModelCard } from "@/components/models/CloudModelCard";
import { DefaultModelCard } from "@/components/models/DefaultModelCard";
import { ImportModelCard } from "@/components/models/ImportModelCard";
import { ModelFilterPills } from "@/components/models/ModelFilterPills";
import { ParakeetModelCard } from "@/components/models/ParakeetModelCard";
import { WhisperModelCard } from "@/components/models/WhisperModelCard";
import {
  isRowCurrent,
  RECOMMENDED_MODELS,
  resolveDefaultDisplayName,
  rowName,
  type CloudModel,
  type CloudProvider,
  type ModelFilter,
  type ModelRow,
  type ParakeetDownloadProgress,
} from "@/components/models/types";
import {
  api,
  type DownloadComplete,
  type DownloadError,
  type DownloadProgress,
  type ParakeetModelState,
  type TranscriptionSource,
  type WhisperModelState,
} from "@/lib/tauri";

export function ModelsPage({
  selectedModelId,
  onSelectModel,
}: {
  selectedModelId: string | null;
  onSelectModel: (id: string | null) => void;
}) {
  const { t } = useTranslation();
  const [whisper, setWhisper] = useState<WhisperModelState[]>([]);
  const [parakeet, setParakeet] = useState<ParakeetModelState[]>([]);
  const [providers, setProviders] = useState<CloudProvider[]>([]);
  const [cloudModels, setCloudModels] = useState<CloudModel[]>([]);
  const [source, setSource] = useState<TranscriptionSource | null>(null);
  const [ep, setEp] = useState<string>("cpu");
  const [filter, setFilter] = useState<ModelFilter>("recommended");
  const [whisperProgress, setWhisperProgress] = useState<
    Record<string, DownloadProgress>
  >({});
  const [parakeetProgress, setParakeetProgress] = useState<
    Record<string, ParakeetDownloadProgress>
  >({});
  const [whisperErrors, setWhisperErrors] = useState<Record<string, string>>(
    {},
  );
  const [parakeetStatus, setParakeetStatus] = useState<Record<string, string>>(
    {},
  );

  // selectedModelId dans une ref pour les callbacks des listeners montes
  // une seule fois (meme intention que l'auto-selection de l'ancien
  // ModelsPanel, sans closure perimee).
  const selectedRef = useRef(selectedModelId);
  useEffect(() => {
    selectedRef.current = selectedModelId;
  }, [selectedModelId]);

  async function refresh() {
    try {
      const [w, pk, provs, cm, src, e] = await Promise.all([
        api.listWhisperModels(),
        api.listParakeetModels(),
        api.listCloudProviders(),
        api.listCloudModels(),
        api.getTranscriptionSource(),
        api.parakeetExecutionProvider(),
      ]);
      setWhisper(w);
      setParakeet(pk);
      setProviders(provs);
      setCloudModels(cm);
      setSource(src);
      setEp(e);
      // Auto-selection : premier whisper telecharge si rien de selectionne
      // (TranscribePanel depend de selectedModelId). Ne change pas le kind.
      if (!selectedRef.current) {
        const first = w.find((m) => m.downloaded);
        if (first) onSelectModel(first.id);
      }
    } catch (e) {
      console.error(e);
    }
  }

  useEffect(() => {
    refresh();
    const unlisteners = [
      listen<TranscriptionSource>("source:changed", (e) => {
        if (e.payload?.kind) setSource(e.payload);
      }),
      listen<DownloadProgress>("model:download:progress", (e) => {
        setWhisperProgress((p) => ({ ...p, [e.payload.id]: e.payload }));
      }),
      listen<DownloadComplete>("model:download:complete", async (e) => {
        setWhisperProgress((p) => {
          const next = { ...p };
          delete next[e.payload.id];
          return next;
        });
        setWhisperErrors((er) => {
          const next = { ...er };
          delete next[e.payload.id];
          return next;
        });
        await refresh();
        if (!selectedRef.current) onSelectModel(e.payload.id);
      }),
      listen<DownloadError>("model:download:error", (e) => {
        setWhisperProgress((p) => {
          const next = { ...p };
          delete next[e.payload.id];
          return next;
        });
        setWhisperErrors((er) => ({
          ...er,
          [e.payload.id]: e.payload.message,
        }));
      }),
      listen<ParakeetDownloadProgress>(
        "parakeet_model:download:progress",
        (e) => {
          setParakeetProgress((p) => ({ ...p, [e.payload.id]: e.payload }));
        },
      ),
      listen<{ id: string; path: string }>(
        "parakeet_model:download:complete",
        (e) => {
          setParakeetProgress((p) => {
            const { [e.payload.id]: _, ...rest } = p;
            return rest;
          });
          refresh();
        },
      ),
      // Le backend emet cet event sur annulation, echec HTTP ou toute
      // erreur download_impl : on nettoie la barre + on affiche le message.
      listen<{ id: string; message: string }>(
        "parakeet_model:download:error",
        (e) => {
          setParakeetProgress((p) => {
            const { [e.payload.id]: _, ...rest } = p;
            return rest;
          });
          setParakeetStatus((s) => ({
            ...s,
            [e.payload.id]: e.payload.message.includes("annule")
              ? t("parakeet.cancelled")
              : t("parakeet.errorPrefix", { message: e.payload.message }),
          }));
        },
      ),
    ];
    return () => {
      Promise.all(unlisteners).then((arr) => arr.forEach((fn) => fn()));
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // --- Whisper ---

  async function downloadWhisper(id: string) {
    // Entree de progression optimiste : bloque le multi-clic, le backend a
    // aussi son propre garde de reentrance.
    if (whisperProgress[id]) return;
    setWhisperProgress((p) => ({
      ...p,
      [id]: { id, downloaded: 0, total: 0 },
    }));
    setWhisperErrors((er) => {
      const next = { ...er };
      delete next[id];
      return next;
    });
    try {
      await api.downloadWhisperModel(id);
    } catch (e) {
      setWhisperProgress((p) => {
        const next = { ...p };
        delete next[id];
        return next;
      });
      setWhisperErrors((er) => ({ ...er, [id]: String(e) }));
    }
  }

  async function deleteWhisper(id: string) {
    try {
      await api.deleteWhisperModel(id);
      if (selectedRef.current === id) onSelectModel(null);
      await refresh();
    } catch (e) {
      setWhisperErrors((er) => ({ ...er, [id]: String(e) }));
    }
  }

  async function setDefaultWhisper(id: string) {
    // selected_whisper_model (via App.handleSelectModel) puis bascule du
    // kind : set_transcription_source n'ecrit pas selected_whisper_model,
    // et set_transcription_kind preserve les selections cloud/parakeet.
    onSelectModel(id);
    try {
      await api.setTranscriptionKind("local");
    } catch (e) {
      console.error(e);
    }
  }

  // --- Parakeet ---

  async function downloadParakeet(id: string) {
    setParakeetProgress((p) => ({
      ...p,
      [id]: { id, downloaded: 0, total: 0, current_file: "" },
    }));
    setParakeetStatus((s) => ({ ...s, [id]: t("parakeet.downloading") }));
    try {
      await api.downloadParakeetModel(id);
      setParakeetStatus((s) => ({ ...s, [id]: "" }));
    } catch (e) {
      setParakeetProgress((p) => {
        const { [id]: _, ...rest } = p;
        return rest;
      });
      setParakeetStatus((s) => ({
        ...s,
        [id]: t("parakeet.errorPrefix", { message: String(e) }),
      }));
    }
  }

  async function deleteParakeet(id: string) {
    if (!confirm(t("parakeet.confirmDelete", { id }))) return;
    try {
      await api.deleteParakeetModel(id);
      await refresh();
    } catch (e) {
      setParakeetStatus((s) => ({
        ...s,
        [id]: t("parakeet.errorPrefix", { message: String(e) }),
      }));
    }
  }

  async function setDefaultParakeet(id: string) {
    try {
      await api.setTranscriptionSource({
        kind: "parakeet",
        whisper_model_id: source?.whisper_model_id,
        cloud_provider: source?.cloud_provider,
        cloud_model: source?.cloud_model,
        parakeet_model_id: id,
      });
    } catch (e) {
      console.error(e);
    }
  }

  // --- Cloud ---

  /// Verifie puis sauvegarde la cle. N'active RIEN : le bouton de la card
  /// devient "Set as Default" et l'activation reste un geste explicite.
  async function verifyAndSaveKey(providerId: string, key: string) {
    await api.verifyApiKey(providerId, key);
    await api.setApiKey(providerId, key);
    await refresh();
  }

  async function setDefaultCloud(m: CloudModel) {
    try {
      await api.setTranscriptionSource({
        kind: "cloud",
        whisper_model_id: source?.whisper_model_id,
        cloud_provider: m.provider_id,
        cloud_model: m.model_id,
        parakeet_model_id: source?.parakeet_model_id,
      });
    } catch (e) {
      console.error(e);
    }
  }

  async function removeKey(providerId: string) {
    try {
      await api.deleteApiKey(providerId);
      // Le provider actif perd sa cle -> retomber sur local (equivalent
      // VoiceInk clearCurrentTranscriptionModel dans clearAPIKey).
      if (source?.kind === "cloud" && source.cloud_provider === providerId) {
        await api.setTranscriptionKind("local");
      }
      await refresh();
    } catch (e) {
      console.error(e);
    }
  }

  // --- Liste unifiee ---

  const rows = useMemo<ModelRow[]>(() => {
    const whisperRows: ModelRow[] = whisper.map((m) => ({
      type: "whisper",
      key: m.id,
      model: m,
    }));
    const parakeetRows: ModelRow[] = parakeet.map((m) => ({
      type: "parakeet",
      key: m.id,
      model: m,
    }));
    // Comme l'ancien panneau cloud : seuls les modeles batch sont
    // activables ici (les streaming-only ont leur propre chemin pipeline).
    const cloudRows: ModelRow[] = cloudModels
      .filter((m) => m.supports_batch)
      .map((m) => ({
        type: "cloud",
        key: `${m.provider_id}:${m.model_id}`,
        model: m,
      }));
    switch (filter) {
      case "local":
        return [...whisperRows, ...parakeetRows];
      case "cloud":
        return cloudRows;
      case "recommended": {
        const all = [...whisperRows, ...parakeetRows, ...cloudRows];
        return all
          .filter((r) => RECOMMENDED_MODELS.includes(rowName(r)))
          .sort(
            (a, b) =>
              RECOMMENDED_MODELS.indexOf(rowName(a)) -
              RECOMMENDED_MODELS.indexOf(rowName(b)),
          );
      }
    }
  }, [whisper, parakeet, cloudModels, filter]);

  const providerById = useMemo(
    () => new Map(providers.map((p) => [p.id, p])),
    [providers],
  );

  const defaultDisplayName = useMemo(
    () => resolveDefaultDisplayName(source, whisper, parakeet, cloudModels),
    [source, whisper, parakeet, cloudModels],
  );

  const showCpuHint = ep === "cpu" && rows.some((r) => r.type === "parakeet");

  return (
    <div className="space-y-4">
      <DefaultModelCard displayName={defaultDisplayName} />

      <DictationLanguagePanel />

      <ModelFilterPills value={filter} onChange={setFilter} />

      {showCpuHint && (
        <div className="flex flex-wrap items-center gap-2 rounded-md border p-3 text-xs">
          <span className="font-medium">{t("parakeet.executionProvider")}</span>
          <code>{ep}</code>
          <span className="text-muted-foreground">{t("parakeet.cpuHint")}</span>
        </div>
      )}

      <div className="space-y-3">
        {rows.map((row) => {
          const current = isRowCurrent(row, source);
          switch (row.type) {
            case "whisper":
              return (
                <WhisperModelCard
                  key={row.key}
                  model={row.model}
                  isCurrent={current}
                  progress={whisperProgress[row.model.id] ?? null}
                  error={whisperErrors[row.model.id] ?? null}
                  onDownload={() => downloadWhisper(row.model.id)}
                  onCancelDownload={() =>
                    api.cancelDownloadWhisperModel(row.model.id)
                  }
                  onDelete={() => deleteWhisper(row.model.id)}
                  onSetDefault={() => setDefaultWhisper(row.model.id)}
                />
              );
            case "parakeet":
              return (
                <ParakeetModelCard
                  key={row.key}
                  model={row.model}
                  isCurrent={current}
                  progress={parakeetProgress[row.model.id] ?? null}
                  status={parakeetStatus[row.model.id] || null}
                  onDownload={() => downloadParakeet(row.model.id)}
                  onCancelDownload={() =>
                    api.cancelDownloadParakeetModel(row.model.id)
                  }
                  onDelete={() => deleteParakeet(row.model.id)}
                  onSetDefault={() => setDefaultParakeet(row.model.id)}
                />
              );
            case "cloud": {
              const provider = providerById.get(row.model.provider_id);
              return (
                <CloudModelCard
                  key={row.key}
                  model={row.model}
                  providerName={
                    provider?.display_name ?? row.model.provider_id
                  }
                  apiKeyUrl={provider?.api_key_url ?? null}
                  isConfigured={provider?.has_api_key ?? false}
                  isCurrent={current}
                  onVerifyAndSave={(key) =>
                    verifyAndSaveKey(row.model.provider_id, key)
                  }
                  onSetDefault={() => setDefaultCloud(row.model)}
                  onRemoveKey={() => removeKey(row.model.provider_id)}
                />
              );
            }
          }
        })}

        {filter === "local" && (
          <ImportModelCard
            onImported={async (id) => {
              await refresh();
              onSelectModel(id);
            }}
          />
        )}
      </div>
    </div>
  );
}
