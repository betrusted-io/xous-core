// Note: this file can be formatted on save because it is entirely new to the crate.

//! Message-independent XMSS Merkle tree cache for `d = 1` parameter sets
//! (NIST SP 800-230 limited-signature sets).
//!
//! With `d = 1`, the entire XMSS tree is a fixed function of the signing key —
//! it does not depend on the message. Ordinary signing therefore recomputes at
//! every signature the same 2^h' -leaf tree that key generation already built
//! once (~80% of the signing cost for SLH-DSA-SHA2-128-24). This module lets an
//! application build that tree once, persist a compact top slice of it, and
//! sign using cached authentication paths.
//!
//! # Time/memory point
//!
//! The cache stores only the nodes at heights `floor..=h'`. Per signature, the
//! height-`floor` subtree containing the selected leaf is recomputed to supply
//! the auth-path nodes below the floor; everything above comes from the cache.
//!
//! | floor | stored nodes           | cache size (n = 16, h' = 22) | per-signature recompute |
//! |-------|------------------------|------------------------------|-------------------------|
//! | 0     | `2^(h'+1) - 1`         | ~134 MB (full tree)          | 1 WOTS leaf (check)     |
//! | 10    | `2^13 - 1` = 8191      | **~128 KiB** (default)       | 2^10 leaves ≈ 3·10^5 hashes |
//! | h'    | 1 (root only)          | 24 B                         | the whole tree          |
//!
//! The default floor is `h' - 12` (≈ 128 KiB stored, ~0.3M hashes/signature —
//! noise next to the ~3·10^8 hashes FORS costs in the same signature). The
//! one-time *build* cost is unchanged: all leaves are computed once (in
//! parallel under the `parallel` feature), and the below-floor levels are
//! discarded.
//!
//! # Storage is the application's concern
//!
//! This library deliberately performs no I/O. [`XmssTreeCache`] is an opaque
//! value with a bytes boundary — [`XmssTreeCache::to_bytes`] /
//! [`XmssTreeCache::from_bytes`] — mirroring how keys are handled elsewhere in
//! this crate. Whether the bytes live in a file next to the key, a database, or
//! anywhere else is up to the caller.
//!
//! # Integrity and security
//!
//! Cache contents are *derived, non-secret* data: internal Merkle nodes, i.e.
//! exactly the material that gets published in signatures' authentication
//! paths. Storing the cache alongside the secret key adds no confidentiality
//! exposure.
//!
//! A corrupted or mismatched cache can never leak the key; the failure mode is
//! producing *invalid* signatures. Two checks defend against that:
//! [`XmssTreeCache::validate`] verifies the stored levels structurally and
//! against the verifying key's `pk_root`, and every cached signing operation
//! recomputes one height-`floor` subtree and asserts its root matches the
//! cached node above it.

use alloc::vec::Vec;
use core::fmt;

use hybrid_array::Array;
use typenum::Unsigned;

use crate::ParameterSet;
use crate::SkSeed;
use crate::address::WotsHash;
use crate::verifying_key::VerifyingKey;

/// Serialization header: magic, version, n, h', floor.
const MAGIC: [u8; 4] = *b"SLHT";
const VERSION: u8 = 1;
const HEADER_LEN: usize = 8;

/// Number of stored levels above the floor for the default configuration:
/// `floor = h' - DEFAULT_CACHE_SPAN` keeps `2^(DEFAULT_CACHE_SPAN + 1) - 1`
/// nodes (~128 KiB at n = 16).
pub(crate) const DEFAULT_CACHE_SPAN: u32 = 12;

/// A cached top slice of the XMSS Merkle tree for a `d = 1` parameter set.
///
/// Build with [`SigningKey::build_tree_cache`](crate::SigningKey::build_tree_cache)
/// (or `build_tree_cache_with_floor` for a custom time/memory point), persist
/// via [`to_bytes`](Self::to_bytes)/[`from_bytes`](Self::from_bytes), and sign
/// with
/// [`SigningKey::try_sign_with_tree_cache`](crate::SigningKey::try_sign_with_tree_cache).
pub struct XmssTreeCache<P: ParameterSet> {
    /// Lowest stored height. Auth nodes below this are recomputed per signature.
    floor: u32,
    /// `levels[i]` holds the `2^(h' - (floor + i))` nodes at height
    /// `floor + i`, in index order. `levels.last()[0]` is the tree root
    /// (equals `pk_root` when `d = 1`).
    levels: Vec<Vec<Array<u8, P::N>>>,
}

/// Errors from [`XmssTreeCache::from_bytes`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeCacheError {
    /// Missing/incorrect magic bytes or unsupported format version.
    BadHeader,
    /// Header n/h' do not match the parameter set `P`.
    WrongParameterSet,
    /// Header floor exceeds h'.
    BadFloor,
    /// Total length is inconsistent with the header.
    BadLength,
}

impl fmt::Display for TreeCacheError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadHeader => f.write_str("tree cache: bad magic or unsupported version"),
            Self::WrongParameterSet => f.write_str("tree cache: n/h' mismatch with this parameter set"),
            Self::BadFloor => f.write_str("tree cache: floor exceeds tree height"),
            Self::BadLength => f.write_str("tree cache: truncated or oversized data"),
        }
    }
}

impl core::error::Error for TreeCacheError {}

impl<P: ParameterSet> XmssTreeCache<P> {
    /// Build the tree bottom-up, keeping only heights `floor..=h'`. `ht` must
    /// be seeded with the key's `pk_seed`; addressing mirrors `xmss_node`
    /// exactly (layer 0, tree 0). All `2^h'` leaves are computed once
    /// regardless of `floor` (that is what determines the top levels); the
    /// below-floor levels are then dropped, so transient memory peaks at one
    /// full level (`2^h' * n` bytes) during the build.
    pub(crate) fn build(ht: &P, sk_seed: &SkSeed<P::N>, floor: u32) -> Self {
        let hp = P::HPrime::U32;
        debug_assert!(floor <= hp);
        let n_leaves = 1usize << hp;
        let base = WotsHash::default(); // layer 0, tree index 0: the d = 1 tree

        // Level 0: one WOTS+ public key per leaf. This is >99% of the build
        // cost, so it is the part worth parallelizing.
        let leaf = |i: usize| {
            let mut adrs = base.clone();
            adrs.key_pair_adrs.set(i as u32);
            ht.wots_pk_gen(sk_seed, &adrs)
        };

        #[cfg(feature = "parallel")]
        let level0: Vec<Array<u8, P::N>> = {
            use rayon::prelude::*;
            (0..n_leaves).into_par_iter().map(leaf).collect()
        };
        #[cfg(not(feature = "parallel"))]
        let level0: Vec<Array<u8, P::N>> = (0..n_leaves).map(leaf).collect();

        let mut levels: Vec<Vec<Array<u8, P::N>>> = Vec::with_capacity((hp - floor) as usize + 1);
        let mut current = level0; // nodes at height `j`
        for j in 0..hp {
            // Combine to height j + 1; addressing identical to `xmss_node`.
            let next: Vec<Array<u8, P::N>> = (0..current.len() / 2)
                .map(|i| {
                    let mut adrs = base.tree_adrs();
                    adrs.tree_height.set(j + 1);
                    adrs.tree_index.set(i as u32);
                    ht.h(&adrs, &current[2 * i], &current[2 * i + 1])
                })
                .collect();
            if j >= floor {
                levels.push(current);
            }
            // else: drop the below-floor level
            current = next;
        }
        levels.push(current); // height h': the root level
        Self { floor, levels }
    }

    /// The lowest stored height.
    pub fn floor(&self) -> u32 { self.floor }

    fn stored(&self, height: u32) -> &[Array<u8, P::N>] { &self.levels[(height - self.floor) as usize] }

    /// The authentication path for `idx_leaf`, as used by `xmss_sign`:
    /// `auth[j]` is the sibling of the path node at height `j`.
    ///
    /// Heights `>= floor` are read from the cache; heights `< floor` are
    /// recomputed by rebuilding the height-`floor` subtree containing
    /// `idx_leaf` (`2^floor` WOTS leaves). The recomputed subtree root is
    /// checked against the cached node above it.
    ///
    /// # Panics
    /// Panics if the recomputed subtree root disagrees with the cache — i.e.
    /// the cache does not belong to this key or is corrupt. This is preferable
    /// to silently emitting an invalid signature.
    pub(crate) fn auth_path(
        &self,
        ht: &P,
        sk_seed: &SkSeed<P::N>,
        idx_leaf: u32,
    ) -> Array<Array<u8, P::N>, P::HPrime> {
        let hp = P::HPrime::U32;
        let floor = self.floor;
        let base = WotsHash::default();
        let mut auth = Array::<Array<u8, P::N>, P::HPrime>::default();

        // Recompute the height-`floor` subtree containing idx_leaf, harvesting
        // the below-floor auth nodes on the way up. Global node addressing
        // matches `xmss_node`: leaves use key_pair_adrs, internal nodes use
        // (tree_height, global tree_index).
        let base_leaf = (idx_leaf >> floor) << floor;
        let mut level: Vec<Array<u8, P::N>> = (0..1u32 << floor)
            .map(|k| {
                let mut adrs = base.clone();
                adrs.key_pair_adrs.set(base_leaf + k);
                ht.wots_pk_gen(sk_seed, &adrs)
            })
            .collect();
        let mut rel = idx_leaf - base_leaf; // path index within the subtree
        for j in 0..floor {
            auth[j as usize] = level[(rel ^ 1) as usize].clone();
            let next: Vec<Array<u8, P::N>> = (0..level.len() / 2)
                .map(|i| {
                    let mut adrs = base.tree_adrs();
                    adrs.tree_height.set(j + 1);
                    adrs.tree_index.set((base_leaf >> (j + 1)) + i as u32);
                    ht.h(&adrs, &level[2 * i], &level[2 * i + 1])
                })
                .collect();
            level = next;
            rel >>= 1;
        }
        assert_eq!(
            level[0],
            self.stored(floor)[(idx_leaf >> floor) as usize],
            "tree cache does not match this signing key (recomputed subtree \
             root differs from cached node) — refusing to emit an invalid signature"
        );

        // Above the floor: straight lookups.
        for j in floor..hp {
            let sibling = ((idx_leaf >> j) ^ 1) as usize;
            auth[j as usize] = self.stored(j)[sibling].clone();
        }
        auth
    }

    /// The tree root, as bytes. For `d = 1` this equals the verifying key's
    /// `pk_root`.
    pub fn root(&self) -> &[u8] { self.levels[self.levels.len() - 1][0].as_slice() }

    /// Integrity check on the stored slice: shape, every stored parent equals
    /// `H(children)` (heights `floor+1..=h'`), and the root equals `vk`'s
    /// `pk_root`. Nodes at the floor itself are additionally cross-checked at
    /// every signature against a freshly recomputed subtree root. With the
    /// default floor this is a few thousand hashes — effectively free; run it
    /// after [`from_bytes`](Self::from_bytes).
    #[must_use]
    pub fn validate(&self, vk: &VerifyingKey<P>) -> bool {
        let hp = P::HPrime::U32;
        if self.floor > hp || self.levels.len() != (hp - self.floor) as usize + 1 {
            return false;
        }
        for (i, level) in self.levels.iter().enumerate() {
            if level.len() != 1usize << (hp - self.floor - i as u32) {
                return false;
            }
        }
        let ht = P::new_from_pk_seed(&vk.pk_seed);
        let base = WotsHash::default();
        for j in (self.floor + 1)..=hp {
            for i in 0..self.stored(j).len() {
                let mut adrs = base.tree_adrs();
                adrs.tree_height.set(j);
                adrs.tree_index.set(i as u32);
                let parent = ht.h(&adrs, &self.stored(j - 1)[2 * i], &self.stored(j - 1)[2 * i + 1]);
                if parent != self.stored(j)[i] {
                    return false;
                }
            }
        }
        self.root() == vk.pk_root.as_slice()
    }

    /// Serialize: 8-byte header (magic "SLHT", version, n, h', floor),
    /// followed by the stored nodes, lowest height first, in index order.
    pub fn to_bytes(&self) -> Vec<u8> {
        let n = P::N::USIZE;
        let total_nodes: usize = self.levels.iter().map(Vec::len).sum();
        let mut out = Vec::with_capacity(HEADER_LEN + total_nodes * n);
        out.extend_from_slice(&MAGIC);
        out.push(VERSION);
        out.push(n as u8);
        out.push(P::HPrime::U8);
        out.push(self.floor as u8);
        for level in &self.levels {
            for node in level {
                out.extend_from_slice(node.as_slice());
            }
        }
        out
    }

    /// Deserialize. Checks the header and total length only; call
    /// [`validate`](Self::validate) to verify contents against a key.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, TreeCacheError> {
        if bytes.len() < HEADER_LEN || bytes[..4] != MAGIC || bytes[4] != VERSION {
            return Err(TreeCacheError::BadHeader);
        }
        let n = P::N::USIZE;
        let hp = P::HPrime::U32;
        if bytes[5] as usize != n || bytes[6] as u32 != hp {
            return Err(TreeCacheError::WrongParameterSet);
        }
        let floor = bytes[7] as u32;
        if floor > hp {
            return Err(TreeCacheError::BadFloor);
        }
        let total_nodes = (1usize << (hp - floor + 1)) - 1;
        if bytes.len() != HEADER_LEN + total_nodes * n {
            return Err(TreeCacheError::BadLength);
        }

        let mut offset = HEADER_LEN;
        let mut levels = Vec::with_capacity((hp - floor) as usize + 1);
        for j in floor..=hp {
            let count = 1usize << (hp - j);
            let mut level = Vec::with_capacity(count);
            for _ in 0..count {
                level.push(Array::try_from(&bytes[offset..offset + n]).expect("length checked"));
                offset += n;
            }
            levels.push(level);
        }
        Ok(Self { floor, levels })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hashes::Sha2_128_24;
    use crate::signing_key::SigningKey;

    /// Cached signing must be bit-identical to the ordinary path, and the
    /// serialization must round-trip. Expensive (builds three full height-22
    /// trees + two FORS forests): run with
    /// `cargo test --release --features tree-cache -- --ignored`.
    #[test]
    #[ignore = "builds multiple full height-22 XMSS trees; run in release with --ignored"]
    fn cached_signature_matches_fresh_sha2_128_24() {
        let sk = SigningKey::<Sha2_128_24>::slh_keygen_internal(&[1u8; 16], &[2u8; 16], &[3u8; 16]);
        let cache = sk.build_tree_cache();
        // Default floor: h' - 12 = 10 -> 2^13 - 1 nodes.
        assert_eq!(cache.floor(), 10);
        assert_eq!(cache.to_bytes().len(), 8 + ((1 << 13) - 1) * 16);
        assert!(cache.validate(&sk.verifying_key), "freshly built cache must validate");

        let msg: &[&[u8]] = &[b"tree cache equivalence test"];
        let fresh = sk.slh_sign_internal(msg, Some(&[7u8; 16]));
        let cached = sk.slh_sign_internal_with_cache(msg, Some(&[7u8; 16]), &cache);
        assert_eq!(fresh, cached, "cached signature must equal fresh signature");

        let roundtrip = XmssTreeCache::<Sha2_128_24>::from_bytes(&cache.to_bytes()).expect("roundtrip");
        assert!(roundtrip.validate(&sk.verifying_key));
        let cached2 = sk.slh_sign_internal_with_cache(msg, Some(&[7u8; 16]), &roundtrip);
        assert_eq!(fresh, cached2);
    }
}
