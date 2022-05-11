// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Modifications Copyright Kani Contributors
// See GitHub history for details.
crate mod cache;
crate mod item_type;
crate mod renderer;

use rustc_hir::def_id::DefId;

use crate::clean;

/// For different handling of associated items from the Deref target of a type rather than the type
/// itself.
#[derive(Copy, Clone, PartialEq)]
crate enum RenderMode {
    Normal,
    ForDeref { mut_: bool },
}

/// Metadata about implementations for a type or trait.
#[derive(Clone, Debug)]
crate struct Impl {
    crate impl_item: clean::Item,
}

impl Impl {
    crate fn inner_impl(&self) -> &clean::Impl {
        match *self.impl_item.kind {
            clean::ImplItem(ref impl_) => impl_,
            _ => panic!("non-impl item found in impl"),
        }
    }

    crate fn trait_did(&self) -> Option<DefId> {
        self.inner_impl().trait_.as_ref().map(|t| t.def_id())
    }
}
