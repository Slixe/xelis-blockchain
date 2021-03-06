use crate::core::reader::{Reader, ReaderError};
use crate::core::serializer::Serializer;
use crate::core::writer::Writer;
use super::address::{Address, AddressType};
use super::hash::Hash;
use std::borrow::Cow;
use std::fmt::{Display, Error, Formatter};
use rand::{rngs::OsRng, RngCore};
use std::hash::Hasher;

pub const KEY_LENGTH: usize = 32;
pub const SIGNATURE_LENGTH: usize = 64;

#[derive(Clone, Eq, Debug)]
pub struct PublicKey(ed25519_dalek::PublicKey);
pub struct PrivateKey(ed25519_dalek::SecretKey);

#[derive(Clone)]
pub struct Signature(ed25519_dalek::Signature); // ([u8; SIGNATURE_LENGTH]);

pub struct KeyPair {
    public_key: PublicKey,
    private_key: PrivateKey
}

impl PublicKey {
    pub fn verify_signature(&self, hash: &Hash, signature: &Signature) -> bool {
        use ed25519_dalek::Verifier;
        self.0.verify(hash.as_bytes(), &signature.0).is_ok()
    }

    pub fn as_bytes(&self) -> &[u8; KEY_LENGTH] {
        self.0.as_bytes()
    }

    pub fn to_address(&self) -> Address { // TODO mainnet mode based on config
        Address::new(true, AddressType::Normal, Cow::Borrowed(self))
    }
}

impl Serializer for PublicKey {
    fn write(&self, writer: &mut Writer) {
        writer.write_bytes(self.as_bytes());
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        match ed25519_dalek::PublicKey::from_bytes(&reader.read_bytes_32()?) {
            Ok(v) => Ok(PublicKey(v)),
            Err(_) => return Err(ReaderError::ErrorTryInto)
        }
    }
}

impl PartialEq for PublicKey {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl std::hash::Hash for PublicKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_bytes().hash(state);
    }
}

impl serde::Serialize for PublicKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_address().to_string())
    }
}

impl Display for PublicKey {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        write!(f, "{}", &self.to_address())
    }
}

impl PrivateKey {
    pub fn sign(&self, data: &[u8], public_key: &PublicKey) -> Signature {
        let expanded_key: ed25519_dalek::ExpandedSecretKey = (&self.0).into();
        Signature(expanded_key.sign(data, &public_key.0))
    }
}

impl KeyPair {
    pub fn new() -> Self {
        let mut csprng = OsRng {};
        let mut bytes = [0u8; KEY_LENGTH];
        csprng.fill_bytes(&mut bytes);
        let secret_key: ed25519_dalek::SecretKey = ed25519_dalek::SecretKey::from_bytes(&bytes).unwrap();
        let public_key: ed25519_dalek::PublicKey = (&secret_key).into();

        KeyPair {
            public_key: PublicKey(public_key),
            private_key: PrivateKey(secret_key)
        }
    }

    pub fn from_keys(public_key: PublicKey, private_key: PrivateKey) -> Self {
        KeyPair {
            public_key,
            private_key
        }
    }

    pub fn get_public_key(&self) -> &PublicKey {
        &self.public_key
    }

    pub fn sign(&self, data: &[u8]) -> Signature {
        self.private_key.sign(data, &self.public_key)
    }
}

impl Signature {
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
}

impl Serializer for Signature {
    fn write(&self, writer: &mut Writer) {
        writer.write_bytes(&self.0.to_bytes());
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let signature = match ed25519_dalek::Signature::from_bytes(&reader.read_bytes_64()?) {
            Ok(v) => v,
            Err(_) => return Err(ReaderError::ErrorTryInto)
        };
        let signature = Signature(signature);
        Ok(signature)
    }
}

impl PartialEq for Signature {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl std::hash::Hash for Signature {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.to_bytes().hash(state);
    }
}

impl serde::Serialize for Signature {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_hex())
    }
}

impl Display for Signature {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        write!(f, "{}", &self.to_hex())
    }
}