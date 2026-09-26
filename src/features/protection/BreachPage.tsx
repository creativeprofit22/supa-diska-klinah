import { useEffect, useState, type FormEvent } from "react";
import { useStrings } from "../../shared/i18n/I18nProvider";
import { checkPasswordBreach, getOverview } from "./api";
import { EvidenceBadge, errorMessage } from "./labels";
import { protectionStrings } from "./strings";
import { MAX_PASSWORD_BYTES, type PasswordBreachResult } from "./types";

/** English message; the page shows the active locale's `breach.tooLong`. */
export const PASSWORD_TOO_LONG = protectionStrings.en.breach.tooLong(String(MAX_PASSWORD_BYTES));

export function BreachPage() {
  const t = useStrings(protectionStrings).breach;
  const [enabled, setEnabled] = useState<boolean | null>(null);
  const [password, setPassword] = useState("");
  const [result, setResult] = useState<PasswordBreachResult | null>(null);
  const [checking, setChecking] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void getOverview().then((overview) => setEnabled(overview.settings.network.passwordBreachCheck)).catch(() => setEnabled(false));
  }, []);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!password) return;
    // maxLength counts UTF-16 units, so multi-byte input can still exceed the backend's byte limit.
    if (new TextEncoder().encode(password).length > MAX_PASSWORD_BYTES) {
      setResult(null);
      setError(t.tooLong(String(MAX_PASSWORD_BYTES)));
      return;
    }
    setChecking(true);
    setError(null);
    setResult(null);
    const value = password;
    setPassword("");
    try {
      setResult(await checkPasswordBreach(value));
    } catch (reason) {
      setError(errorMessage(reason, t.failed));
    } finally {
      setChecking(false);
    }
  };

  return <section aria-labelledby="breach-title">
    <h2 id="breach-title">{t.title}</h2>
    <p>{t.intro}</p>
    <p className="protection-hint">{t.emailNote}</p>
    {enabled === false && <p role="note">{t.off}</p>}
    <form onSubmit={(event) => void submit(event)}>
      <label>{t.password} <input type="password" autoComplete="off" maxLength={MAX_PASSWORD_BYTES} value={password} disabled={!enabled || checking} onChange={(event) => setPassword(event.target.value)} /></label>
      <button type="submit" disabled={!enabled || checking || !password}>{t.check}</button>
    </form>
    {checking && <p role="status">{t.checking}</p>}
    {error && <p role="alert">{error}</p>}
    {result && <p role="status"><EvidenceBadge evidence={result.evidence} /> {result.evidence.kind === "external" ? result.evidence.detail : ""}</p>}
  </section>;
}
