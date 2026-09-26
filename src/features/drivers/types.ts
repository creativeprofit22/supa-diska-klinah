/** Mirrors `windows_platform::drivers::DriverPackage` (serde camelCase). */
export type DriverPackageStatus = "inUse" | "current" | "superseded";
export interface DriverPackage {
  publishedName: string;
  originalName: string | null;
  provider: string | null;
  class: string | null;
  driverDate: string | null;
  driverVersion: string | null;
  status: DriverPackageStatus;
  deletable: boolean;
}
