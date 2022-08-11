// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! This module implements a proof of concept collector that allow us to find all items that
//! should be included in order to verify one or more proof harness.
//!
//! Note: For now we run one traversal for all harnesses and rely on CBMC to slice them further.
//! We could potentially do the following:
//! 1- Run collection on a per harness fashion.
//! 2- Run codegen in the union of all items collected in 1.
//! 3- Use the per-harness collection result to generate the symtab files.
#![allow(dead_code)]
use crate::codegen_cprover_gotoc::GotocCtx;
use rustc_data_structures::fx::FxHashSet;
use rustc_middle::mir::mono::CodegenUnit;
use rustc_middle::mir::mono::MonoItem;
use rustc_middle::mir::visit::Visitor as MirVisitor;
use rustc_middle::ty::TyCtxt;
use tracing::debug;

struct MonoItemsCollector<'tcx> {
    tcx: TyCtxt<'tcx>,
    collected: FxHashSet<MonoItem<'tcx>>,
    queue: Vec<MonoItem<'tcx>>,
}

impl<'tcx> MonoItemsCollector<'tcx> {
    /// Collects all reachable items starting from the given root.
    pub fn collect(&mut self, root: MonoItem<'tcx>) {
        debug!(?root, "collect");
        self.queue.push(root);
        self.reachable_items();
    }

    /// TODO: Implement this.
    fn reachable_items(&mut self) {}
}

impl<'tcx> MirVisitor<'tcx> for MonoItemsCollector<'tcx> {}

pub fn collect_reachable_items<'tcx>(
    tcx: TyCtxt<'tcx>,
    ctx: &GotocCtx,
    codegen_units: &'tcx [CodegenUnit<'tcx>],
) -> FxHashSet<MonoItem<'tcx>> {
    // Filter proof harnesses.
    let items = codegen_units
        .iter()
        .flat_map(|cgu| cgu.items_in_deterministic_order(tcx))
        .filter_map(|(item, _)| match item {
            MonoItem::Fn(instance) if ctx.is_proof_harness(&instance) => Some(item),
            MonoItem::Fn(_) | MonoItem::Static(_) | MonoItem::GlobalAsm(_) => None,
        });
    // For each harness, collect items using the same collector.
    let mut collector = MonoItemsCollector { tcx, collected: FxHashSet::default(), queue: vec![] };
    items.for_each(|item| collector.collect(item));
    collector.collected
}
