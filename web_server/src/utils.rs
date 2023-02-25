use sha2::{Sha256, Digest};

pub fn encode_id(id: i32) -> String {
    let hasher = crate::HASH_ID.read().expect("Cannot get ID hasher");
    hasher.encode(&[id.try_into().unwrap()])
}

/// Decode a hash ID, if wrong it return -1
pub fn decode_id(hash: String) -> i32 {
    let hasher = crate::HASH_ID.read().expect("Cannot get ID hasher");
    let result = hasher.decode(hash);
    if let Ok(ids) = result {
        return ids[0].try_into().unwrap();
    }
    log::error!("Cannot decode hash ID");
    -1
}

pub fn sha256(value: String) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    format!("{:x}", hasher.finalize())
}


#[cfg(test)]
mod tests {
    use crate::utils::{sha256, encode_id, decode_id};

    #[test]
    fn test_sha256() {
        assert_eq!(sha256(String::from("Test string")), "a3e49d843df13c2e2a7786f6ecd7e0d184f45d718d1ac1a8a63e570466e489dd");
        assert_ne!(sha256(String::from("Test string")), "0000000000000000000000000000000000000000000000000000000000000000");
    }

    #[test]
    fn test_hash_ids() {
        let re = regex::Regex::new(r"[a-zA-T0-9]+").unwrap();
        let encoded = encode_id(1234);
        assert!(encoded.len() >= 8);
        assert!(re.is_match(&encoded));
        let decoded = decode_id(encoded);
        assert_eq!(decoded, 1234);
    }
}

