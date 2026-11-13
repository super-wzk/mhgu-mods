//! PNG image entries in standard-detail Frontier monster PAC texture bundles.
use anyhow::{Result, bail};
use png::{ColorType, Decoder, Transformations};

pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub fn decode(bytes: &[u8]) -> Result<Image> {
    let mut decoder = Decoder::new(bytes);
    decoder.set_transformations(Transformations::EXPAND | Transformations::STRIP_16);
    // PNG's row bounds are determined by the image header.
    decoder.set_limits(png::Limits { bytes: usize::MAX });
    let mut reader = decoder.read_info()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let frame = reader.next_frame(&mut buf)?;
    buf.truncate(frame.buffer_size());
    let rgba = match frame.color_type {
        ColorType::Rgba => buf,
        ColorType::Rgb => buf
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        ColorType::Grayscale => buf.into_iter().flat_map(|x| [x, x, x, 255]).collect(),
        ColorType::GrayscaleAlpha => buf
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        ColorType::Indexed => bail!("PNG palette was not expanded"),
    };
    Ok(Image {
        width: frame.width,
        height: frame.height,
        rgba,
    })
}
