import { copyFileSync, mkdirSync } from "node:fs";

const publicDir = new URL("./public/", import.meta.url);
mkdirSync(publicDir, { recursive: true });
copyFileSync(new URL("../../scripts/install.sh", import.meta.url), new URL("install.sh", publicDir));
copyFileSync(new URL("./_headers", import.meta.url), new URL("_headers", publicDir));
