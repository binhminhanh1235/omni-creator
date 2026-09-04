use std::{
    fs,
    io::{Read, Write},
    path::Path,
};

use serde::Serialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{Error, Result};

pub(crate) fn atomic_write_json(path: &Path, value: &(impl Serialize + ?Sized)) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::InvalidWorkspace("target has no parent directory".to_owned()))?;
    fs::create_dir_all(parent)?;

    let temp = parent.join(format!(".omnicreator-{}.tmp", Uuid::new_v4().simple()));
    let bytes = serde_json::to_vec_pretty(value)?;
    {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
    }

    if let Err(error) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(error.into());
    }
    Ok(())
}

pub(crate) fn sha256_file(path: &Path) -> Result<(String, u64)> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 128 * 1024];
    let mut size = 0_u64;

    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        size += read as u64;
    }

    let digest = hasher.finalize();
    let hex = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok((hex, size))
}
