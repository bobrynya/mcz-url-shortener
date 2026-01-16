use url::Url;

/// Ошибки нормализации URL
#[derive(Debug, thiserror::Error)]
pub enum UrlNormalizationError {
    #[error("Invalid URL format: {0}")]
    InvalidFormat(String),

    #[error("Only HTTP and HTTPS protocols are allowed")]
    UnsupportedProtocol,

    #[error("Failed to normalize URL: {0}")]
    NormalizationFailed(String),
}

/// Нормализует URL для консистентного хранения
pub fn normalize_url(input: &str) -> Result<String, UrlNormalizationError> {
    // Парсим URL
    let mut url =
        Url::parse(input).map_err(|e| UrlNormalizationError::InvalidFormat(e.to_string()))?;

    // Проверяем протокол
    match url.scheme() {
        "http" | "https" => {}
        _ => return Err(UrlNormalizationError::UnsupportedProtocol),
    }

    // Приводим хост к нижнему регистру
    if let Some(host) = url.host_str() {
        let host_lowercase = host.to_ascii_lowercase();
        url.set_host(Some(&host_lowercase)).map_err(|_| {
            UrlNormalizationError::NormalizationFailed("Failed to set normalized host".to_string())
        })?;
    }

    // Удаляем фрагмент (#anchor)
    url.set_fragment(None);

    // Удаляем порты по умолчанию
    let is_default_port = matches!(
        (url.scheme(), url.port()),
        ("http", Some(80)) | ("https", Some(443))
    );
    if is_default_port {
        url.set_port(None).map_err(|_| {
            UrlNormalizationError::NormalizationFailed("Failed to remove default port".to_string())
        })?;
    }

    Ok(url.to_string())
}
