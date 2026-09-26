// @vitest-environment jsdom
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { I18nProvider } from "../i18n/I18nProvider";
import type { CleanupExecutionSummary } from "../cleanup/api";
import { CleanupOutcomes } from "./CleanupOutcomes";
import { StoragePaging, StorageScanStatus } from "./StorageScanStatus";
import { storageError } from "./api";
import { storageStrings } from "./strings";

afterEach(cleanup);

const execution: CleanupExecutionSummary = {executionId:"e".repeat(32), planId:"c".repeat(32), disposition:"recycleBin", completed:true,
  items:[{itemId:"f".repeat(32),state:"recycled",logicalBytes:512},{itemId:"locked",state:"failed",logicalBytes:0,failure:"in-use"}],
  accounting:{selectedBytes:512,processedBytes:512,failedBytes:0,quarantinedBytes:0,purgedBytes:0,occupiedBytes:512,reclaimedBytes:1536}};

it("renders cleanup outcomes in Latin American Spanish", () => {
  render(<I18nProvider languages={["es-MX"]}><CleanupOutcomes value={execution} expanded/></I18nProvider>);
  expect(screen.getByText("Resultados por elemento")).toBeTruthy();
  expect(screen.getByText("2 elementos en total · 1 elemento fallido · 0 elementos inciertos · 1 elemento confirmado")).toBeTruthy();
  expect(screen.getByText(/^1.5 KB recuperados/)).toBeTruthy();
  expect(screen.getByText("Conservado en la Papelera de reciclaje · 512 B de tamaño lógico")).toBeTruthy();
  expect(screen.getByText(/está abierto en otro programa/)).toBeTruthy();
  expect(screen.getByRole("button", {name:"Elementos siguientes"})).toBeTruthy();
});

it("renders scan status and paging in Latin American Spanish", () => {
  render(<I18nProvider languages={["es-MX"]}>
    <StorageScanStatus phase="scanning" error={null} cancel={() => undefined}
      status={{snapshotId:"a".repeat(32),module:"largeFiles",phase:"walking",visitedEntries:12345,retainedRecords:8,hashedBytes:0,completedHashes:0,completeness:{reasons:[]}}}/>
    <StoragePaging loading={false} hasNext count={4} total={8} firstPage={() => undefined} nextPage={() => undefined}/>
  </I18nProvider>);
  expect(screen.getByRole("region", {name:"Estado del análisis de almacenamiento"})).toBeTruthy();
  expect(screen.getByText("Analizando. No se está modificando ningún archivo.")).toBeTruthy();
  expect(screen.getByText(/entradas revisadas · 8 registros conservados$/)).toBeTruthy();
  expect(screen.getByRole("button", {name:"Cancelar análisis"})).toBeTruthy();
  expect(screen.getByText("4 registros en esta página; 8 conservados en esta vista.")).toBeTruthy();
});

it("localizes storage errors and keeps English as the default", () => {
  expect(storageError({code:"busy"})).toBe("Another scan is active. Cancel it or wait before starting again.");
  expect(storageError({code:"busy"}, storageStrings.es419.errors)).toBe("Hay otro análisis en curso. Cancélalo o espera antes de empezar de nuevo.");
  expect(storageError(new Error("x"), storageStrings.es419.errors)).toBe(storageStrings.es419.errors.fallback);
});
