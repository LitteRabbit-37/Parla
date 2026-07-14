// Pill switcher Recommended / Local / Cloud - replique le "modern compact
// pill switcher" de VoiceInk ModelManagementView.availableModelsSection.
// Purement visuel : changer de filtre ne touche jamais la source active.

import { useTranslation } from "react-i18next";
import { cn } from "@/lib/utils";
import type { ModelFilter } from "./types";

const FILTERS: ModelFilter[] = ["recommended", "local", "cloud"];

export function ModelFilterPills({
  value,
  onChange,
}: {
  value: ModelFilter;
  onChange: (filter: ModelFilter) => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="flex items-center gap-2">
      {FILTERS.map((f) => (
        <button
          key={f}
          onClick={() => onChange(f)}
          className={cn(
            "rounded-full border px-4 py-1.5 text-sm transition-colors",
            value === f
              ? "border-primary bg-primary/10 font-semibold"
              : "border-transparent text-muted-foreground hover:bg-accent/40 hover:text-foreground",
          )}
        >
          {t(`aiModels.filter.${f}`)}
        </button>
      ))}
    </div>
  );
}
