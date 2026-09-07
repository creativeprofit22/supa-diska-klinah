import { useCallback, useEffect, useState } from "react";
import { driveInventoryError, listDriveInventory, type DriveInventory } from "./api";

type InventoryState =
  | { kind: "loading" }
  | { kind: "error"; message: string }
  | { kind: "ready"; inventory: DriveInventory };

export function useDriveInventory() {
  const [state, setState] = useState<InventoryState>({ kind: "loading" });
  const [request, setRequest] = useState(0);
  const refresh = useCallback(() => {
    setState({ kind: "loading" });
    setRequest((value) => value + 1);
  }, []);

  useEffect(() => {
    let active = true;
    listDriveInventory().then(
      (inventory) => { if (active) setState({ kind: "ready", inventory }); },
      (reason: unknown) => {
        if (active) setState({ kind: "error", message: driveInventoryError(reason) });
      },
    );
    return () => { active = false; };
  }, [request]);

  return { state, refresh };
}
