//! Icône de l'application (assets/icon.ico, reprise de MCHOSE HUB).
//!
//! Le fichier contient des entrées BMP 32 bits (16/24/32/48) et une entrée PNG 256 :
//! on décode les BMP à la main pour éviter une dépendance PNG.

const ICO: &[u8] = include_bytes!("../assets/icon.ico");

/// Image BGRA (alpha non prémultiplié, lignes de haut en bas), format natif des icônes Win32.
pub struct Image {
    pub size: u32,
    pub bgra: Vec<u8>,
}

impl Image {
    pub fn to_rgba(&self) -> Vec<u8> {
        self.bgra.chunks_exact(4).flat_map(|p| [p[2], p[1], p[0], p[3]]).collect()
    }
}

/// Plus petite entrée BMP de taille >= `size` (ou la plus grande), mise à l'échelle si besoin.
pub fn image(size: u32) -> Option<Image> {
    let u16_at = |i: usize| u16::from_le_bytes([ICO[i], ICO[i + 1]]) as usize;
    let u32_at = |i: usize| u32::from_le_bytes([ICO[i], ICO[i + 1], ICO[i + 2], ICO[i + 3]]) as usize;
    let entries: Vec<(u32, usize)> = (0..u16_at(4))
        .filter_map(|n| {
            let e = 6 + n * 16;
            let side = if ICO[e] == 0 { 256 } else { ICO[e] as u32 };
            let off = u32_at(e + 12);
            // Les entrées PNG commencent par la signature 0x89 'P' : on ne garde que les BMP 32 bits.
            (ICO.get(off..off + 4)? == [40, 0, 0, 0] && u16_at(off + 14) == 32).then_some((side, off))
        })
        .collect();
    let &(side, off) =
        entries.iter().filter(|e| e.0 >= size).min_by_key(|e| e.0).or_else(|| entries.iter().max_by_key(|e| e.0))?;
    // BITMAPINFOHEADER (40 octets) puis pixels BGRA de bas en haut ; le masque AND est ignoré.
    let pixels = ICO.get(off + 40..off + 40 + (side * side * 4) as usize)?;
    let row = side as usize * 4;
    let src: Vec<u8> = pixels.chunks_exact(row).rev().flatten().copied().collect();
    if side == size {
        return Some(Image { size, bgra: src });
    }
    // Réduction par moyenne de zone (cas d'une taille absente du fichier, ex. 20 px à 125 %).
    let mut bgra = vec![0u8; (size * size * 4) as usize];
    let ratio = side as f32 / size as f32;
    for y in 0..size {
        for x in 0..size {
            let (x0, y0) = ((x as f32 * ratio) as u32, (y as f32 * ratio) as u32);
            let (x1, y1) =
                ((((x + 1) as f32 * ratio) as u32).max(x0 + 1), (((y + 1) as f32 * ratio) as u32).max(y0 + 1));
            let mut acc = [0u32; 4];
            for sy in y0..y1.min(side) {
                for sx in x0..x1.min(side) {
                    let p = &src[(sy * side + sx) as usize * 4..][..4];
                    let a = p[3] as u32;
                    // Moyenne pondérée par l'alpha pour ne pas assombrir les bords.
                    for c in 0..3 {
                        acc[c] += p[c] as u32 * a;
                    }
                    acc[3] += a;
                }
            }
            let n = (x1.min(side) - x0) * (y1.min(side) - y0);
            let d = &mut bgra[(y * size + x) as usize * 4..][..4];
            if acc[3] > 0 {
                for c in 0..3 {
                    d[c] = (acc[c] / acc[3]) as u8;
                }
            }
            d[3] = (acc[3] / n.max(1)) as u8;
        }
    }
    Some(Image { size, bgra })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_embedded_sizes() {
        for size in [16, 20, 24, 32, 48, 64] {
            let img = image(size).unwrap();
            assert_eq!((img.size, img.bgra.len()), (size, (size * size * 4) as usize));
            // Coin (arrondi) au moins partiellement transparent, bord gauche opaque.
            assert!(img.bgra[3] < 200);
            let c = ((size / 2) * size + 2) as usize * 4;
            assert_eq!(img.bgra[c + 3], 255);
        }
    }
}
