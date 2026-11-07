//! Remote images (gallery, staff avatars, server banners).
//!
//! Bytes are cached on disk; decoding and downscaling run on a background thread, so the UI only
//! ever holds images at roughly display size. This is where most of an image-heavy app's memory
//! goes, and the reason a 4000 px upload costs ~1 MB here instead of ~64 MB.

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    path::PathBuf,
    rc::Rc,
};

use slint::{Image, Rgba8Pixel, SharedPixelBuffer};
use tokio::runtime::Handle;

type Waiter = Box<dyn FnOnce()>;

pub struct ImageLoader {
    http: reqwest::Client,
    rt: Handle,
    dir: PathBuf,
    ready: RefCell<HashMap<String, Image>>,
    pending: RefCell<HashMap<String, Vec<Waiter>>>,
    failed: RefCell<HashSet<String>>,
}

impl ImageLoader {
    pub fn new(http: reqwest::Client, rt: Handle, dir: PathBuf) -> Rc<Self> {
        let _ = std::fs::create_dir_all(&dir);
        Rc::new(Self {
            http,
            rt,
            dir,
            ready: RefCell::default(),
            pending: RefCell::default(),
            failed: RefCell::default(),
        })
    }

    /// The image if it is already decoded. Otherwise starts loading it (once) and calls
    /// `on_ready` on the UI thread when it arrives; failed images are not retried this session.
    pub fn get(
        self: &Rc<Self>,
        url: &str,
        max_px: u32,
        on_ready: impl FnOnce() + 'static,
    ) -> Option<Image> {
        self.get_shaped(url, max_px, false, on_ready)
    }

    /// Like [Self::get], cropped to a centred circle (avatars). The renderer cannot clip images
    /// to rounded shapes, so the mask is applied to the pixels once, at decode time.
    pub fn get_circle(
        self: &Rc<Self>,
        url: &str,
        max_px: u32,
        on_ready: impl FnOnce() + 'static,
    ) -> Option<Image> {
        self.get_shaped(url, max_px, true, on_ready)
    }

    fn get_shaped(
        self: &Rc<Self>,
        url: &str,
        max_px: u32,
        circle: bool,
        on_ready: impl FnOnce() + 'static,
    ) -> Option<Image> {
        if url.is_empty() {
            return None;
        }
        let key = format!("{max_px}:{}:{url}", if circle { "o" } else { "r" });
        if let Some(image) = self.ready.borrow().get(&key) {
            return Some(image.clone());
        }
        if self.failed.borrow().contains(&key) {
            return None;
        }
        let mut pending = self.pending.borrow_mut();
        if let Some(waiters) = pending.get_mut(&key) {
            waiters.push(Box::new(on_ready));
            return None;
        }
        pending.insert(key.clone(), vec![Box::new(on_ready)]);
        drop(pending);

        let task = self.rt.spawn(fetch_and_decode(
            self.http.clone(),
            url.to_owned(),
            self.dir.join(format!("{:016x}.img", fnv1a(url))),
            max_px,
            circle,
        ));
        let this = self.clone();
        let spawned = slint::spawn_local(async move {
            let buffer = task.await.ok().flatten();
            match buffer {
                Some(buffer) => {
                    this.ready
                        .borrow_mut()
                        .insert(key.clone(), Image::from_rgba8(buffer));
                }
                None => {
                    this.failed.borrow_mut().insert(key.clone());
                }
            }
            let waiters = this.pending.borrow_mut().remove(&key).unwrap_or_default();
            for waiter in waiters {
                waiter();
            }
        });
        if spawned.is_err() {
            self.pending.borrow_mut().clear();
        }
        None
    }
}

async fn fetch_and_decode(
    http: reqwest::Client,
    url: String,
    path: PathBuf,
    max_px: u32,
    circle: bool,
) -> Option<SharedPixelBuffer<Rgba8Pixel>> {
    let bytes = match tokio::fs::read(&path).await {
        Ok(bytes) => bytes,
        Err(_) => {
            let response = http.get(&url).send().await.ok()?.error_for_status().ok()?;
            let bytes = response.bytes().await.ok()?.to_vec();
            let tmp = path.with_extension("tmp");
            if tokio::fs::write(&tmp, &bytes).await.is_ok() {
                let _ = tokio::fs::rename(&tmp, &path).await;
            }
            bytes
        }
    };
    tokio::task::spawn_blocking(move || decode(&bytes, max_px, circle))
        .await
        .ok()
        .flatten()
}

fn decode(bytes: &[u8], max_px: u32, circle: bool) -> Option<SharedPixelBuffer<Rgba8Pixel>> {
    let mut image = image::load_from_memory(bytes).ok()?;
    if circle {
        // Centre-crop to a square first so the circle is not an ellipse.
        let side = image.width().min(image.height());
        image = image.crop_imm(
            (image.width() - side) / 2,
            (image.height() - side) / 2,
            side,
            side,
        );
    }
    if image.width() > max_px || image.height() > max_px {
        image = image.thumbnail(max_px, max_px);
    }
    let mut rgba = image.into_rgba8();
    if circle {
        mask_circle(&mut rgba);
    }
    Some(SharedPixelBuffer::clone_from_slice(
        rgba.as_raw(),
        rgba.width(),
        rgba.height(),
    ))
}

/// Makes everything outside the inscribed circle transparent, with a one-pixel soft edge.
fn mask_circle(image: &mut image::RgbaImage) {
    let r = image.width().min(image.height()) as f32 / 2.0;
    let (cx, cy) = (image.width() as f32 / 2.0, image.height() as f32 / 2.0);
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
        let coverage = (r - d + 0.5).clamp(0.0, 1.0);
        pixel[3] = (pixel[3] as f32 * coverage) as u8;
    }
}

/// Stable file names for the disk cache (std's hasher may change between Rust releases).
fn fnv1a(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, b| {
        (hash ^ b as u64).wrapping_mul(0x0100_0000_01b3)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_downscales() {
        let big = image::RgbaImage::from_pixel(2000, 1000, image::Rgba([255, 0, 0, 255]));
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(big)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let buffer = decode(&png, 512, false).unwrap();
        assert_eq!((buffer.width(), buffer.height()), (512, 256));
        let round = decode(&png, 100, true).unwrap();
        assert_eq!((round.width(), round.height()), (100, 100));
        let px = round.as_slice();
        assert_eq!(px[0].a, 0, "corner is transparent");
        assert_eq!(px[50 * 100 + 50].a, 255, "centre is opaque");
        assert!(decode(b"not an image", 512, false).is_none());
    }
}
