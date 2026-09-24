import { useEffect, useState, type FormEvent } from "react";
import { checkPasswordBreach, getOverview } from "./api";
import { EvidenceBadge, errorMessage } from "./labels";
import { MAX_PASSWORD_BYTES, type PasswordBreachResult } from "./types";

export const PASSWORD_TOO_LONG = `Passwords longer than ${MAX_PASSWORD_BYTES} bytes can't be checked.`;

export function BreachPage() {
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
      setError(PASSWORD_TOO_LONG);
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
      setError(errorMessage(reason, "The check could not be completed. Nothing about the result is known."));
    } finally {
      setChecking(false);
    }
  };

  return <section aria-labelledby="breach-title">
    <h2 id="breach-title">Password breach check</h2>
    <p>Checks whether a password appears in the public Pwned Passwords list. Only the first 5 characters of its SHA-1 hash are sent; the match happens on this PC. The password is not stored.</p>
    <p className="protection-hint">E-mail breach monitoring is not offered because it would send your address to a third party.</p>
    {enabled === false && <p role="note">This check is off. Turn on "Allow the password breach check" in Overview to use it.</p>}
    <form onSubmit={(event) => void submit(event)}>
      <label>Password <input type="password" autoComplete="off" maxLength={MAX_PASSWORD_BYTES} value={password} disabled={!enabled || checking} onChange={(event) => setPassword(event.target.value)} /></label>
      <button type="submit" disabled={!enabled || checking || !password}>Check</button>
    </form>
    {checking && <p role="status">Checking…</p>}
    {error && <p role="alert">{error}</p>}
    {result && <p role="status"><EvidenceBadge evidence={result.evidence} /> {result.evidence.kind === "external" ? result.evidence.detail : ""}</p>}
  </section>;
}
