/** The limits `read` holds an image to, in `crates/dim-agent-capabilities/src/images.rs`: every later request of the Session carries the image again. */
const MAX_EDGE = 2048
const MAX_BASE64 = 500_000
const JPEG_QUALITY = 0.85

function fits(blob: Blob): boolean {
  return Math.ceil(blob.size / 3) * 4 <= MAX_BASE64
}

function encode(canvas: HTMLCanvasElement, type: string): Promise<Blob> {
  return new Promise((resolve, reject) => canvas.toBlob((blob) => blob === null ? reject(new Error(type)) : resolve(blob), type, JPEG_QUALITY))
}

/**
 * The image within those limits: the file itself where it fits, otherwise its pixels scaled to
 * the edge limit — PNG for a PNG whose scaled PNG fits, JPEG on white otherwise, the long edge
 * stepping down until the JPEG fits. An animation that does not fit is sent as one frame.
 */
export async function fitImage(file: Blob): Promise<Blob> {
  const bitmap = await createImageBitmap(file)
  try {
    const longest = Math.max(bitmap.width, bitmap.height)
    if (longest <= MAX_EDGE && fits(file)) return file
    const canvas = document.createElement("canvas")
    for (let edge = Math.min(longest, MAX_EDGE), first = true; ; edge = Math.floor(edge * 7 / 8), first = false) {
      canvas.width = Math.max(1, Math.round(bitmap.width * edge / longest))
      canvas.height = Math.max(1, Math.round(bitmap.height * edge / longest))
      const context = canvas.getContext("2d")
      if (context === null) return file
      context.imageSmoothingQuality = "high"
      context.drawImage(bitmap, 0, 0, canvas.width, canvas.height)
      if (first && file.type === "image/png") {
        const png = await encode(canvas, "image/png")
        if (fits(png)) return png
      }
      context.globalCompositeOperation = "destination-over"
      context.fillStyle = "#fff"
      context.fillRect(0, 0, canvas.width, canvas.height)
      const jpeg = await encode(canvas, "image/jpeg")
      if (fits(jpeg) || edge <= 1) return jpeg
    }
  } finally { bitmap.close() }
}
