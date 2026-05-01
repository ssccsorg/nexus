use async_trait::async_trait;
use edgeparse_core::{
    api::config::{ProcessingConfig, TableMethod},
    convert_bytes,
    output::markdown,
};
use tracing::info;

use super::{PdfConversionConfig, PdfConverter};
use crate::error::PdfConversionError;

/// Fast CPU-only PDF converter powered by EdgeParse.
#[derive(Debug, Default)]
pub struct EdgeParsePdfConverter;

#[async_trait]
impl PdfConverter for EdgeParsePdfConverter {
    async fn convert(
        &self,
        pdf_bytes: &[u8],
        config: &PdfConversionConfig,
    ) -> Result<String, PdfConversionError> {
        let pdf_bytes = pdf_bytes.to_vec();
        let filename = config
            .filename
            .clone()
            .unwrap_or_else(|| "document.pdf".to_string());
        let table_method = config.table_method.clone();

        tokio::task::spawn_blocking(move || {
            let processing = ProcessingConfig {
                table_method: match table_method.as_deref() {
                    Some("cluster") => TableMethod::Cluster,
                    _ => TableMethod::Default,
                },
                ..Default::default()
            };

            let document = convert_bytes(&pdf_bytes, &filename, &processing)
                .map_err(|error| PdfConversionError::Backend(error.to_string()))?;
            let markdown = markdown::to_markdown(&document)
                .map_err(|error| PdfConversionError::Backend(error.to_string()))?;

            if markdown.trim().is_empty() {
                return Err(PdfConversionError::EmptyOutput(
                    "edgeparse returned no markdown",
                ));
            }

            info!(
                pages = document.number_of_pages,
                markdown_len = markdown.len(),
                "EdgeParse conversion completed"
            );

            Ok(markdown)
        })
        .await
        .map_err(|error| PdfConversionError::Internal(error.to_string()))?
    }

    fn backend_name(&self) -> &'static str {
        "edgeparse"
    }
}
