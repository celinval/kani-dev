// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! This module provides MIR transformation passes that we want to perform before code generation.
use tracing::debug;

use crate::mir_transform::fn_call_abstractions::FnCallAbstractionPass;
use rustc_hir::def_id::DefId;
use rustc_middle::mir::{Body, MirPass};
use rustc_middle::ty::query::Providers;
use rustc_middle::ty::TyCtxt;

mod fn_call_abstractions;

// TODO: This should be replaced by rustc_interface::DEFAULT_QUERY_PROVIDERS
// once we change RMC to be a driver instead of just a codegen.
// Right now, we cannot depend on rustc_interface. The rustc_interface crate already
// depends on rustc_codegen_rmc. We cannot have cyclic dependency.
type OptimizedMIR = for<'tcx> fn(TyCtxt<'tcx>, DefId) -> &Body<'tcx>;
static mut OPTIMIZED_MIR_FN: OptimizedMIR = |_, _| {
    unimplemented!();
};

fn run_transformation_passes(tcx: TyCtxt<'tcx>, def_id: DefId) -> &Body<'tcx> {
    debug!(?def_id, "Run rustc transformation passes");
    let body: &Body<'tcx>;
    unsafe {
        body = OPTIMIZED_MIR_FN(tcx, def_id);
    }

    debug!(?def_id, "Run RMC's transformation passes");
    let mut new_body = body.clone();
    FnCallAbstractionPass::new(tcx).run_pass(tcx, &mut new_body);
    return tcx.arena.alloc(new_body);
}

/// Override optimized_mir query provider to run our transformation passes after standard passes.
pub fn provide(providers: &mut Providers) {
    unsafe {
        OPTIMIZED_MIR_FN = providers.optimized_mir;
        providers.optimized_mir = run_transformation_passes;
    }
}
