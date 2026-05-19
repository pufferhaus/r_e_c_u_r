//! In-memory shader bank state. Mirrors `state::Bank` for video slots but
//! holds shader-name + 8 param values per slot instead of file metadata.

use serde::{Deserialize, Serialize};

pub const SHADER_SLOTS_PER_BANK: usize = 10;
pub const MAX_SHADER_BANKS: u8 = 26;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShaderSlot {
    pub shader: String,
    #[serde(default = "default_params")]
    pub params: [f32; 8],
}

fn default_params() -> [f32; 8] {
    [0.0; 8]
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ShaderBank {
    #[serde(default)]
    pub slots: Vec<Option<ShaderSlot>>,
}

impl ShaderBank {
    pub fn empty() -> Self {
        Self {
            slots: (0..SHADER_SLOTS_PER_BANK).map(|_| None).collect(),
        }
    }

    pub fn first_empty(&self) -> Option<usize> {
        self.slots.iter().position(Option::is_none)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_bank_has_ten_none_slots() {
        let b = ShaderBank::empty();
        assert_eq!(b.slots.len(), 10);
        assert!(b.slots.iter().all(Option::is_none));
    }

    #[test]
    fn first_empty_returns_zero_on_empty_bank() {
        assert_eq!(ShaderBank::empty().first_empty(), Some(0));
    }

    #[test]
    fn first_empty_skips_filled_slots() {
        let mut b = ShaderBank::empty();
        b.slots[0] = Some(ShaderSlot {
            shader: "color_shift".into(),
            params: [0.0; 8],
        });
        assert_eq!(b.first_empty(), Some(1));
    }

    #[test]
    fn first_empty_returns_none_when_full() {
        let mut b = ShaderBank::empty();
        for i in 0..10 {
            b.slots[i] = Some(ShaderSlot {
                shader: format!("s{i}"),
                params: [0.0; 8],
            });
        }
        assert_eq!(b.first_empty(), None);
    }
}
