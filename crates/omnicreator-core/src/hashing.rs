use sha2::{Digest, Sha256};

pub fn deterministic_input_hash(parts: &[&[u8]]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"omnicreator-input-v1\0");

    for part in parts {
        hasher.update((part.len() as u64).to_be_bytes());
        hasher.update(part);
    }

    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_hash_is_stable_and_length_delimited() {
        let first = deterministic_input_hash(&[b"ab", b"c"]);
        let same = deterministic_input_hash(&[b"ab", b"c"]);
        let different_boundary = deterministic_input_hash(&[b"a", b"bc"]);

        assert_eq!(first, same);
        assert_ne!(first, different_boundary);
        assert_eq!(first.len(), 64);
    }
}
