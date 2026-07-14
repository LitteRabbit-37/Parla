// Card modele cloud - replique fidele de VoiceInk CloudModelCardView :
// bouton d'action a 3 etats (Configure -> Set as Default -> Default Model),
// section de configuration de la cle API inline depliable, menu
// "Remove API Key". Verifier une cle la sauvegarde mais n'active jamais le
// modele : l'activation reste le geste explicite "Set as Default" (issue #6,
// meme comportement que VoiceInk).
//
// Divergence assumee : pas de pre-remplissage de la cle sauvegardee (le
// Credential Manager n'est pas relisible cote front, seul has_api_key
// existe). Changer de cle = Remove API Key puis Configure.

import { useState } from "react";
import { useTranslation } from "react-i18next";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  Cloud,
  ExternalLink,
  Loader2,
  MoreHorizontal,
  Settings,
  ShieldCheck,
  Trash2,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { RatingDots } from "@/components/RatingDots";
import { cn } from "@/lib/utils";
import type { CloudModel } from "./types";

type VerifyStatus = "none" | "success" | "failure";

type Props = {
  model: CloudModel;
  providerName: string;
  apiKeyUrl: string | null;
  isConfigured: boolean;
  isCurrent: boolean;
  /** Verifie puis sauvegarde la cle ; doit throw si la verification echoue. */
  onVerifyAndSave: (key: string) => Promise<void>;
  onSetDefault: () => void;
  onRemoveKey: () => void;
};

export function CloudModelCard({
  model: m,
  providerName,
  apiKeyUrl,
  isConfigured,
  isCurrent,
  onVerifyAndSave,
  onSetDefault,
  onRemoveKey,
}: Props) {
  const { t } = useTranslation();
  const [isExpanded, setIsExpanded] = useState(false);
  const [apiKey, setApiKey] = useState("");
  const [verifying, setVerifying] = useState(false);
  const [verifyStatus, setVerifyStatus] = useState<VerifyStatus>("none");
  const [verifyError, setVerifyError] = useState<string | null>(null);

  async function verify() {
    const key = apiKey.trim();
    if (!key || verifying) return;
    setVerifying(true);
    setVerifyStatus("none");
    setVerifyError(null);
    try {
      await onVerifyAndSave(key);
      // Cle valide et sauvee : la section se referme et le bouton de la
      // card devient "Set as Default" (isConfigured passe a true au
      // refresh du parent) - comme VoiceInk.
      setVerifyStatus("success");
      setApiKey("");
      setIsExpanded(false);
    } catch (e) {
      setVerifyStatus("failure");
      setVerifyError(String(e));
    } finally {
      setVerifying(false);
    }
  }

  return (
    <div
      className={cn(
        "rounded-lg border transition-colors",
        isCurrent && "border-primary bg-primary/5",
      )}
    >
      <div className="flex items-start justify-between gap-3 p-4">
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <p className="font-medium">{m.display_name}</p>
            {m.supports_streaming && (
              <span className="rounded bg-muted px-1.5 py-0.5 text-[10px] text-muted-foreground">
                {t("aiModels.cloud.streaming")}
              </span>
            )}
          </div>
          <div className="mt-1 flex flex-wrap items-center gap-3 text-[11px] text-muted-foreground">
            <span className="inline-flex items-center gap-1">
              <Cloud className="h-3 w-3" />
              {providerName}
            </span>
            <span className="inline-flex items-center gap-1">
              {t("common.speed")}
              <RatingDots value={m.speed} />
            </span>
            <span className="inline-flex items-center gap-1">
              {t("common.accuracy")}
              <RatingDots value={m.accuracy} />
            </span>
          </div>
          <p className="mt-1 text-xs text-muted-foreground">{m.notes}</p>
        </div>
        <div className="flex shrink-0 items-center gap-1.5">
          {isCurrent ? (
            <span className="px-3 py-1.5 text-xs font-medium text-muted-foreground">
              {t("aiModels.defaultModel")}
            </span>
          ) : isConfigured ? (
            <Button size="sm" variant="outline" onClick={onSetDefault}>
              {t("aiModels.setAsDefault")}
            </Button>
          ) : (
            <Button size="sm" onClick={() => setIsExpanded((v) => !v)}>
              <Settings className="h-3.5 w-3.5" />
              {t("aiModels.configure")}
            </Button>
          )}
          {isConfigured && (
            <Popover>
              <PopoverTrigger asChild>
                <Button size="icon" variant="ghost" className="h-8 w-8">
                  <MoreHorizontal className="h-4 w-4" />
                </Button>
              </PopoverTrigger>
              <PopoverContent align="end" className="w-auto p-1">
                <Button
                  size="sm"
                  variant="ghost"
                  className="text-destructive hover:text-destructive"
                  onClick={onRemoveKey}
                >
                  <Trash2 className="h-3.5 w-3.5" />
                  {t("aiModels.removeApiKey")}
                </Button>
              </PopoverContent>
            </Popover>
          )}
        </div>
      </div>

      {isExpanded && !isConfigured && (
        <div className="border-t p-4">
          <div className="flex items-center justify-between gap-2">
            <p className="text-sm font-medium">
              {t("aiModels.cloud.apiKeyConfig")}
            </p>
            {apiKeyUrl && (
              <button
                type="button"
                onClick={() => openUrl(apiKeyUrl)}
                className="inline-flex items-center gap-1 text-[11px] font-medium text-primary hover:underline"
              >
                <ExternalLink className="h-3 w-3" />
                {t("aiModels.getApiKey")}
              </button>
            )}
          </div>
          <div className="mt-2 grid grid-cols-[1fr_auto] gap-2">
            <input
              type="password"
              placeholder={t("aiModels.cloud.apiKeyPlaceholder", {
                provider: providerName,
              })}
              value={apiKey}
              onChange={(e) => setApiKey(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") verify();
              }}
              disabled={verifying}
              className="flex h-9 rounded-md border border-input bg-background px-3 text-sm shadow-sm"
              autoComplete="off"
            />
            <Button
              size="sm"
              className="h-9"
              onClick={verify}
              disabled={!apiKey.trim() || verifying}
            >
              {verifying ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <ShieldCheck className="h-3.5 w-3.5" />
              )}
              {verifying
                ? t("aiModels.cloud.verifying")
                : t("aiModels.cloud.verify")}
            </Button>
          </div>
          {verifyStatus === "failure" && (
            <p className="mt-2 text-xs text-destructive">
              {verifyError ?? t("aiModels.cloud.verifyFailed")}
            </p>
          )}
          {verifyStatus === "success" && (
            <p className="mt-2 text-xs text-green-600 dark:text-green-400">
              {t("aiModels.cloud.verifySuccess")}
            </p>
          )}
        </div>
      )}
    </div>
  );
}
