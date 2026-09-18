use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder};
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

/// 将 RGBA 像素缓冲区编码为 PNG 写入磁盘。
///
/// `rgba` 长度必须 >= width * height * 4，多余部分被忽略。
pub fn write_png_rgba(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
    let need = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or_else(|| "图像尺寸过大".to_string())?;
    if rgba.len() < need {
        return Err(format!(
            "像素数据不足: 需要 {} 字节，实际 {} 字节",
            need,
            rgba.len()
        ));
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let file = File::create(path).map_err(|e| format!("创建图标文件失败: {}", e))?;
    let writer = BufWriter::new(file);
    let encoder = PngEncoder::new(writer);
    encoder
        .write_image(
            &rgba[..need],
            width,
            height,
            ExtendedColorType::Rgba8,
        )
        .map_err(|e| format!("PNG 编码失败: {}", e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_and_reloads_a_valid_png() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("icon.png");
        let w = 4u32;
        let h = 2u32;
        let mut px = vec![0u8; (w * h * 4) as usize];
        // 第一个像素设为不透明白色
        px[0] = 255;
        px[1] = 255;
        px[2] = 255;
        px[3] = 255;

        write_png_rgba(&path, w, h, &px).unwrap();
        assert!(path.exists(), "PNG 文件应被写入，且自动创建父目录");

        let decoded = image::ImageReader::open(&path)
            .expect("打开 PNG")
            .decode()
            .expect("解码 PNG");
        assert_eq!(decoded.width(), w);
        assert_eq!(decoded.height(), h);
        let rgba = decoded.to_rgba8();
        assert_eq!(rgba.get_pixel(0, 0).0, [255, 255, 255, 255]);
    }

    #[test]
    fn rejects_short_buffer() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.png");
        assert!(write_png_rgba(&path, 8, 8, &[0u8; 8]).is_err());
    }
}
