//! Argon2 password hashes. The panel's sign-in and the derived password fields
//! both use these functions, so every stored password has the same format.

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{Error, PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;

/// Hashes `password` with argon2 and a random salt. The result is a PHC string,
/// such as `$argon2id$v=19$...`, and is safe to store.
pub fn hash_password(password: &str) -> Result<String, Error> {
    let salt = SaltString::generate(&mut OsRng);
    Ok(Argon2::default()
        .hash_password(password.as_bytes(), &salt)?
        .to_string())
}

/// Returns true when `password` matches the stored `hash`. A malformed hash never matches.
pub fn verify_password(hash: &str, password: &str) -> bool {
    match PasswordHash::new(hash) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wrong_password_does_not_verify() {
        let hash = hash_password("correct horse").unwrap();

        assert!(verify_password(&hash, "correct horse"));
        assert!(!verify_password(&hash, "wrong password"));
    }

    #[test]
    fn a_malformed_hash_never_verifies() {
        assert!(!verify_password("not-a-hash", "anything"));
    }

    #[test]
    fn hashes_use_a_fresh_salt() {
        assert_ne!(
            hash_password("same").unwrap(),
            hash_password("same").unwrap()
        );
    }
}
