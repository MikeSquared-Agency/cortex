use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};

/// Read and validate the encryption key from the `CORTEX_ENCRYPTION_KEY` environment variable.
///
/// The key must be a base64-encoded 256-bit (32-byte) value.
pub fn derive_key() -> anyhow::Result<[u8; 32]> {
    let raw_key = std::env::var("CORTEX_ENCRYPTION_KEY").map_err(|_| {
        anyhow::anyhow!(
            "CORTEX_ENCRYPTION_KEY environment variable not set. \
             Run `cortex-server security generate-key` to create one."
        )
    })?;

    let key_bytes = BASE64
        .decode(raw_key.trim())
        .map_err(|_| anyhow::anyhow!("CORTEX_ENCRYPTION_KEY is not valid base64"))?;

    if key_bytes.len() != 32 {
        return Err(anyhow::anyhow!(
            "CORTEX_ENCRYPTION_KEY must decode to exactly 32 bytes (256 bits), \
             got {} bytes",
            key_bytes.len()
        ));
    }

    let mut output = [0u8; 32];
    output.copy_from_slice(&key_bytes);
    Ok(output)
}

/// Generate a random 256-bit key and return it as a base64 string.
pub fn generate_key() -> String {
    let key: [u8; 32] = rand::random();
    BASE64.encode(key)
}

/// Encrypt a file in-place using AES-256-GCM.
///
/// Format: `[12-byte nonce][ciphertext+tag]`
pub fn encrypt_file(path: &std::path::Path, key: &[u8; 32]) -> anyhow::Result<()> {
    let plaintext = std::fs::read(path)
        .map_err(|e| anyhow::anyhow!("Failed to read file for encryption: {}", e))?;

    let cipher = Aes256Gcm::new_from_slice(key).expect("key is always 32 bytes");

    let nonce_bytes: [u8; 12] = rand::random();
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_ref())
        .map_err(|e| anyhow::anyhow!("Encryption failed: {:?}", e))?;

    let mut output = nonce_bytes.to_vec();
    output.extend_from_slice(&ciphertext);

    std::fs::write(path, output)
        .map_err(|e| anyhow::anyhow!("Failed to write encrypted file: {}", e))?;
    Ok(())
}

/// Decrypt a file in-place using AES-256-GCM.
///
/// Expects format: `[12-byte nonce][ciphertext+tag]`
pub fn decrypt_file(path: &std::path::Path, key: &[u8; 32]) -> anyhow::Result<()> {
    let data = std::fs::read(path)
        .map_err(|e| anyhow::anyhow!("Failed to read file for decryption: {}", e))?;

    if data.len() < 12 {
        return Err(anyhow::anyhow!(
            "File is too short to be a valid encrypted database (< 12 bytes)"
        ));
    }

    let (nonce_bytes, ciphertext) = data.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);
    let cipher = Aes256Gcm::new_from_slice(key).expect("key is always 32 bytes");

    let plaintext = cipher.decrypt(nonce, ciphertext).map_err(|_| {
        anyhow::anyhow!("Decryption failed — wrong key or corrupt/unencrypted data")
    })?;

    std::fs::write(path, plaintext)
        .map_err(|e| anyhow::anyhow!("Failed to write decrypted file: {}", e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bin");
        let original = b"hello cortex world 1234567890!@#$";
        std::fs::write(&path, original).unwrap();

        let key: [u8; 32] = rand::random();
        encrypt_file(&path, &key).unwrap();

        // After encryption the file should differ
        let encrypted = std::fs::read(&path).unwrap();
        assert_ne!(encrypted, original);
        assert!(encrypted.len() > 12); // nonce + tag overhead

        decrypt_file(&path, &key).unwrap();
        let decrypted = std::fs::read(&path).unwrap();
        assert_eq!(decrypted, original);
    }

    #[test]
    fn test_wrong_key_fails() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bin");
        std::fs::write(&path, b"secret data").unwrap();

        let key: [u8; 32] = rand::random();
        encrypt_file(&path, &key).unwrap();

        let wrong_key: [u8; 32] = rand::random();
        assert!(decrypt_file(&path, &wrong_key).is_err());
    }

    #[test]
    fn test_generate_key_is_32_bytes() {
        let key_b64 = generate_key();
        let decoded = BASE64.decode(&key_b64).unwrap();
        assert_eq!(decoded.len(), 32);
    }

    #[test]
    fn test_derive_key_bad_length() {
        std::env::set_var("CORTEX_ENCRYPTION_KEY", BASE64.encode(b"tooshort"));
        let result = derive_key();
        assert!(result.is_err());
        std::env::remove_var("CORTEX_ENCRYPTION_KEY");
    }

    #[test]
    fn test_derive_key_valid() {
        let key_b64 = generate_key();
        std::env::set_var("CORTEX_ENCRYPTION_KEY", &key_b64);
        let result = derive_key();
        assert!(result.is_ok());
        std::env::remove_var("CORTEX_ENCRYPTION_KEY");
    }
}
