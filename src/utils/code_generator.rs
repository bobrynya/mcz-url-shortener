use base64::Engine as _;
use rand_core::{OsRng, TryRngCore}; // ИСПРАВЛЕНО: добавили TryRngCore

/// Длина генерируемого кода в байтах
const CODE_LENGTH_BYTES: usize = 9;

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
