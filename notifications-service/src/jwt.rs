use jsonwebtoken::{self, Algorithm, DecodingKey, EncodingKey, Header, TokenData, Validation};
use serde::{Deserialize, Serialize};
use std::{time::{SystemTime, UNIX_EPOCH}, fs, env};
use tracing::warn;

#[derive(Serialize, Deserialize, Debug)]
pub struct Claims {
    exp: usize,
    pub sub: String,
}

impl Claims {
    #[allow(dead_code)]
    pub fn new(subject: String) -> Self {
        let start = SystemTime::now();
        let since_the_epoch = start
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards");
        let expiration_s = since_the_epoch.as_secs() + 60 * 60 * 24 * 100;

        Self {
            exp: expiration_s as usize,
            sub: subject,
        }
    }
}

#[allow(dead_code)]
pub fn encode(claims: &Claims) -> Result<String, jsonwebtoken::errors::Error> {
    let private_key = env::var("RSA_PRIVATE_KEY")
        .expect("missing environment variable RSA_PRIVATE_KEY");
    let private_key = fs::read(&private_key).unwrap();

    jsonwebtoken::encode(
        &Header::new(Algorithm::RS256),
        claims,
        &EncodingKey::from_rsa_pem(&private_key).unwrap(),
    )
}

#[allow(dead_code)]
pub fn decode(token: &str) -> Result<TokenData<Claims>, jsonwebtoken::errors::Error> {
    let public_key = match env::var("RSA_PUBLIC_KEY") {
        Ok(path) => {
            fs::read(&path).unwrap()
        },
        Err(_) => {
            warn!("environment variable RSA_PUBLIC_KEY was not provided. Using default public_key");
            include_bytes!("../ca.crt").to_vec()
        }
    };

    jsonwebtoken::decode::<Claims>(
        &token,
        &DecodingKey::from_rsa_pem(&public_key).unwrap(),
        &Validation::new(Algorithm::RS256),
    )
}
