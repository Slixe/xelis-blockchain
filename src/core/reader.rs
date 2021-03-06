use crate::crypto::hash::Hash;
use std::fmt::{Display, Error, Formatter};
use std::convert::TryInto;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ReaderError {
    InvalidSize,
    InvalidValue,
    InvalidHex,
    ErrorTryInto
}

// Reader help us to read safely from bytes
// Mostly used when de-serializing an object from Serializer trait 
pub struct Reader<'a> {
    bytes: &'a[u8], // bytes to read
    total: usize // total read bytes
}

impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Reader {
            bytes,
            total: 0
        }
    }

    pub fn read_bool(&mut self) -> Result<bool, ReaderError> {
        Ok(self.read_u8()? == 1)
    }

    pub fn read_bytes<T>(&mut self, n: usize) -> Result<T, ReaderError>
    where T: for<'b> std::convert::TryFrom<&'b[u8]> {
        if n > self.size() {
            return Err(ReaderError::InvalidSize)
        }

        let result = match self.bytes[self.total..self.total+n].try_into() {
            Ok(v) => {
                Ok(v)
            },
            Err(_) => Err(ReaderError::ErrorTryInto)
        };

        self.total += n;
        result
    }

    pub fn read_bytes_32(&mut self) -> Result<[u8; 32], ReaderError> {
        self.read_bytes(32)
    }

    pub fn read_bytes_64(&mut self) -> Result<[u8; 64], ReaderError> {
        self.read_bytes(64)
    }

    pub fn read_hash(&mut self) -> Result<Hash, ReaderError> {
        Ok(Hash::new(self.read_bytes_32()?))
    }

    pub fn read_u8(&mut self) -> Result<u8, ReaderError> {
        if self.size() == 0 {
            return Err(ReaderError::InvalidSize)
        }
        let byte: u8 = self.bytes[self.total];
        self.total += 1;
        Ok(byte)
    }

    pub fn read_u16(&mut self) -> Result<u16, ReaderError> {
        Ok(u16::from_be_bytes(self.read_bytes(2)?))
    }

    pub fn read_u64(&mut self) -> Result<u64, ReaderError> {
        Ok(u64::from_be_bytes(self.read_bytes(8)?))
    }

    pub fn read_u128(&mut self) -> Result<u128, ReaderError> {
        Ok(u128::from_be_bytes(self.read_bytes(16)?))
    }

    pub fn read_string_with_size(&mut self, size: usize) -> Result<String, ReaderError> {
        let bytes: Vec<u8> = self.read_bytes(size)?;
        match String::from_utf8(bytes) {
            Ok(v) => Ok(v),
            Err(_) => Err(ReaderError::InvalidValue)
        }
    }

    pub fn read_string(&mut self) -> Result<String, ReaderError> {
        let size = self.read_u8()?;
        self.read_string_with_size(size as usize)
    }

    pub fn read_optional_string(&mut self) -> Result<Option<String>, ReaderError> {
        match self.read_u8()? {
            0 => Ok(None),
            n => Ok(Some(self.read_string_with_size(n as usize)?)),
        }
    }

    pub fn total_size(&self) -> usize {
        self.bytes.len()
    }

    pub fn size(&self) -> usize {
        self.bytes.len() - self.total
    }

    pub fn total_read(&self) -> usize {
        self.total
    }
}

impl Display for ReaderError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::result::Result<(), Error> {
        match self {
            ReaderError::ErrorTryInto => write!(f, "Error on try into"),
            ReaderError::InvalidSize => write!(f, "Invalid size"),
            ReaderError::InvalidValue => write!(f, "Invalid value"),
            ReaderError::InvalidHex => write!(f, "Invalid hex"),
        }
    }
}