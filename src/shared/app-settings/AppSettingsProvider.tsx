import { createContext, type ReactNode, useCallback, useContext, useEffect, useMemo, useRef, useState } from "react";
import { useI18n } from "../i18n/I18nProvider";
import { type AppSettings, DEFAULT_APP_SETTINGS, getAppSettings, setAppSettings } from "./api";

type SaveOutcome = "saved" | "failed";

type AppSettingsContextValue = Readonly<{
  settings: AppSettings;
  /** False until the saved settings were read (defaults are used meanwhile). */
  loaded: boolean;
  save: (next: AppSettings) => Promise<SaveOutcome>;
}>;

const AppSettingsContext = createContext<AppSettingsContextValue>({
  settings: DEFAULT_APP_SETTINGS,
  loaded: false,
  save: async () => "failed",
});

type Loader = Readonly<{ get: typeof getAppSettings; set: typeof setAppSettings }>;
const IPC: Loader = { get: getAppSettings, set: setAppSettings };

/**
 * Loads app-wide preferences once and applies the saved language. Must sit
 * inside `I18nProvider`. A failed load keeps safe defaults (system language,
 * update check off) rather than blocking the app.
 */
export function AppSettingsProvider({ children, loader = IPC }: Readonly<{ children: ReactNode; loader?: Loader }>): ReactNode {
  const { setPreference } = useI18n();
  const [settings, setSettings] = useState<AppSettings>(DEFAULT_APP_SETTINGS);
  const [loaded, setLoaded] = useState(false);
  const loaderRef = useRef(loader);

  useEffect(() => {
    let active = true;
    void (async () => {
      const result = await loaderRef.current.get();
      if (!active) return;
      if (result.ok) {
        setSettings(result.value);
        setPreference(result.value.language);
      }
      setLoaded(true);
    })();
    return () => {
      active = false;
    };
  }, [setPreference]);

  const save = useCallback(async (next: AppSettings): Promise<SaveOutcome> => {
    const result = await loaderRef.current.set(next);
    if (!result.ok) return "failed";
    setSettings(result.value);
    setPreference(result.value.language);
    return "saved";
  }, [setPreference]);

  const value = useMemo(() => ({ settings, loaded, save }), [settings, loaded, save]);
  return <AppSettingsContext.Provider value={value}>{children}</AppSettingsContext.Provider>;
}

export function useAppSettings(): AppSettingsContextValue {
  return useContext(AppSettingsContext);
}
