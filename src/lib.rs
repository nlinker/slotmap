#![deny(warnings, missing_docs, missing_debug_implementations)]
#![doc(html_root_url = "https://docs.rs/slotmap/0.2.1")]
#![crate_name = "slotmap"]
#![cfg_attr(feature = "unstable", feature(untagged_unions))]

//! # slotmap
//!
//! This library provides two containers with persistent unique keys to access
//! stored values, [`SlotMap`] and [`DenseSlotMap`]. Upon insertion a key
//! is returned that can be used to later access or remove the values.
//! Insertion, deletion and access all take O(1) time with low overhead. Great
//! for storing collections of objects that need stable, safe references but
//! have no clear ownership otherwise, such as game entities or graph nodes.
//!
//! The difference between a [`BTreeMap`] or [`HashMap`] and a slot map is
//! that the slot map generates and returns the key when inserting a value. A
//! key is always unique and will only refer to the value that was inserted.
//! A slot map's main purpose is to simply own things in a safe and efficient
//! manner.
//!
//! # Examples
//!
//! ```
//! # use slotmap::*;
//! let mut sm = SlotMap::new();
//! let foo = sm.insert("foo");  // Key generated on insert.
//! let bar = sm.insert("bar");
//! assert_eq!(sm[foo], "foo");
//! assert_eq!(sm[bar], "bar");
//!
//! sm.remove(bar);
//! let reused = sm.insert("reuse");  // Space from bar reused.
//! assert_eq!(sm.contains_key(bar), false);  // After deletion a key stays invalid.
//! ```
//!
//! # Serialization through [`serde`]
//!
//! Both [`Key`] and the slot maps have full (de)seralization support through
//! the [`serde`] library. A key remains valid for a slot map even after one or
//! both have been serialized and deserialized! This makes storing or
//! transferring complicated referential structures and graphs a breeze. Care has
//! been taken such that deserializing keys and slot maps from untrusted sources
//! is safe.
//!
//! # Why not [`slab`]?
//!
//! Unlike [`slab`], the keys returned by the slots maps are versioned. This
//! means that once a key is removed, it stays removed, even if the physical
//! storage inside the slotmap is re-used for new elements. A [`DenseSlotMap`]
//! also provides faster iteration than [`slab`] does. Additionally, at the time
//! of writing [`slab`] does not support serialization.
//!
//! # Choosing `SlotMap` or `DenseSlotMap`
//!
//! The overhead on access with a [`Key`] in a [`SlotMap`] compared to
//! storing your elements in a [`Vec`] is a mere equality check.  However, as
//! there can be 'holes' in the underlying representation of a [`SlotMap`]
//! iteration can be inefficient when many slots are unoccupied (a slot gets
//! unoccupied when an element is removed, and gets filled back up on
//! insertion). If you often require fast iteration over all values, we also
//! provide a [`DenseSlotMap`]. It trades random access performance for fast
//! iteration over values by storing the actual values contiguously and using an
//! extra array access to translate a key into a value index.
//!
//! # Performance characteristics and implementation details
//!
//! Insertion, access and deletion is all O(1) with low overhead by storing the
//! elements inside a [`Vec`]. Unlike references or indices into a vector,
//! unless you remove a key it is never invalidated. Behind the scenes each
//! slot in the vector is a `(value, version)` tuple. After insertion the
//! returned key also contains a version. Only when the stored version and
//! version in a key match is a key valid. This allows us to reuse space in the
//! vector after deletion without letting removed keys point to spurious new
//! elements. After 2<sup>31</sup> deletions and insertions to the same
//! underlying slot the version wraps around and such a spurious reference
//! could potentially occur. It is incredibly unlikely however, and in all
//! circumstances is the behavior safe. A slot map can hold up to
//! 2<sup>32</sup> - 1 elements at a time.
//!
//! A slot map never shrinks - it couldn't even if it wanted to. It needs to
//! remember for each slot what the latest version is as to not generate
//! duplicate keys. The overhead for each element in [`SlotMap`] is 8 bytes. In
//! [`DenseSlotMap`] it is 12 bytes.
//!
//! [`Vec`]: https://doc.rust-lang.org/std/vec/struct.Vec.html
//! [`BTreeMap`]: https://doc.rust-lang.org/std/collections/struct.BTreeMap.html
//! [`HashMap`]: https://doc.rust-lang.org/std/collections/struct.HashMap.html
//! [`Key`]: struct.Key.html
//! [`SlotMap`]: struct.SlotMap.html
//! [`DenseSlotMap`]: dense/struct.DenseSlotMap.html
//! [`serde`]: https://github.com/serde-rs/serde
//! [`slab`]: https://github.com/carllerche/slab

#[cfg(feature = "serde")]
#[macro_use]
extern crate serde;

#[cfg(test)]
#[macro_use]
extern crate quickcheck;

#[cfg(test)]
extern crate serde_json;

pub(crate) mod normal;
pub use normal::*;

pub mod dense;
pub use dense::DenseSlotMap;

use std::num::NonZeroU32;

/// Key used to access stored values in a slot map.
///
/// Do not use a key from one slot map in another. The behavior is safe but
/// non-sensical (and might panic in case of out-of-bounds). Keys implement
/// `Ord` so they can be used in e.g.
/// [`BTreeMap`](https://doc.rust-lang.org/std/collections/struct.BTreeMap.html)
/// but their order is arbitrary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Key {
    idx: u32,
    version: NonZeroU32,
}

impl Key {
    fn new(idx: u32, version: u32) -> Self {
        Self {
            idx,
            version: NonZeroU32::new(version).unwrap(),
        }
    }

    /// Creates a new key that is always invalid and distinct from any non-null
    /// key. A null key can only be created through this method, or default
    /// initialization of `Key`.
    ///
    /// A null key is always invalid, but an invalid key (that is, a key that
    /// has been removed from the slot map) does not become a null key. A null
    /// is safe to use with any safe method of any slot map instance.
    ///
    /// # Examples
    ///
    /// ```
    /// # use slotmap::*;
    /// let mut sm = SlotMap::<i32>::new();
    /// let nk = Key::null();
    /// assert!(nk.is_null());
    /// assert_eq!(sm.get(nk), None);
    /// ```
    pub fn null() -> Self {
        Self::new(std::u32::MAX, 1)
    }

    /// Checks if a key is null. There is only a single null key, that is
    /// `a.is_null() && b.is_null()` implies `a == b`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use slotmap::*;
    /// let a = Key::null();
    /// let b = Key::default();
    /// assert_eq!(a, b);
    /// ```
    pub fn is_null(self) -> bool {
        self.idx == std::u32::MAX
    }
}

impl Default for Key {
    fn default() -> Self {
        Self::null()
    }
}

// Serialization with serde.
#[cfg(feature = "serde")]
mod serialize {
    use super::*;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize, Deserialize)]
    pub struct SerKey {
        idx: u32,
        version: u32,
    }

    impl Serialize for Key {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            let ser_key = SerKey {
                idx: self.idx,
                version: self.version.get(),
            };
            ser_key.serialize(serializer)
        }
    }

    impl<'de> Deserialize<'de> for Key {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            let mut ser_key: SerKey = Deserialize::deserialize(deserializer)?;

            // Ensure a.is_null() && b.is_null() implies a == b.
            if ser_key.idx == std::u32::MAX {
                ser_key.version = 1;
            }

            ser_key.version |= 1; // Ensure version is odd.
            Ok(Key::new(ser_key.idx, ser_key.version))
        }
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "serde")]
    use super::*;

    #[cfg(feature = "serde")]
    #[test]
    fn key_serde() {
        // Check round-trip through serde.
        let mut sm = SlotMap::new();
        let k = sm.insert(42);
        let ser = serde_json::to_string(&k).unwrap();
        let de: Key = serde_json::from_str(&ser).unwrap();
        assert_eq!(k, de);

        // Even if a malicious entity sends up even (unoccupied) versions in the
        // key, we make the version point to the occupied version.
        let malicious = serde_json::from_str::<Key>(&r#"{"idx":0,"version":4}"#).unwrap();
        assert_eq!(malicious.version.get(), 5);
    }
}
