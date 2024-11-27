use serde::{Deserialize, Serialize};

pub fn to_bytes<T: Serialize>(value: &T) -> Box<[u8]> {
    postcard::to_allocvec(value)
        .expect("WebWorker serialization failed")
        .into()
}

pub fn from_bytes<'de, T: Deserialize<'de>>(bytes: &'de [u8]) -> T {
    postcard::from_bytes(bytes).expect("WebWorker deserialization failed")
}
