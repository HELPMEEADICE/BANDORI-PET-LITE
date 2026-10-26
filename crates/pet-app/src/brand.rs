//! Embedded application artwork shared by native windows and the tray.

pub(crate) fn icon_rgba(size: u32) -> Result<image::RgbaImage, image::ImageError> {
    Ok(
        image::load_from_memory(include_bytes!("../../../assets/icons/bandori.png"))?
            .resize_exact(size, size, image::imageops::FilterType::Lanczos3)
            .into_rgba8(),
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn brand_icon_has_visible_pixels_and_a_transparent_background() {
        for size in [16, 32, 48] {
            let image = super::icon_rgba(size).unwrap();
            assert_eq!(image.dimensions(), (size, size));
            assert!(image.pixels().any(|pixel| pixel.0[3] > 200));
            assert!(image.pixels().any(|pixel| pixel.0[3] == 0));
        }
    }
}
