import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

/**
 * Libelle du raccourci Alt+chiffre du Nieme profil Power Mode active (0-base).
 * 0..8 -> Alt+1..Alt+9, 9 -> Alt+0 (comme VoiceInk Option+1..0). Retourne
 * null au-dela du 10e profil (aucun raccourci) ou pour un index invalide.
 * L'ordre doit correspondre au backend (session::enabled_configs : configs
 * filtrees par is_enabled, dans l'ordre stocke).
 */
export function powerShortcutLabel(enabledIndex: number): string | null {
  if (enabledIndex < 0 || enabledIndex > 9) return null;
  return enabledIndex < 9 ? `Alt+${enabledIndex + 1}` : "Alt+0";
}
