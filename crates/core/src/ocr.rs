//! Local text recognition (Phase 6, slice 49) with the OCR built into Windows
//! (`Windows.Media.Ocr`): nothing is downloaded and nothing leaves the machine. Scanned PDF pages
//! are rendered with `Windows.Data.Pdf` first. Other systems get [`available`] = false.

#[cfg(not(windows))]
use crate::Error;
use crate::Result;

/// The text of one image or PDF page, lines joined by `\n`.
pub struct Page {
    pub text: String,
}

#[cfg(windows)]
mod imp {
    use super::Page;
    use crate::{Error, Result};
    use windows::core::HSTRING;
    use windows::Data::Pdf::{PdfDocument, PdfPageRenderOptions};
    use windows::Globalization::Language;
    use windows::Graphics::Imaging::{
        BitmapAlphaMode, BitmapDecoder, BitmapInterpolationMode, BitmapPixelFormat, BitmapTransform, ColorManagementMode,
        ExifOrientationMode, SoftwareBitmap,
    };
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::Streams::{DataWriter, IRandomAccessStream, InMemoryRandomAccessStream};

    fn err(e: windows::core::Error) -> Error {
        Error::Invalid(format!("Texterkennung: {}", e.message()))
    }

    /// German first, then the languages of the Windows user profile.
    pub fn engine() -> Result<OcrEngine> {
        for tag in ["de-DE", "de"] {
            if let Ok(lang) = Language::CreateLanguage(&HSTRING::from(tag)) {
                if OcrEngine::IsLanguageSupported(&lang).unwrap_or(false) {
                    if let Ok(e) = OcrEngine::TryCreateFromLanguage(&lang) {
                        return Ok(e);
                    }
                }
            }
        }
        OcrEngine::TryCreateFromUserProfileLanguages().map_err(|_| {
            Error::Invalid(
                "Die Texterkennung von Windows ist für Deutsch nicht installiert (Einstellungen → Zeit und Sprache → Sprache → Deutsch → Sprachoptionen). Bitte den Text abtippen oder diktieren.".into(),
            )
        })
    }

    pub fn available() -> bool {
        engine().is_ok()
    }

    fn stream_of(bytes: &[u8]) -> Result<InMemoryRandomAccessStream> {
        let stream = InMemoryRandomAccessStream::new().map_err(err)?;
        let writer = DataWriter::CreateDataWriter(&stream).map_err(err)?;
        writer.WriteBytes(bytes).map_err(err)?;
        writer.StoreAsync().map_err(err)?.join().map_err(err)?;
        writer.FlushAsync().map_err(err)?.join().map_err(err)?;
        writer.DetachStream().map_err(err)?;
        stream.Seek(0).map_err(err)?;
        Ok(stream)
    }

    /// The image upright (EXIF orientation of phone photos) and no larger than the engine takes.
    fn bitmap(stream: &IRandomAccessStream) -> Result<SoftwareBitmap> {
        let decoder = BitmapDecoder::CreateAsync(stream).map_err(err)?.join().map_err(|_| Error::Invalid("Das Bild kann nicht gelesen werden".into()))?;
        let (w, h) = (decoder.OrientedPixelWidth().map_err(err)?, decoder.OrientedPixelHeight().map_err(err)?);
        let max = OcrEngine::MaxImageDimension().map_err(err)?.max(1);
        let transform = BitmapTransform::new().map_err(err)?;
        let longest = w.max(h).max(1);
        if longest > max {
            let scale = f64::from(max) / f64::from(longest);
            // The transform scales the stored (unrotated) pixels.
            let (pw, ph) = (decoder.PixelWidth().map_err(err)?, decoder.PixelHeight().map_err(err)?);
            transform.SetScaledWidth(((f64::from(pw) * scale) as u32).max(1)).map_err(err)?;
            transform.SetScaledHeight(((f64::from(ph) * scale) as u32).max(1)).map_err(err)?;
            transform.SetInterpolationMode(BitmapInterpolationMode::Fant).map_err(err)?;
        }
        decoder
            .GetSoftwareBitmapTransformedAsync(
                BitmapPixelFormat::Bgra8,
                BitmapAlphaMode::Premultiplied,
                &transform,
                ExifOrientationMode::RespectExifOrientation,
                ColorManagementMode::DoNotColorManage,
            )
            .map_err(err)?
            .join()
            .map_err(err)
    }

    fn recognize(engine: &OcrEngine, bitmap: &SoftwareBitmap) -> Result<Page> {
        let result = engine.RecognizeAsync(bitmap).map_err(err)?.join().map_err(err)?;
        let mut lines = Vec::new();
        for line in result.Lines().map_err(err)? {
            lines.push(line.Text().map_err(err)?.to_string_lossy());
        }
        Ok(Page { text: lines.join("\n") })
    }

    pub fn image(bytes: &[u8]) -> Result<Page> {
        let engine = engine()?;
        let stream: IRandomAccessStream = windows::core::Interface::cast(&stream_of(bytes)?).map_err(err)?;
        recognize(&engine, &bitmap(&stream)?)
    }

    /// Pages of a PDF: how many there are, and a page rendered (≈ 300 dpi on A4) and recognized.
    pub struct Pdf {
        doc: PdfDocument,
        engine: Option<OcrEngine>,
    }

    impl Pdf {
        pub fn open(bytes: &[u8]) -> Result<Self> {
            let stream: IRandomAccessStream = windows::core::Interface::cast(&stream_of(bytes)?).map_err(err)?;
            let doc = PdfDocument::LoadFromStreamAsync(&stream)
                .map_err(err)?
                .join()
                .map_err(|_| Error::Invalid("Das PDF kann nicht geöffnet werden (beschädigt oder mit Passwort geschützt)".into()))?;
            Ok(Self { doc, engine: None })
        }

        pub fn pages(&self) -> Result<u32> {
            self.doc.PageCount().map_err(err)
        }

        pub fn page(&mut self, index: u32) -> Result<Page> {
            if self.engine.is_none() {
                self.engine = Some(engine()?);
            }
            let page = self.doc.GetPage(index).map_err(err)?;
            let size = page.Size().map_err(err)?;
            let out = InMemoryRandomAccessStream::new().map_err(err)?;
            let options = PdfPageRenderOptions::new().map_err(err)?;
            // Points are 1/72 inch: ×4 is ≈ 290 dpi, enough for small print.
            let max = OcrEngine::MaxImageDimension().map_err(err)?.max(1) as f32;
            let scale = (4.0f32).min(max / size.Width.max(size.Height).max(1.0));
            options.SetDestinationWidth((size.Width * scale) as u32).map_err(err)?;
            options.SetDestinationHeight((size.Height * scale) as u32).map_err(err)?;
            page.RenderWithOptionsToStreamAsync(&out, &options).map_err(err)?.join().map_err(err)?;
            out.Seek(0).map_err(err)?;
            let stream: IRandomAccessStream = windows::core::Interface::cast(&out).map_err(err)?;
            let engine = self.engine.as_ref().expect("set above");
            recognize(engine, &bitmap(&stream)?)
        }
    }
}

/// Windows can recognize German text here.
pub fn available() -> bool {
    #[cfg(windows)]
    {
        imp::available()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(not(windows))]
fn unavailable() -> Error {
    Error::Invalid("Texterkennung gibt es nur unter Windows. Bitte den Text abtippen oder diktieren.".into())
}

/// The text in a photo or scan (PNG, JPEG, TIFF, BMP, GIF; HEIC with the Windows extension).
pub fn image(bytes: &[u8]) -> Result<Page> {
    #[cfg(windows)]
    {
        imp::image(bytes)
    }
    #[cfg(not(windows))]
    {
        let _ = bytes;
        Err(unavailable())
    }
}

/// A PDF opened for page-wise recognition.
pub struct Pdf {
    #[cfg(windows)]
    inner: imp::Pdf,
}

impl Pdf {
    pub fn open(bytes: &[u8]) -> Result<Self> {
        #[cfg(windows)]
        {
            Ok(Self { inner: imp::Pdf::open(bytes)? })
        }
        #[cfg(not(windows))]
        {
            let _ = bytes;
            Err(unavailable())
        }
    }

    pub fn pages(&self) -> Result<u32> {
        #[cfg(windows)]
        {
            self.inner.pages()
        }
        #[cfg(not(windows))]
        {
            Err(unavailable())
        }
    }

    /// Renders and recognizes page `index` (0-based).
    pub fn page(&mut self, index: u32) -> Result<Page> {
        #[cfg(windows)]
        {
            self.inner.page(index)
        }
        #[cfg(not(windows))]
        {
            let _ = index;
            Err(unavailable())
        }
    }
}
