import { defineConfig } from "cf/config";

export default defineConfig({
  worker: {
    name: "gds-installer",
    compatibilityDate: "2026-11-04",
    domains: ["gds.gandesktop.dev"],
    workersDev: false,
    previewUrls: false,
  },
});
