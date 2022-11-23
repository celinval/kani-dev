// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Represent the work done to generate output files

use std::ffi::OsStr;

pub enum KaniFileType {
    Goto,
    Metadata,
    SymTab,
    TypeMap,
    VTableRestriction,
}

impl KaniFileType {
    const fn extension(&self) -> &'static str {
        match self {
            KaniFileType::Goto => "symtab.out",
            KaniFileType::Metadata => "kani-metadata.json",
            KaniFileType::SymTab => "symtab.json",
            KaniFileType::TypeMap => "type_map.json",
            KaniFileType::VTableRestriction => "restrictions.json",
        }
    }
}

impl AsRef<str> for KaniFileType {
    fn as_ref(&self) -> &str {
        self.extension()
    }
}

impl AsRef<OsStr> for KaniFileType {
    fn as_ref(&self) -> &OsStr {
        self.extension().as_ref()
    }
}
