// Page AI Models : selecteur de source en haut, puis le panneau correspondant.
//
// La `kind` du store (source ACTIVE - ce que le pipeline utilise a
// l'enregistrement) et la `viewKind` (quel panneau on affiche) sont deux
// choses distinctes ici. Cliquer un tile change uniquement la vue. Le
// kind actif ne change que quand l'utilisateur clique "Activer" sur un
// modele specifique dans le sous-panneau correspondant. Ca evite le
// piege "je clique Whisper juste pour voir" qui desactivait le Parakeet
// en arriere-plan.

import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { CloudProvidersPanel } from "@/components/CloudProvidersPanel";
import { DictationLanguagePanel } from "@/components/DictationLanguagePanel";
import { ModelsPanel } from "@/components/ModelsPanel";
import { ParakeetPanel } from "@/components/ParakeetPanel";
import { TranscriptionSourcePanel } from "@/components/TranscriptionSourcePanel";
import { api, type TranscriptionSource } from "@/lib/tauri";

type Kind = "local" | "cloud" | "parakeet";

export function ModelsPage({
  selectedModelId,
  onSelectModel,
}: {
  selectedModelId: string | null;
  onSelectModel: (id: string | null) => void;
}) {
  // Active kind (what the pipeline uses).
  const [activeKind, setActiveKind] = useState<Kind>("local");
  // View kind (what panel is displayed). Defaults to activeKind on mount
  // but is NOT tied to the store - user can switch freely without
  // changing what's active.
  const [viewKind, setViewKind] = useState<Kind>("local");

  useEffect(() => {
    refresh();
    const un = listen<TranscriptionSource>("source:changed", (e) => {
      if (e.payload?.kind) {
        setActiveKind(e.payload.kind);
        // When the active kind changes from outside (Parakeet Activer
        // button, PowerMode session, etc.) bring the view in sync so
        // the user sees where the switch happened.
        setViewKind(e.payload.kind);
      }
    });
    return () => {
      un.then((fn) => fn());
    };
  }, []);

  async function refresh() {
    try {
      const s = await api.getTranscriptionSource();
      setActiveKind(s.kind);
      setViewKind(s.kind);
    } catch (e) {
      console.error(e);
    }
  }

  return (
    <div className="space-y-4">
      <TranscriptionSourcePanel
        viewKind={viewKind}
        activeKind={activeKind}
        onViewKindChange={setViewKind}
      />

      <DictationLanguagePanel />

      {viewKind === "local" && (
        <ModelsPanel
          selectedId={selectedModelId}
          onSelect={onSelectModel}
        />
      )}

      {viewKind === "parakeet" && <ParakeetPanel />}

      {viewKind === "cloud" && <CloudProvidersPanel />}
    </div>
  );
}
