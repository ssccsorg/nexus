use std::sync::Arc;

use async_trait::async_trait;
use edgequake_llm::traits::LLMProvider;
use edgequake_pdf2md::{convert_from_bytes, ConversionConfig, FileCheckpointStore};
use tracing::info;

use super::{PdfConversionConfig, PdfConverter};
use crate::error::PdfConversionError;

/// Existing vision-based PDF converter backed by `edgequake-pdf2md`.
pub struct VisionPdfConverter {
    llm_provider: Option<Arc<dyn LLMProvider>>,
}

impl std::fmt::Debug for VisionPdfConverter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VisionPdfConverter")
            .field(
                "llm_provider",
                &self.llm_provider.as_ref().map(|_| "<provider>"),
            )
            .finish()
    }
}

impl VisionPdfConverter {
    pub fn new(llm_provider: Option<Arc<dyn LLMProvider>>) -> Self {
        Self { llm_provider }
    }
}

#[async_trait]
impl PdfConverter for VisionPdfConverter {
    async fn convert(
        &self,
        pdf_bytes: &[u8],
        config: &PdfConversionConfig,
    ) -> Result<String, PdfConversionError> {
        let provider = self
            .llm_provider
            .clone()
            .ok_or(PdfConversionError::BackendNotConfigured("vision provider"))?;
        let vision = config
            .vision
            .as_ref()
            .ok_or(PdfConversionError::BackendNotConfigured("vision config"))?;
        let model = vision
            .model
            .clone()
            .ok_or(PdfConversionError::BackendNotConfigured("vision model"))?;

        let mut builder = ConversionConfig::builder()
            .provider(provider)
            .model(model.clone());

        if let Some(concurrency) = vision.concurrency {
            builder = builder.concurrency(concurrency);
        }
        if let Some(dpi) = vision.dpi {
            builder = builder.dpi(dpi);
        }
        if let Some(progress_callback) = vision.progress_callback.clone() {
            builder = builder.progress_callback(progress_callback);
        }
        if let Some(checkpoint_dir) = vision.checkpoint_dir.clone() {
            builder = builder.checkpoint_store(Arc::new(FileCheckpointStore::new(&checkpoint_dir)));
        }
        if vision.no_resume {
            builder = builder.no_resume(true);
        }

        let conversion_config = builder
            .build()
            .map_err(|error| PdfConversionError::Backend(error.to_string()))?;
        let output = convert_from_bytes(pdf_bytes, &conversion_config)
            .await
            .map_err(|error| PdfConversionError::Backend(error.to_string()))?;

        if output.markdown.trim().is_empty() {
            return Err(PdfConversionError::EmptyOutput(
                "vision returned no markdown",
            ));
        }

        info!(
            pages = output.stats.total_pages,
            processed_pages = output.stats.processed_pages,
            markdown_len = output.markdown.len(),
            "Vision conversion completed"
        );

        Ok(output.markdown)
    }

    fn backend_name(&self) -> &'static str {
        "vision"
    }
}
