//! Decoding a WireGuard config from a QR-code image.

use std::path::Path;

use super::config_runs_scripts;
use crate::config;
use crate::safe_file::{self, Rules};

/// Decode a QR-code image file into its text (a WireGuard `.conf`). The file
/// is opened through the shared safe-open routine: a FIFO, socket or device
/// is refused immediately instead of blocking the import.
pub fn decode_qr(path: &Path) -> Result<String, String> {
    let bytes = safe_file::read_bytes(path, Rules::QR_IMAGE)?;
    decode_qr_bytes(&bytes)
}

pub fn decode_qr_bytes(bytes: &[u8]) -> Result<String, String> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    // Bounds cover a full-size screenshot while limiting both decoder buffers
    // and rqrr's additional per-pixel working memory.
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let img = reader
        .decode()
        .map_err(|e| format!("Couldn't decode bounded QR image: {e}"))?
        .to_luma8();
    let mut prepared = rqrr::PreparedImage::prepare(img);
    let grids = prepared.detect_grids();
    if grids.is_empty() {
        return Err("No QR code found in the image.".into());
    }
    select_qr_config(grids.iter().map(|grid| {
        grid.decode()
            .map(|(_, content)| content)
            .map_err(|e| e.to_string())
    }))
}

fn select_qr_config(
    results: impl IntoIterator<Item = Result<String, String>>,
) -> Result<String, String> {
    let mut configs = Vec::new();
    for result in results {
        let Ok(content) = result else {
            continue;
        };
        if config::validate_basic_wireguard_config(&content).is_ok()
            && !config_runs_scripts(&content)
            && !configs.contains(&content)
        {
            configs.push(content);
            if configs.len() > 1 {
                return Err("Image contains multiple different WireGuard configs; use an image with one tunnel.".into());
            }
        }
    }
    configs
        .pop()
        .ok_or("No valid WireGuard config QR code found in the image.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::scratch_dir;

    const KEY: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopo=";

    #[test]
    fn qr_selection_skips_bad_grids_and_refuses_ambiguous_tunnels() {
        let valid = format!("[Interface]\nPrivateKey = {KEY}\n");
        assert_eq!(
            select_qr_config([
                Err("decode failed".into()),
                Ok("unrelated QR".into()),
                Ok(valid.clone())
            ])
            .unwrap(),
            valid
        );
        assert!(
            select_qr_config([
                Ok(valid.clone()),
                Ok(format!("{valid}Address = 10.0.0.1/24\n"))
            ])
            .is_err()
        );
        assert_eq!(
            select_qr_config([Ok(valid.clone()), Ok(valid.clone())]).unwrap(),
            valid
        );
    }

    #[test]
    fn qr_decoder_rejects_oversized_dimensions() {
        let dir = scratch_dir("qr");
        let path = dir.join("wide.png");
        image::GrayImage::new(4097, 1).save(&path).unwrap();
        assert!(decode_qr(&path).unwrap_err().contains("bounded QR image"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn qr_import_never_hangs_on_special_files() {
        use std::os::unix::ffi::OsStrExt;
        let dir = scratch_dir("qr-special");
        let fifo = dir.join("code.png");
        let c = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        let started = std::time::Instant::now();
        assert!(decode_qr(&fifo).unwrap_err().contains("regular file"));
        assert!(
            decode_qr(Path::new("/dev/zero"))
                .unwrap_err()
                .contains("regular file")
        );
        assert!(decode_qr(&dir).unwrap_err().contains("regular file"));
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn qr_round_trip_decodes_an_encoded_config() {
        let valid = format!("[Interface]\nPrivateKey = {KEY}\n");
        let code = qrcode::QrCode::new(valid.as_bytes()).unwrap();
        let width = code.width() as u32;
        let colors = code.to_colors();
        let scale = 8;
        let border = 4;
        let size = (width + 2 * border) * scale;
        let img = image::GrayImage::from_fn(size, size, |x, y| {
            let (mx, my) = (
                (x / scale) as i64 - border as i64,
                (y / scale) as i64 - border as i64,
            );
            let dark = mx >= 0
                && my >= 0
                && (mx as u32) < width
                && (my as u32) < width
                && colors[(my as u32 * width + mx as u32) as usize] == qrcode::Color::Dark;
            image::Luma([if dark { 0 } else { 255 }])
        });
        let mut png = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        assert_eq!(decode_qr_bytes(&png).unwrap(), valid);
    }
}
