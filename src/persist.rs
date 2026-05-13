//! TOML roundtrip for user state (banks, settings, paths).
//!
//! TOML has no null/None type, so `Vec<Option<Slot>>` cannot be serialized
//! directly. We use index-preserving wire types: each occupied slot is stored
//! as `{ index = N, location = ..., ... }` in a sparse array. On load we
//! reconstruct a full 10-element `Vec<Option<Slot>>` from those entries.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::state::{Bank, SamplerSettings, Slot, SLOTS_PER_BANK};

fn write_atomic(path: &Path, contents: &str) -> Result<()> {
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, contents)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Wire types (TOML representation)
// ---------------------------------------------------------------------------

/// A single occupied slot with its position index.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SlotEntry {
    index: usize,
    #[serde(flatten)]
    slot: Slot,
}

/// Wire representation of one bank.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct BankWire {
    #[serde(default)]
    slots: Vec<SlotEntry>,
}

/// Wire representation of the banks file.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct BanksFileWire {
    #[serde(default)]
    banks: Vec<BankWire>,
}

fn bank_to_wire(bank: &Bank) -> BankWire {
    let slots = bank
        .slots
        .iter()
        .enumerate()
        .filter_map(|(i, s)| {
            s.as_ref().map(|slot| SlotEntry {
                index: i,
                slot: slot.clone(),
            })
        })
        .collect();
    BankWire { slots }
}

fn wire_to_bank(wire: BankWire, file: &str) -> Result<Bank> {
    let mut bank = Bank::empty();
    for entry in wire.slots {
        if entry.index >= SLOTS_PER_BANK {
            return Err(Error::Other(format!(
                "{}: slot index {} out of range (max {})",
                file,
                entry.index,
                SLOTS_PER_BANK - 1
            )));
        }
        bank.slots[entry.index] = Some(entry.slot);
    }
    Ok(bank)
}

// ---------------------------------------------------------------------------
// Public file types (kept pub for callers that want to inspect the shape)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathsFile {
    #[serde(default)]
    pub roots: Vec<PathBuf>,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn load_banks(state_dir: &Path) -> Result<Vec<Bank>> {
    let p = state_dir.join("banks.toml");
    if !p.exists() {
        return Ok(vec![Bank::empty()]);
    }
    let s = std::fs::read_to_string(&p)?;
    let file_str = p.display().to_string();
    let wire: BanksFileWire = toml::from_str(&s).map_err(|e| Error::TomlParse {
        file: file_str.clone(),
        source: e,
    })?;
    if wire.banks.is_empty() {
        return Ok(vec![Bank::empty()]);
    }
    wire.banks
        .into_iter()
        .map(|bw| wire_to_bank(bw, &file_str))
        .collect()
}

pub fn save_banks(state_dir: &Path, banks: &[Bank]) -> Result<()> {
    let p = state_dir.join("banks.toml");
    let wire = BanksFileWire {
        banks: banks.iter().map(bank_to_wire).collect(),
    };
    let s = toml::to_string_pretty(&wire).map_err(|e| Error::TomlSerialize {
        file: p.display().to_string(),
        source: e,
    })?;
    write_atomic(&p, &s)?;
    Ok(())
}

pub fn load_settings(state_dir: &Path) -> Result<SamplerSettings> {
    let p = state_dir.join("settings.toml");
    if !p.exists() {
        return Ok(SamplerSettings::default());
    }
    let s = std::fs::read_to_string(&p)?;
    toml::from_str(&s).map_err(|e| Error::TomlParse {
        file: p.display().to_string(),
        source: e,
    })
}

pub fn save_settings(state_dir: &Path, settings: &SamplerSettings) -> Result<()> {
    let p = state_dir.join("settings.toml");
    let s = toml::to_string_pretty(settings).map_err(|e| Error::TomlSerialize {
        file: p.display().to_string(),
        source: e,
    })?;
    write_atomic(&p, &s)?;
    Ok(())
}

pub fn load_paths(state_dir: &Path) -> Result<Vec<PathBuf>> {
    let p = state_dir.join("paths.toml");
    if !p.exists() {
        return Ok(Vec::new());
    }
    let s = std::fs::read_to_string(&p)?;
    let file: PathsFile = toml::from_str(&s).map_err(|e| Error::TomlParse {
        file: p.display().to_string(),
        source: e,
    })?;
    Ok(file.roots)
}

pub fn save_paths(state_dir: &Path, roots: &[PathBuf]) -> Result<()> {
    let p = state_dir.join("paths.toml");
    let file = PathsFile {
        roots: roots.to_vec(),
    };
    let s = toml::to_string_pretty(&file).map_err(|e| Error::TomlSerialize {
        file: p.display().to_string(),
        source: e,
    })?;
    write_atomic(&p, &s)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Slot;
    use pretty_assertions::assert_eq;

    fn slot(name: &str) -> Slot {
        Slot {
            location: format!("/clips/{}", name).into(),
            name: name.to_string(),
            start: 1.5,
            end: 4.2,
            length: 10.0,
            rate: 1.0,
        }
    }

    #[test]
    fn banks_load_default_when_file_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let banks = load_banks(tmp.path()).unwrap();
        assert_eq!(banks.len(), 1);
        assert_eq!(banks[0].slots.len(), 10);
    }

    #[test]
    fn banks_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let mut b = Bank::empty();
        b.slots[0] = Some(slot("foo.mp4"));
        b.slots[3] = Some(slot("bar.mkv"));
        let banks = vec![b];
        save_banks(tmp.path(), &banks).unwrap();
        let got = load_banks(tmp.path()).unwrap();
        assert_eq!(got, banks);
    }

    #[test]
    fn settings_load_default_when_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let s = load_settings(tmp.path()).unwrap();
        assert_eq!(s, SamplerSettings::default());
    }

    #[test]
    fn settings_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = SamplerSettings::default();
        s.fixed_length_mode = true;
        s.fixed_length = 2.5;
        save_settings(tmp.path(), &s).unwrap();
        let got = load_settings(tmp.path()).unwrap();
        assert_eq!(got, s);
    }

    #[test]
    fn paths_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let roots = vec![PathBuf::from("/clips"), PathBuf::from("/media/usb")];
        save_paths(tmp.path(), &roots).unwrap();
        let got = load_paths(tmp.path()).unwrap();
        assert_eq!(got, roots);
    }

    #[test]
    fn save_banks_writes_atomically() {
        let tmp = tempfile::tempdir().unwrap();
        let banks = vec![Bank::empty()];
        save_banks(tmp.path(), &banks).unwrap();
        // After write, no .tmp file should remain.
        let tmp_path = tmp.path().join("banks.toml.tmp");
        assert!(!tmp_path.exists());
        let real = tmp.path().join("banks.toml");
        assert!(real.exists());
    }
}
