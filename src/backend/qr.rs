//! Decoding a WireGuard config from a QR-code image. The bounded image decode
//! and grid selection live in [`crate::import_parse`].

use std::path::Path;

#[cfg(test)]
use crate::import_parse::decode_qr_bytes;
#[cfg(test)]
use crate::import_parse::select_qr_config;
use crate::safe_file::{self, Rules};

/// Decode a QR-code image file into its text (a WireGuard `.conf`). The file
/// is opened through the shared safe-open routine: a FIFO, socket or device
/// is refused immediately instead of blocking the import. Decoding runs in a
/// child process ([`crate::qr_isolate`]) so a decoder crash cannot abort the app.
pub fn decode_qr(path: &Path) -> Result<String, String> {
    let bytes = safe_file::read_bytes(path, Rules::QR_IMAGE)?;
    crate::qr_isolate::decode(&bytes)
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
