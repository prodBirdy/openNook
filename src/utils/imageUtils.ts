
/**
 * Extracts the dominant color from an image base64 string or URL.
 * Returns the color as an RGB array [r, g, b].
 */
export async function getDominantColor(imageSrc: string): Promise<[number, number, number] | null> {
    return new Promise((resolve) => {
        const img = new Image();
        img.crossOrigin = "Anonymous";
        img.src = imageSrc;

        img.onload = () => {
            const canvas = document.createElement('canvas');
            const ctx = canvas.getContext('2d');
            if (!ctx) {
                resolve(null);
                return;
            }

            // Downscale for performance
            canvas.width = 50;
            canvas.height = 50;

            ctx.drawImage(img, 0, 0, 50, 50);

            try {
                const imageData = ctx.getImageData(0, 0, 50, 50);
                const data = imageData.data;

                let r = 0, g = 0, b = 0, count = 0;

                // Simple average for now, but skipping very dark/white pixels for better vibrancy
                for (let i = 0; i < data.length; i += 4) {
                    const cr = data[i];
                    const cg = data[i + 1];
                    const cb = data[i + 2];
                    const ca = data[i + 3];

                    // Skip transparent
                    if (ca < 200) continue;

                    // Skip near black and near white
                    // Calculate brightness
                    const brightness = (cr + cg + cb) / 3;
                    if (brightness < 20 || brightness > 230) continue;

                    r += cr;
                    g += cg;
                    b += cb;
                    count++;
                }

                if (count === 0) {
                    // Fallback to simple average if everything was filtered out
                    for (let i = 0; i < data.length; i += 4) {
                        r += data[i];
                        g += data[i + 1];
                        b += data[i + 2];
                        count++;
                    }
                }

                if (count > 0) {
                    resolve([Math.round(r / count), Math.round(g / count), Math.round(b / count)]);
                } else {
                    resolve(null);
                }
            } catch (e) {
                console.error("Error getting image data", e);
                resolve(null);
            }
        };

        img.onerror = () => {
            resolve(null);
        };
    });
}
