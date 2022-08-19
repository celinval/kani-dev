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
//!
//! We try to keep this agnostic of any Kani code in case we can contribute this back to rustc.
//!
//! Users should include `extern` functions that should be included in this analysis as part of
//! the reachability analysis.
//!
//! TODO: Allow a few extension points such as:
//!   - Search boundary via closure (e.g.: should_codegen_locally)
//!   - Partition? Parallelism?
#![allow(dead_code)]
use crate::codegen_cprover_gotoc::GotocCtx;
use rustc_data_structures::fx::FxHashSet;
use rustc_middle::mir::mono::CodegenUnit;
use rustc_middle::mir::mono::MonoItem;
use rustc_middle::mir::visit::Visitor as MirVisitor;
use rustc_middle::mir::{Body, CastKind, Constant, Location, Rvalue, Terminator, TerminatorKind};
use rustc_middle::ty::adjustment::PointerCast;
use rustc_middle::ty::{
    Closure, ClosureKind, Const, Instance, InstanceDef, ParamEnv, Ty, TyCtxt, TyKind, TypeFoldable,
    VtblEntry,
};
use rustc_span::def_id::DefId;
use rustc_span::DUMMY_SP;
use tracing::{debug, trace};

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

    /// Traverses the call graph starting from the given root. For every function, we visit all
    /// instruction looking for the items that should be included in the compilation.
    fn reachable_items(&mut self) {
        while !self.queue.is_empty() {
            let to_visit = self.queue.pop().unwrap();
            if !self.collected.contains(&to_visit) {
                match to_visit {
                    MonoItem::Fn(instance) => {
                        self.visit_fn(instance);
                    }
                    MonoItem::Static(def_id) => {
                        self.visit_static(def_id);
                    }
                    MonoItem::GlobalAsm(_) => {
                        self.visit_asm(to_visit);
                    }
                }
            }
        }
    }

    /// Visit a function and collect all mono-items reachable from its instructions.
    fn visit_fn(&mut self, instance: Instance<'tcx>) {
        debug!(?instance, "visit_fn");
        let body = self.tcx.instance_mir(instance.def);
        let mut collector =
            MonoItemsFnCollector { tcx: self.tcx, collected: FxHashSet::default(), instance, body };
        collector.visit_body(body);
        self.queue.extend(collector.collected.iter().filter(|item| !self.collected.contains(item)));
    }

    /// Visit a static object and collect drop / initialization functions.
    fn visit_static(&mut self, def_id: DefId) {
        debug!(?def_id, "visit_static");
        let instance = Instance::mono(self.tcx, def_id);

        // Collect drop function.
        let static_ty = instance.ty(self.tcx, ParamEnv::reveal_all());
        let instance = Instance::resolve_drop_in_place(self.tcx, static_ty);
        self.queue.push(MonoItem::Fn(instance.polymorphize(self.tcx)));

        // TODO: Collect initialization.
    }

    /// Visit global assembly and emit either a warning or an error.
    fn visit_asm(&mut self, item: MonoItem) {
        debug!(?item, "visit_asm");
    }
}

struct MonoItemsFnCollector<'a, 'tcx> {
    tcx: TyCtxt<'tcx>,
    collected: FxHashSet<MonoItem<'tcx>>,
    instance: Instance<'tcx>,
    body: &'a Body<'tcx>,
}

impl<'a, 'tcx> MonoItemsFnCollector<'a, 'tcx> {
    fn monomorphize<T>(&self, value: T) -> T
    where
        T: TypeFoldable<'tcx>,
    {
        trace!(instance=?self.instance, ?value, "monomorphize");
        self.instance.subst_mir_and_normalize_erasing_regions(
            self.tcx,
            ParamEnv::reveal_all(),
            value,
        )
    }

    /// Collect the implementation of all trait methods and its supertrait methods for the given
    /// concrete type.
    fn collect_vtable_methods(&mut self, concrete_ty: Ty<'tcx>, trait_ty: Ty<'tcx>) {
        assert!(!concrete_ty.is_trait());
        assert!(trait_ty.is_trait());

        if let TyKind::Dynamic(trait_list, ..) = trait_ty.kind() {
            // A trait object type can have multiple trait bounds but up to one non-auto-trait
            // bound. This non-auto-trait, named principal, is the only one that can have methods.
            if let Some(principal) = trait_list.principal() {
                let poly_trait_ref = principal.with_self_ty(self.tcx, concrete_ty);

                // Walk all methods of the trait, including those of its supertraits
                let entries = self.tcx.vtable_entries(poly_trait_ref);
                let methods = entries.iter().filter_map(|entry| match entry {
                    VtblEntry::MetadataDropInPlace
                    | VtblEntry::MetadataSize
                    | VtblEntry::MetadataAlign
                    | VtblEntry::Vacant => None,
                    VtblEntry::TraitVPtr(_) => {
                        // all super trait items already covered, so skip them.
                        None
                    }
                    VtblEntry::Method(instance) if should_codegen_locally(self.tcx, instance) => {
                        Some(MonoItem::Fn(instance.polymorphize(self.tcx)))
                    }
                    VtblEntry::Method(..) => None,
                });
                self.collected.extend(methods);
            }
        }

        // Add the destructor for the concrete type.
        let instance = Instance::resolve_drop_in_place(self.tcx, concrete_ty);
        self.collect_instance(instance, false);
    }

    /// Collect an instance depending on how it is used (invoked directly or via fn_ptr).
    fn collect_instance(&mut self, instance: Instance<'tcx>, is_direct_call: bool) {
        if should_codegen_locally(self.tcx, &instance) {
            let should_collect = match instance.def {
                InstanceDef::Virtual(..) | InstanceDef::Intrinsic(_) => {
                    assert!(is_direct_call, "Expected direct call {:?}", instance);
                    true
                }
                InstanceDef::DropGlue(_, None) => {
                    // Only need the glue if we are not calling it directly.
                    !is_direct_call
                }
                InstanceDef::DropGlue(_, Some(_))
                | InstanceDef::VtableShim(..)
                | InstanceDef::ReifyShim(..)
                | InstanceDef::ClosureOnceShim { .. }
                | InstanceDef::Item(..)
                | InstanceDef::FnPtrShim(..)
                | InstanceDef::CloneShim(..) => true,
            };
            if should_collect {
                self.collected.insert(MonoItem::Fn(instance.polymorphize(self.tcx)));
            }
        }
    }
}

/// Visit the function body looking for MonoItems that should be included in the analysis.
/// This code has been mostly taken from [rustc_monomorphize::collector::MirNeighborCollector].
impl<'a, 'tcx> MirVisitor<'tcx> for MonoItemsFnCollector<'a, 'tcx> {
    /// Collect the following:
    /// - Trait implementations when casting from concrete to dyn Trait.
    /// - Functions / Closures that have their address taken.
    /// - Thread Local.
    fn visit_rvalue(&mut self, rvalue: &Rvalue<'tcx>, location: Location) {
        debug!(rvalue=?*rvalue, "visit_rvalue");

        match *rvalue {
            Rvalue::Cast(CastKind::Pointer(PointerCast::Unsize), ref operand, target) => {
                // Check if the conversion include casting a concrete type to a trait type.
                // If so, collect items from the impl `Trait for Concrete {}`.
                let target_ty = self.monomorphize(target);
                let source_ty = self.monomorphize(operand.ty(self.body, self.tcx));
                if let Some((concrete_ty, trait_ty)) =
                    find_trait_conversion(self.tcx, source_ty, target_ty)
                {
                    self.collect_vtable_methods(concrete_ty, trait_ty);
                }
            }
            Rvalue::Cast(CastKind::Pointer(PointerCast::ReifyFnPointer), ref operand, _) => {
                let fn_ty = operand.ty(self.body, self.tcx);
                let fn_ty = self.monomorphize(fn_ty);
                if let TyKind::FnDef(def_id, substs) = *fn_ty.kind() {
                    let instance = Instance::resolve_for_fn_ptr(
                        self.tcx,
                        ParamEnv::reveal_all(),
                        def_id,
                        substs,
                    )
                    .unwrap();
                    self.collect_instance(instance, false);
                } else {
                    unreachable!("Expected FnDef type, but got: {:?}", fn_ty);
                }
            }
            Rvalue::Cast(CastKind::Pointer(PointerCast::ClosureFnPointer(_)), ref operand, _) => {
                let source_ty = operand.ty(self.body, self.tcx);
                let source_ty = self.monomorphize(source_ty);
                match *source_ty.kind() {
                    Closure(def_id, substs) => {
                        let instance = Instance::resolve_closure(
                            self.tcx,
                            def_id,
                            substs,
                            ClosureKind::FnOnce,
                        )
                        .expect("failed to normalize and resolve closure during codegen");
                        self.collect_instance(instance, false);
                    }
                    _ => unreachable!("Unexpected type: {:?}", source_ty),
                }
            }
            Rvalue::ThreadLocalRef(def_id) => {
                assert!(self.tcx.is_thread_local_static(def_id));
                let instance = Instance::mono(self.tcx, def_id);
                if should_codegen_locally(self.tcx, &instance) {
                    trace!("collecting thread-local static {:?}", def_id);
                    self.collected.insert(MonoItem::Static(def_id));
                }
            }
            _ => { /* not interesting */ }
        }

        self.super_rvalue(rvalue, location);
    }

    /// This does not walk the constant, as it has been handled entirely here and trying
    /// to walk it would attempt to evaluate the `Const` inside, which doesn't necessarily
    /// work, as some constants cannot be represented in the type system.
    fn visit_constant(&mut self, constant: &Constant<'tcx>, location: Location) {
        // TODO: Not sure if we need to do anything here.
        self.super_constant(constant, location);
    }

    fn visit_const(&mut self, constant: Const<'tcx>, _location: Location) {
        // TODO: Not sure if we need to do anything here.
        self.super_const(constant);
    }

    /// Collect function calls.
    fn visit_terminator(&mut self, terminator: &Terminator<'tcx>, location: Location) {
        debug!("visiting terminator {:?} @ {:?}", terminator, location);

        let tcx = self.tcx;
        match terminator.kind {
            TerminatorKind::Call { ref func, .. } => {
                let callee_ty = func.ty(self.body, tcx);
                let fn_ty = self.monomorphize(callee_ty);
                if let TyKind::FnDef(def_id, substs) = *fn_ty.kind() {
                    let instance =
                        Instance::resolve(self.tcx, ParamEnv::reveal_all(), def_id, substs)
                            .unwrap()
                            .unwrap();
                    self.collect_instance(instance, true);
                } else {
                    unreachable!();
                }
            }
            TerminatorKind::Drop { ref place, .. }
            | TerminatorKind::DropAndReplace { ref place, .. } => {
                let place_ty = place.ty(self.body, self.tcx).ty;
                let place_mono_ty = self.monomorphize(place_ty);
                let instance = Instance::resolve_drop_in_place(self.tcx, place_mono_ty);
                self.collect_instance(instance, true);
            }
            TerminatorKind::InlineAsm { .. } => {
                // We don't support inline assembly. Skip for now.
            }
            TerminatorKind::Abort { .. } | TerminatorKind::Assert { .. } => {
                // We generate code for this without invoking any lang item.
            }
            TerminatorKind::Goto { .. }
            | TerminatorKind::SwitchInt { .. }
            | TerminatorKind::Resume
            | TerminatorKind::Return
            | TerminatorKind::Unreachable => {}
            TerminatorKind::GeneratorDrop
            | TerminatorKind::Yield { .. }
            | TerminatorKind::FalseEdge { .. }
            | TerminatorKind::FalseUnwind { .. } => {
                unreachable!("Unexpected at this MIR level")
            }
        }

        self.super_terminator(terminator, location);
    }
}

/// Visit every instruction in a function and collect the following:
/// 1. Every function / method / closures that may be directly invoked.
/// 2. Every function / method / closures that may have their address taken.
/// 3. Every method that compose the impl of a trait for a given type when there's a conversion
/// from the type to the trait.
///    - I.e.: If we visit the following code:
///      ```
///      let var = MyType::new();
///      let ptr : &dyn MyTrait = &var;
///      ```
///      We collect the entire implementation of `MyTrait` for `MyType`.
/// 4. Every Static variable that is referenced in the function.
/// 5. Drop glue? Static Initialization?
///
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

/// Return whether we should include the item into codegen.
/// We don't include foreign items only.
fn should_codegen_locally<'tcx>(tcx: TyCtxt<'tcx>, instance: &Instance<'tcx>) -> bool {
    let def_id = instance.def_id();
    if tcx.is_foreign_item(def_id) {
        // We cannot codegen foreign items.
        false
    } else {
        assert!(tcx.is_mir_available(def_id), "no MIR available for {:?}", def_id);
        true
    }
}

/// Extract the pair (concrete, trait) for a unsized cast.
/// This function will return None if it cannot extract a trait (e.g.: unsized type is a slice).
/// This also handles nested cases: `Struct<Struct<dyn T>>` returns `dyn T`
fn find_trait_conversion<'tcx>(
    tcx: TyCtxt<'tcx>,
    src_ty: Ty<'tcx>,
    dst_ty: Ty<'tcx>,
) -> Option<(Ty<'tcx>, Ty<'tcx>)> {
    let param_env = ParamEnv::reveal_all();
    let dst_ty_inner = dst_ty.builtin_deref(true).unwrap().ty;
    if dst_ty_inner.is_sized(tcx.at(DUMMY_SP), param_env) {
        None
    } else {
        let unsized_ty = tcx.struct_tail_erasing_lifetimes(dst_ty_inner, param_env);
        match unsized_ty.kind() {
            TyKind::Foreign(..) | TyKind::Str | TyKind::Slice(..) => None,
            TyKind::Dynamic(..) => {
                let src_ty_inner = src_ty.builtin_deref(true).unwrap().ty;
                let concrete_ty = tcx.struct_tail_erasing_lifetimes(src_ty_inner, param_env);
                Some((concrete_ty, unsized_ty))
            }
            _ => unreachable!("unexpected unsized tail: {:?}", unsized_ty),
        }
    }
}
