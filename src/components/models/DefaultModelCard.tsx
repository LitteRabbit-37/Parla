// Header "Default Model" - replique VoiceInk
// ModelManagementView.defaultModelSection : label secondaire + displayName
// du modele actif en grand, ou "No model selected".

import { useTranslation } from "react-i18next";
import { Card, CardContent } from "@/components/ui/card";

export function DefaultModelCard({
  displayName,
}: {
  displayName: string | null;
}) {
  const { t } = useTranslation();
  return (
    <Card>
      <CardContent className="p-6">
        <p className="text-sm font-medium text-muted-foreground">
          {t("aiModels.defaultModel")}
        </p>
        <p className="mt-1 text-2xl font-bold">
          {displayName ?? t("aiModels.noModelSelected")}
        </p>
      </CardContent>
    </Card>
  );
}
