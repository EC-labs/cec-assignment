use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm,
    Key, // Or `Aes128Gcm`
};
use base64::{engine::general_purpose, Engine as _};
use generic_array::GenericArray;
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone)]
pub enum NotificationType {
    OutOfRange,
    Stabilized,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct HashData {
    pub notification_type: Option<NotificationType>,
    pub researcher: String,
    pub experiment_id: String,
    pub measurement_id: String,
    pub timestamp: f64,
}

#[derive(Debug, Serialize)]
pub enum DecryptError {
    MalformedHashDataString,
    MalformedB64Nonce,
    MalformedB64Ciphertext,
    DecryptionError,
    Utf8DecodingError,
    JsonDeserializationError,
}

impl std::error::Error for DecryptError {}

impl std::fmt::Display for DecryptError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl HashData {
    pub fn decrypt(key: &[u8], hash_data: &str) -> Result<HashData, DecryptError> {
        let cipher_components: Vec<_> = hash_data.split('.').collect();
        if cipher_components.len() != 2 {
            return Err(DecryptError::MalformedHashDataString);
        }
        let key = Key::<Aes256Gcm>::from_slice(key);
        let cipher = Aes256Gcm::new(key);
        let nonce = general_purpose::STANDARD_NO_PAD
            .decode(cipher_components[0])
            .map_err(|_| DecryptError::MalformedB64Nonce)?;
        let nonce = GenericArray::clone_from_slice(&nonce[..]);
        let ciphertext = general_purpose::STANDARD_NO_PAD
            .decode(cipher_components[1])
            .map_err(|_| DecryptError::MalformedB64Ciphertext)?;
        let plaintext = cipher
            .decrypt(&nonce, ciphertext.as_ref())
            .map_err(|_| DecryptError::DecryptionError)?;
        let plaintext =
            String::from_utf8(plaintext).map_err(|_| DecryptError::Utf8DecodingError)?;

        let hash_data: HashData =
            serde_json::from_str(&plaintext).map_err(|_| DecryptError::JsonDeserializationError)?;
        Ok(hash_data)
    }

    pub fn encrypt(&self, key: &[u8]) -> String {
        let key = Key::<Aes256Gcm>::from_slice(key);
        let cipher = Aes256Gcm::new(key);
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng); // 96-bits; unique per message
        let ciphertext = cipher
            .encrypt(&nonce, serde_json::to_string(&self).unwrap().as_bytes())
            .unwrap();

        let b64_cipher: String = general_purpose::STANDARD_NO_PAD.encode(ciphertext);
        let b64_nonce: String = general_purpose::STANDARD_NO_PAD.encode(nonce);
        b64_nonce + "." + &b64_cipher
    }
}
