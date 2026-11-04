import { defineConfig } from "vite"
import vue from "@vitejs/plugin-vue"
import tailwindcss from "@tailwindcss/vite"
import { existsSync, readFileSync, readdirSync } from "node:fs"
import { fileURLToPath } from "node:url"

/** The `dimcode web` entry the dev server forwards /api and /ws to. */
const target = process.env.DIM_WEB_TARGET ?? "http://127.0.0.1:43201"
const unused = fileURLToPath(new URL("./src/vendor/unused-optional.ts", import.meta.url))

export default defineConfig({
  base: "/",
  resolve: {
    alias: [
      // Curated shiki bundle on the JavaScript regex engine (see src/vendor/shiki-web.ts).
      { find: /^shiki$/, replacement: fileURLToPath(new URL("./src/vendor/shiki-web.ts", import.meta.url)) },
      { find: /^shiki\/wasm$/, replacement: unused },
      { find: /^@terrastruct\/d2$/, replacement: unused },
    ],
  },
  plugins: [tailwindcss(), vue(), {
    name: "third-party-notices",
    apply: "build",
    generateBundle() {
      const lock = JSON.parse(readFileSync(new URL("./package-lock.json", import.meta.url), "utf8"))
      const notices = [readFileSync(new URL("../NOTICE", import.meta.url), "utf8")]
      for (const directory of Object.keys(lock.packages).sort()) {
        const pkg = lock.packages[directory]
        if (!directory || pkg.dev) continue
        const root = new URL(`./${directory}/`, import.meta.url)
        if (pkg.optional && !existsSync(root)) continue
        const name = directory.slice(directory.lastIndexOf("node_modules/") + "node_modules/".length)
        const files = readdirSync(root, { withFileTypes: true })
          .filter(file => file.isFile() && /^(licen[cs]e|copying|notice|ofl)(?:[.-]|$)/i.test(file.name))
          .map(file => new URL(file.name, root))
        if (files.length === 0) files.push(new URL(`./licenses/${name}/${pkg.version}/LICENSE`, import.meta.url))
        notices.push(`${name}@${pkg.version}`, ...files.map(file => readFileSync(file, "utf8")))
      }
      this.emitFile({ type: "asset", fileName: "THIRD-PARTY-NOTICES.txt", source: notices.join("\n\n") })
    },
  }],
  server: {
    proxy: {
      "/api": { target, changeOrigin: true, headers: { origin: target } },
      "/ws": { target, changeOrigin: true, ws: true, headers: { origin: target } },
    },
  },
  build: {
    outDir: "dist",
    assetsInlineLimit: 0,
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (!id.includes("/node_modules/")) return undefined
          if (/\/node_modules\/(@vue|vue|vue-router|vue-i18n|@intlify|pinia)\//.test(id)) return "vendor-vue"
          return undefined
        },
      },
    },
  },
})
