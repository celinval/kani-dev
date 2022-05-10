// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Modifications Copyright Kani Contributors
// See GitHub history for details.
crate mod escape;
crate mod layout;
mod length_limit;
// used by the error-index generator, so it needs to be public
pub mod markdown;
crate mod sources;
crate mod static_files;
crate mod toc;
