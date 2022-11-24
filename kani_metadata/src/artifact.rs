// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Represent information about an artifact type.

use std::ffi::OsStr;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ArtifactType {
    Goto,
    Metadata,
    SymTab,
    SymTabGoto,
    TypeMap,
    VTableRestriction,
}

impl ArtifactType {
    const fn extension(&self) -> &'static str {
        match self {
            ArtifactType::Goto => "out",
            ArtifactType::Metadata => "kani-metadata.json",
            ArtifactType::SymTab => "symtab.json",
            ArtifactType::SymTabGoto => "symtab.out",
            ArtifactType::TypeMap => "type_map.json",
            ArtifactType::VTableRestriction => "restrictions.json",
        }
    }
}

impl AsRef<str> for ArtifactType {
    fn as_ref(&self) -> &str {
        self.extension()
    }
}

impl AsRef<OsStr> for ArtifactType {
    fn as_ref(&self) -> &OsStr {
        self.extension().as_ref()
    }
}
