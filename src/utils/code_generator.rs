use crate::error::AppError;
use base64::Engine as _;
use rand_core::{OsRng, TryRngCore};
use serde_json::json;

const CODE_LENGTH_BYTES: usize = 9;

// Зарезервированные коды
const RESERVED_CODES: &[&str] = &["stats", "health", "domains", "admin", "api", "dashboard"];

/// Генерирует уникальный код для короткой ссылки
pub fn generate_code() -> String {
    let mut buffer = [0u8; CODE_LENGTH_BYTES];

    // Используем OsRng для криптографически стойкой случайности
    OsRng
        .try_fill_bytes(&mut buffer)
        .expect("Failed to generate random bytes");

    // Кодируем в URL-safe base64 без padding
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buffer)
}

/// Валидация кастомного кода
pub fn validate_custom_code(code: &str) -> Result<(), AppError> {
    // Проверка длины
    if code.len() < 8 || code.len() > 15 {
        return Err(AppError::bad_request(
            "Custom code must be 8-15 characters",
            json!({ "provided_length": code.len() }),
        ));
    }

    // Проверка формата
    if !code
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(AppError::bad_request(
            "Custom code can only contain lowercase letters, digits, and hyphens",
            json!({ "code": code }),
        ));
    }

    // Проверка на начало/конец с дефиса
    if code.starts_with('-') || code.ends_with('-') {
        return Err(AppError::bad_request(
            "Custom code cannot start or end with a hyphen",
            json!({ "code": code }),
        ));
    }

    // Проверка зарезервированных слов
    if RESERVED_CODES.contains(&code) {
        return Err(AppError::bad_request(
            "This code is reserved",
            json!({ "code": code }),
        ));
    }

    Ok(())
}
