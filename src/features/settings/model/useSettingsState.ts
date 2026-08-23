import { useState } from "react";

export function useSettingsState() {
  const [showAdvanced, setShowAdvanced] = useState(false);

  return { showAdvanced, setShowAdvanced };
}
