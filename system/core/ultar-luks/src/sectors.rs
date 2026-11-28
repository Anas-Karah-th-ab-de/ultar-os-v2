//! `aes-xts-plain64` on 512-byte sectors: the tweak is the sector number (little endian),
//! counted from the start of the encrypted area – as dm-crypt does.

use crate::{Key, KEY_LEN, SECTOR};
use aes::cipher::{generic_array::GenericArray, KeyInit};
use aes::Aes256;
use xts_mode::{get_tweak_default, Xts128};

/// Encrypts and decrypts whole sectors with one XTS key.
pub struct Sectors(Xts128<Aes256>);

impl Sectors {
    /// XTS with the first half of `key` for the data, the second for the tweak.
    pub fn new(key: &[u8; KEY_LEN]) -> Sectors {
        let a = Aes256::new(GenericArray::from_slice(&key[..32]));
        let b = Aes256::new(GenericArray::from_slice(&key[32..]));
        Sectors(Xts128::new(a, b))
    }

    /// From a master key.
    pub fn from_key(key: &Key) -> Sectors {
        Sectors::new(key)
    }

    /// Encrypts `buf` (whole sectors) that starts at sector `first` of the area.
    pub fn encrypt(&self, first: u64, buf: &mut [u8]) {
        assert_eq!(buf.len() as u64 % SECTOR, 0, "whole sectors only");
        self.0.encrypt_area(buf, SECTOR as usize, u128::from(first), get_tweak_default);
    }

    /// Decrypts `buf` (whole sectors) that starts at sector `first` of the area.
    pub fn decrypt(&self, first: u64, buf: &mut [u8]) {
        assert_eq!(buf.len() as u64 % SECTOR, 0, "whole sectors only");
        self.0.decrypt_area(buf, SECTOR as usize, u128::from(first), get_tweak_default);
    }
}
