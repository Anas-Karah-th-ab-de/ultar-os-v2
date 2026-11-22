//! Gesichtserkennung ohne Netz: YuNet findet Gesichter samt 5 Merkmalspunkten (Augen, Nase, Mundwinkel),
//! das Gesicht wird daran auf 112×112 ausgerichtet, SFace berechnet daraus 128 Merkmale.
//! Zwei Gesichter gehören zur selben Person, wenn der Kosinus ihrer Merkmale hoch genug ist.
//! Modelle: OpenCV-Zoo (YuNet: MIT, SFace: Apache-2.0), siehe docs/GESICHT.md.

use tract_onnx::prelude::*;

/// Bild als 0x00RRGGBB, zeilenweise.
#[derive(Clone)]
pub struct Img {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u32>,
}

impl Img {
    pub fn new(w: usize, h: usize) -> Self {
        Self {
            w,
            h,
            px: vec![0; w * h],
        }
    }
    #[inline]
    fn rgb(&self, x: usize, y: usize) -> [f32; 3] {
        let p = self.px[y * self.w + x];
        [
            ((p >> 16) & 255) as f32,
            ((p >> 8) & 255) as f32,
            (p & 255) as f32,
        ]
    }
    /// Bilinear abtasten (außerhalb: schwarz)
    fn sample(&self, x: f32, y: f32) -> [f32; 3] {
        if x < 0.0 || y < 0.0 || x > (self.w - 1) as f32 || y > (self.h - 1) as f32 {
            return [0.0; 3];
        }
        let (x0, y0) = (x.floor() as usize, y.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(self.w - 1), (y0 + 1).min(self.h - 1));
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let (a, b, c, d) = (
            self.rgb(x0, y0),
            self.rgb(x1, y0),
            self.rgb(x0, y1),
            self.rgb(x1, y1),
        );
        let mut o = [0.0; 3];
        for i in 0..3 {
            o[i] =
                (a[i] * (1.0 - fx) + b[i] * fx) * (1.0 - fy) + (c[i] * (1.0 - fx) + d[i] * fx) * fy;
        }
        o
    }
    /// Um Vierteldrehungen im Uhrzeigersinn drehen.
    pub fn rotate(&self, q: u8) -> Img {
        match q % 4 {
            0 => self.clone(),
            2 => Img {
                w: self.w,
                h: self.h,
                px: self.px.iter().rev().copied().collect(),
            },
            q => {
                let mut o = Img::new(self.h, self.w);
                for y in 0..self.h {
                    for x in 0..self.w {
                        let (nx, ny) = if q == 1 {
                            (self.h - 1 - y, x)
                        } else {
                            (y, self.w - 1 - x)
                        };
                        o.px[ny * o.w + nx] = self.px[y * self.w + x];
                    }
                }
                o
            }
        }
    }
    /// Mittlere Helligkeit 0..255
    pub fn brightness(&self) -> f32 {
        let step = (self.px.len() / 4096).max(1);
        let (mut s, mut n) = (0f32, 0f32);
        for p in self.px.iter().step_by(step) {
            s += (((p >> 16) & 255) + ((p >> 8) & 255) + (p & 255)) as f32 / 3.0;
            n += 1.0;
        }
        s / n.max(1.0)
    }
}

#[derive(Debug, Clone)]
pub struct Face {
    pub score: f32,
    /// x, y, Breite, Höhe im Originalbild
    pub bbox: [f32; 4],
    /// rechtes Auge, linkes Auge, Nase, rechter / linker Mundwinkel (x, y)
    pub kps: [[f32; 2]; 5],
}

type Model = std::sync::Arc<TypedRunnableModel>;

pub struct Engine {
    det: Model,
    rec: Model,
    pub det_size: usize,
}

/// Standardpunkte für 112×112 (ArcFace-Vorlage, wie OpenCV FaceRecognizerSF)
const TEMPLATE: [[f32; 2]; 5] = [
    [38.2946, 51.6963],
    [73.5318, 51.5014],
    [56.0252, 71.7366],
    [41.5493, 92.3655],
    [70.7299, 92.2041],
];

impl Engine {
    /// Modelle laden. `det_size`: Kantenlänge der Erkennungs-Eingabe (Vielfaches von 32, z. B. 320).
    pub fn load(det_path: &str, rec_path: &str, det_size: usize) -> TractResult<Engine> {
        let det = tract_onnx::onnx()
            .model_for_path(det_path)?
            .with_input_fact(0, f32::fact([1, 3, det_size, det_size]).into())?
            .into_optimized()?
            .into_runnable()
            .map_err(|e| e.context("YuNet"))?;
        let rec = tract_onnx::onnx()
            .model_for_path(rec_path)?
            .with_input_fact(0, f32::fact([1, 3, 112, 112]).into())?
            .into_optimized()?
            .into_runnable()?;
        Ok(Engine { det, rec, det_size })
    }

    /// Gesichter finden (beste zuerst).
    pub fn detect(&self, img: &Img, min_score: f32) -> TractResult<Vec<Face>> {
        let s = self.det_size;
        let scale = (s as f32 / img.w as f32).min(s as f32 / img.h as f32);
        let (nw, nh) = (
            ((img.w as f32 * scale) as usize).min(s),
            ((img.h as f32 * scale) as usize).min(s),
        );
        // BGR, 0..255, oben links eingepasst
        let mut t = tract_ndarray::Array4::<f32>::zeros((1, 3, s, s));
        for y in 0..nh {
            for x in 0..nw {
                let c = img.sample(
                    (x as f32 + 0.5) / scale - 0.5,
                    (y as f32 + 0.5) / scale - 0.5,
                );
                t[[0, 0, y, x]] = c[2];
                t[[0, 1, y, x]] = c[1];
                t[[0, 2, y, x]] = c[0];
            }
        }
        let out = self.det.run(tvec!(t.into_tensor().into()))?;
        // Ausgaben: cls_8,16,32, obj_8,16,32, bbox_8,16,32, kps_8,16,32
        let mut faces = Vec::new();
        for (k, stride) in [8usize, 16, 32].iter().enumerate() {
            let cls = &sl(&out[k])?;
            let obj = &sl(&out[3 + k])?;
            let bb = &sl(&out[6 + k])?;
            let kp = &sl(&out[9 + k])?;
            let cols = s / stride;
            for i in 0..cls.len() {
                let sc = (cls[i].clamp(0.0, 1.0) * obj[i].clamp(0.0, 1.0)).sqrt();
                if sc < min_score {
                    continue;
                }
                let (r, c) = ((i / cols) as f32, (i % cols) as f32);
                let st = *stride as f32;
                let cx = (c + bb[i * 4]) * st;
                let cy = (r + bb[i * 4 + 1]) * st;
                let w = bb[i * 4 + 2].exp() * st;
                let h = bb[i * 4 + 3].exp() * st;
                let mut kps = [[0f32; 2]; 5];
                for (n, p) in kps.iter_mut().enumerate() {
                    *p = [
                        (kp[i * 10 + 2 * n] + c) * st / scale,
                        (kp[i * 10 + 2 * n + 1] + r) * st / scale,
                    ];
                }
                faces.push(Face {
                    score: sc,
                    bbox: [
                        (cx - w / 2.0) / scale,
                        (cy - h / 2.0) / scale,
                        w / scale,
                        h / scale,
                    ],
                    kps,
                });
            }
        }
        faces.sort_by(|a, b| b.score.total_cmp(&a.score));
        // Überlappende entfernen (NMS, IoU 0.3)
        let mut keep: Vec<Face> = Vec::new();
        for f in faces {
            if keep.iter().all(|k| iou(&k.bbox, &f.bbox) < 0.3) {
                keep.push(f);
            }
        }
        Ok(keep)
    }

    /// Gesicht ausrichten (112×112) und 128 Merkmale berechnen (normiert).
    pub fn embed(&self, img: &Img, f: &Face) -> TractResult<Vec<f32>> {
        let a = align(img, &f.kps);
        let mut t = tract_ndarray::Array4::<f32>::zeros((1, 3, 112, 112));
        for y in 0..112 {
            for x in 0..112 {
                let p = a.px[y * 112 + x];
                t[[0, 0, y, x]] = ((p >> 16) & 255) as f32;
                t[[0, 1, y, x]] = ((p >> 8) & 255) as f32;
                t[[0, 2, y, x]] = (p & 255) as f32;
            }
        }
        let out = self.rec.run(tvec!(t.into_tensor().into()))?;
        let v: Vec<f32> = sl(&out[0])?;
        Ok(normalize(v))
    }
}

fn sl(t: &TValue) -> TractResult<Vec<f32>> {
    Ok(t.try_as_plain_ram()?.as_slice::<f32>()?.to_vec())
}

pub fn normalize(mut v: Vec<f32>) -> Vec<f32> {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-9);
    v.iter_mut().for_each(|x| *x /= n);
    v
}

/// Kosinus-Ähnlichkeit (bei normierten Vektoren = Skalarprodukt)
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn iou(a: &[f32; 4], b: &[f32; 4]) -> f32 {
    let x1 = a[0].max(b[0]);
    let y1 = a[1].max(b[1]);
    let x2 = (a[0] + a[2]).min(b[0] + b[2]);
    let y2 = (a[1] + a[3]).min(b[1] + b[3]);
    let i = (x2 - x1).max(0.0) * (y2 - y1).max(0.0);
    i / (a[2] * a[3] + b[2] * b[3] - i).max(1e-6)
}

/// Ähnlichkeitstransformation (Drehung+Skalierung+Verschiebung) nach Umeyama: src -> dst.
/// Liefert [a, -b, tx; b, a, ty] als (a, b, tx, ty).
pub fn similarity(src: &[[f32; 2]; 5], dst: &[[f32; 2]; 5]) -> (f32, f32, f32, f32) {
    let n = 5.0;
    let (mut sx, mut sy, mut dx, mut dy) = (0.0, 0.0, 0.0, 0.0);
    for i in 0..5 {
        sx += src[i][0];
        sy += src[i][1];
        dx += dst[i][0];
        dy += dst[i][1];
    }
    let (sx, sy, dx, dy) = (sx / n, sy / n, dx / n, dy / n);
    let (mut num_a, mut num_b, mut den) = (0.0, 0.0, 0.0);
    for i in 0..5 {
        let (px, py) = (src[i][0] - sx, src[i][1] - sy);
        let (qx, qy) = (dst[i][0] - dx, dst[i][1] - dy);
        num_a += px * qx + py * qy;
        num_b += px * qy - py * qx;
        den += px * px + py * py;
    }
    let a = num_a / den.max(1e-9);
    let b = num_b / den.max(1e-9);
    let tx = dx - (a * sx - b * sy);
    let ty = dy - (b * sx + a * sy);
    (a, b, tx, ty)
}

/// Gesicht an den 5 Punkten auf die 112×112-Vorlage ausrichten.
pub fn align(img: &Img, kps: &[[f32; 2]; 5]) -> Img {
    let (a, b, tx, ty) = similarity(kps, &TEMPLATE);
    // Umkehrung: Ziel (u,v) -> Quelle
    let d = a * a + b * b;
    let (ia, ib) = (a / d, -b / d);
    let mut o = Img::new(112, 112);
    for v in 0..112 {
        for u in 0..112 {
            let (uu, vv) = (u as f32 - tx, v as f32 - ty);
            let x = ia * uu - ib * vv;
            let y = ib * uu + ia * vv;
            let c = img.sample(x, y);
            o.px[v * 112 + u] =
                ((c[0].round() as u32) << 16) | ((c[1].round() as u32) << 8) | c[2].round() as u32;
        }
    }
    o
}

/// Gespeicherte Merkmale: "UFC1", Anzahl (u32), dann Anzahl × 128 f32 (LE)
pub fn encode(list: &[Vec<f32>]) -> Vec<u8> {
    let mut o = b"UFC1".to_vec();
    o.extend_from_slice(&(list.len() as u32).to_le_bytes());
    for v in list {
        for x in v {
            o.extend_from_slice(&x.to_le_bytes());
        }
    }
    o
}

pub fn decode(b: &[u8]) -> Option<Vec<Vec<f32>>> {
    if b.len() < 8 || &b[..4] != b"UFC1" {
        return None;
    }
    let n = u32::from_le_bytes(b[4..8].try_into().ok()?) as usize;
    if b.len() != 8 + n * 128 * 4 {
        return None;
    }
    Some(
        (0..n)
            .map(|i| {
                (0..128)
                    .map(|j| {
                        f32::from_le_bytes(b[8 + (i * 128 + j) * 4..][..4].try_into().unwrap())
                    })
                    .collect()
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn umeyama_identitaet_und_skalierung() {
        let (a, b, tx, ty) = similarity(&TEMPLATE, &TEMPLATE);
        assert!((a - 1.0).abs() < 1e-4 && b.abs() < 1e-4 && tx.abs() < 1e-3 && ty.abs() < 1e-3);
        let src: [[f32; 2]; 5] = TEMPLATE.map(|p| [p[0] * 2.0 + 10.0, p[1] * 2.0 + 5.0]);
        let (a, b, tx, ty) = similarity(&src, &TEMPLATE);
        assert!((a - 0.5).abs() < 1e-4 && b.abs() < 1e-4);
        assert!(
            (a * src[0][0] + tx - TEMPLATE[0][0]).abs() < 1e-3
                && (a * src[0][1] + ty - TEMPLATE[0][1]).abs() < 1e-3
        );
    }

    #[test]
    fn drehen() {
        let mut i = Img::new(3, 2);
        i.px = vec![1, 2, 3, 4, 5, 6];
        let r = i.rotate(1);
        assert_eq!((r.w, r.h), (2, 3));
        assert_eq!(r.px, vec![4, 1, 5, 2, 6, 3]);
        assert_eq!(i.rotate(1).rotate(3).px, i.px);
        assert_eq!(i.rotate(2).px, vec![6, 5, 4, 3, 2, 1]);
    }

    #[test]
    fn speichern() {
        let v = vec![
            normalize((0..128).map(|x| x as f32).collect()),
            normalize(vec![1.0; 128]),
        ];
        let d = decode(&encode(&v)).unwrap();
        assert_eq!(d, v);
        assert!((cosine(&d[1], &d[1]) - 1.0).abs() < 1e-5);
        assert!(decode(b"UFC1\x01\0\0\0").is_none());
    }
}
