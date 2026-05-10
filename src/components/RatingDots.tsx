// Cinq cercles de rating pour vitesse / precision, alignes sur VoiceInk
// progressDotsWithNumber dans Views/AI Models/WhisperModelCardView.swift.
// Couleurs (vert/jaune/orange/rouge) basees sur la valeur normalisee 0..1.

import { cn } from "@/lib/utils";

type Props = {
  /** Note entre 0 et 1 (ex 0.95 = 95%). */
  value: number;
  className?: string;
  /** Affiche aussi le nombre formate ("9.5"). True par defaut. */
  showLabel?: boolean;
};

function performanceColor(value: number): string {
  if (value >= 0.8) return "fill-green-500";
  if (value >= 0.6) return "fill-yellow-500";
  if (value >= 0.4) return "fill-orange-500";
  return "fill-red-500";
}

export function RatingDots({ value, className, showLabel = true }: Props) {
  // VoiceInk : `Int(value * 10 / 2)` = floor(value * 5).
  const filled = Math.floor(value * 5);
  const colorClass = performanceColor(value);
  const display = (value * 10).toFixed(1);

  return (
    <span className={cn("inline-flex items-center gap-1", className)}>
      <span className="inline-flex items-center gap-0.5">
        {Array.from({ length: 5 }, (_, i) => (
          <svg
            key={i}
            viewBox="0 0 6 6"
            className={cn(
              "h-1.5 w-1.5",
              i < filled ? colorClass : "fill-muted-foreground/30",
            )}
            aria-hidden="true"
          >
            <circle cx="3" cy="3" r="3" />
          </svg>
        ))}
      </span>
      {showLabel && (
        <span className="font-mono text-[10px] text-muted-foreground">
          {display}
        </span>
      )}
    </span>
  );
}
