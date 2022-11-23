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
