import { computed, type ComputedRef } from "vue"
import { useI18n } from "vue-i18n"
import type { ImageBlock } from "../../rpc/protocol"
import type { ComposerDraft } from "./composer-drafts"

/** The files the attachment button offers. */
export const FILE_ACCEPT = "image/png,image/jpeg,image/webp,image/gif,application/pdf,.pdf,text/*,application/json,application/javascript,application/xml,.ts,.tsx,.jsx,.vue,.rs,.go,.py,.php,.sql,.md,.yaml,.yml,.toml,.ini,.sh,.css,.scss,.jsonc,.log,.diff,.patch,.svg"

/** The limits of `crates/dim-cli/src/attachments.rs`: a paste longer than this is stored as a Session attachment. */
const INLINE_PASTE = 16 * 1024
const MAX_PASTE = 10 * 1024 * 1024
const MAX_ATTACHMENTS = 20
const IMAGE_TYPES = new Set(["image/png", "image/jpeg", "image/webp", "image/gif"])

function count(target: ComposerDraft): number {
  return target.images.length + target.pastes.length + target.files.length
}

/**
 * The draft's attachments: pasted, dropped or chosen images, PDFs and text files,
 * and long pastes kept aside, within the CLI's limits. `enabled` is false where a
 * draft carries no attachments; `storesPastes` is true where the Session stores
 * text attachments. A read that finishes after the draft changed still lands on
 * the draft that started it.
 */
export function useComposerAttachments(draft: ComputedRef<ComposerDraft>, options: { enabled: () => boolean; storesPastes: () => boolean }) {
  const { t } = useI18n()
  const images = computed({ get: () => draft.value.images, set: (value) => { draft.value.images = value } })
  const pastes = computed({ get: () => draft.value.pastes, set: (value) => { draft.value.pastes = value } })
  const files = computed({ get: () => draft.value.files, set: (value) => { draft.value.files = value } })
  const empty = computed(() => images.value.length === 0 && pastes.value.length === 0 && files.value.length === 0)

  function readImage(file: File): Promise<{ url: string; block: ImageBlock }> {
    return new Promise((resolve, reject) => {
      const reader = new FileReader()
      reader.onload = () => {
        const url = String(reader.result)
        resolve({ url, block: { type: "image", source: { type: "base64", mediaType: file.type, data: url.slice(url.indexOf(",") + 1) } } })
      }
      reader.onerror = () => reject(reader.error ?? new Error(t("composer.imageReadFailed")))
      reader.readAsDataURL(file)
    })
  }

  async function readPdf(file: File): Promise<string> {
    const head = new TextDecoder("latin1").decode(await file.slice(0, 5).arrayBuffer())
    if (head !== "%PDF-") throw new Error(t("composer.unsupportedFile", { name: file.name }))
    return new Promise((resolve, reject) => {
      const reader = new FileReader()
      reader.onload = () => {
        const url = String(reader.result)
        resolve(url.slice(url.indexOf(",") + 1))
      }
      reader.onerror = () => reject(reader.error ?? new Error(t("composer.unsupportedFile", { name: file.name })))
      reader.readAsDataURL(file)
    })
  }

  async function addFiles(selected: File[]): Promise<void> {
    const target = draft.value
    const textAllowed = options.storesPastes()
    target.reading++
    target.error = ""
    try {
      for (const file of selected) {
        if (count(target) >= MAX_ATTACHMENTS) { target.error = t("composer.attachmentLimit"); break }
        if (file.size > MAX_PASTE) { target.error = t("composer.fileTooLarge", { name: file.name }); continue }
        try {
          if (IMAGE_TYPES.has(file.type)) {
            const image = await readImage(file)
            if (target.images.reduce((size, item) => size + item.block.source.data.length, 0) + image.block.source.data.length > MAX_PASTE) { target.error = t("composer.imagesTooLarge"); continue }
            if (count(target) < MAX_ATTACHMENTS) target.images = [...target.images, { id: ++target.nextId, ...image }]
          } else if (textAllowed && (file.type === "application/pdf" || /\.pdf$/i.test(file.name))) {
            const pdfBase64 = await readPdf(file)
            if (count(target) < MAX_ATTACHMENTS) target.files = [...target.files, { id: ++target.nextId, name: file.name, text: "", pdfBase64 }]
          } else {
            if (!textAllowed || /\.(pdf|docx?|xlsx?|pptx?|zip|gz|7z|rar|exe|dmg|mp[34]|mov|wav|bin)$/i.test(file.name) || (file.type.startsWith("image/") && file.type !== "image/svg+xml")) throw new Error(t("composer.unsupportedFile", { name: file.name }))
            const bytes = await file.arrayBuffer()
            const value = new TextDecoder("utf-8", { fatal: true }).decode(bytes)
            if (value.includes("\0")) throw new Error(t("composer.unsupportedFile", { name: file.name }))
            if (new TextEncoder().encode(JSON.stringify(value)).length > 12 * 1024 * 1024) { target.error = t("composer.fileTooLarge", { name: file.name }); continue }
            if (count(target) < MAX_ATTACHMENTS) target.files = [...target.files, { id: ++target.nextId, name: file.name, text: value }]
          }
        } catch (cause) { target.error = cause instanceof TypeError ? t("composer.unsupportedFile", { name: file.name }) : cause instanceof Error ? cause.message : t("composer.imageReadFailed") }
      }
    } finally { target.reading-- }
  }

  function addPaste(value: string): void {
    const target = draft.value
    if (new TextEncoder().encode(value).length > MAX_PASTE) {
      target.error = t("composer.pasteTooLarge")
      return
    }
    if (new TextEncoder().encode(JSON.stringify(value)).length > 12 * 1024 * 1024) { target.error = t("composer.messageTooLarge"); return }
    if (count(target) >= MAX_ATTACHMENTS) { target.error = t("composer.attachmentLimit"); return }
    target.pastes = [...target.pastes, { id: ++target.nextId, text: value }]
  }

  /** Images on the clipboard become attachments; a long text paste is kept aside where the Session stores it. */
  function onPaste(event: ClipboardEvent): void {
    if (!options.enabled()) return
    const pictures = [...(event.clipboardData?.files ?? [])].filter((file) => file.type.startsWith("image/"))
    if (pictures.length > 0) {
      event.preventDefault()
      void addFiles(pictures)
      return
    }
    const value = event.clipboardData?.getData("text/plain") ?? ""
    if (options.storesPastes() && new TextEncoder().encode(value).length > INLINE_PASTE) {
      event.preventDefault()
      addPaste(value)
    }
  }

  function onDrop(event: DragEvent): void {
    event.preventDefault()
    if (!options.enabled()) return
    void addFiles([...(event.dataTransfer?.files ?? [])])
  }

  function choose(event: Event): void {
    const input = event.target as HTMLInputElement
    void addFiles([...(input.files ?? [])])
    input.value = ""
  }

  return {
    images, pastes, files, empty, onPaste, onDrop, choose,
    removeImage: (id: number) => { images.value = images.value.filter((image) => image.id !== id) },
    removePaste: (id: number) => { pastes.value = pastes.value.filter((paste) => paste.id !== id) },
    removeFile: (id: number) => { files.value = files.value.filter((file) => file.id !== id) },
  }
}
