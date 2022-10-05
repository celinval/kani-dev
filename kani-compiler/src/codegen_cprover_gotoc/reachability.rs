// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! This module implements a cross-crate collector that allow us to find all items that
//! should be included in order to verify one or more proof harness.
//!
//! This module works as following:
//!   - Traverse all reachable items starting at the given starting points.
//!   - For every function, traverse its body and collect the following:
//!     - Constants / Static objects.
//!     - Functions that are called or have their address taken.
//!     - VTable methods for types that are coerced as unsized types.
//!   - For every static, collect initializer and drop functions.
//!
//! We have kept this module agnostic of any Kani code in case we can contribute this back to rustc.
use rustc_data_structures::fingerprint::Fingerprint;
use rustc_data_structures::fx::FxHashSet;
use rustc_data_structures::stable_hasher::{HashStable, StableHasher};
use rustc_hir::lang_items::LangItem;
use rustc_middle::mir::interpret::{AllocId, ConstValue, ErrorHandled, GlobalAlloc, Scalar};
use rustc_middle::mir::mono::MonoItem;
use rustc_middle::mir::visit::Visitor as MirVisitor;
use rustc_middle::mir::{
    Body, CastKind, Constant, ConstantKind, Location, Rvalue, Terminator, TerminatorKind,
};
use rustc_middle::span_bug;
use rustc_middle::traits::{ImplSource, ImplSourceUserDefinedData};
use rustc_middle::ty::adjustment::CustomCoerceUnsized;
use rustc_middle::ty::adjustment::PointerCast;
use rustc_middle::ty::{
    self, Closure, ClosureKind, Const, ConstKind, Instance, InstanceDef, ParamEnv, TraitRef, Ty,
    TyCtxt, TyKind, TypeFoldable, VtblEntry,
};
use rustc_span::def_id::DefId;
use rustc_span::source_map::DUMMY_SP;
use tracing::{debug, debug_span, trace, warn};

/// Collect all reachable items starting from the given starting points.
pub fn collect_reachable_items<'tcx>(
    tcx: TyCtxt<'tcx>,
    starting_points: &[MonoItem<'tcx>],
) -> Vec<MonoItem<'tcx>> {
    // For each harness, collect items using the same collector.
    // I.e.: This will return any item that is reachable from one or more of the starting points.
    let mut collector = MonoItemsCollector { tcx, collected: FxHashSet::default(), queue: vec![] };
    for item in starting_points {
        collector.collect(*item);
    }

    // Sort the result so code generation follows deterministic order.
    // This helps us to debug the code, but it also provides the user a good experience since the
    // order of the errors and warnings is stable.
    check_result(tcx, &collector.collected);
    let mut sorted_items: Vec<_> = collector.collected.into_iter().collect();
    sorted_items.sort_by_cached_key(|item| to_fingerprint(tcx, item));
    sorted_items
}

/// Collect all items in the crate that matches the given predicate.
pub fn filter_crate_items<F>(tcx: TyCtxt, predicate: F) -> Vec<MonoItem>
where
    F: FnMut(TyCtxt, DefId) -> bool,
{
    // Filter proof harnesses.
    let mut filter = predicate;
    tcx.hir_crate_items(())
        .items()
        .filter_map(|hir_id| {
            let def_id = hir_id.def_id.to_def_id();
            filter(tcx, def_id).then(|| MonoItem::Fn(Instance::mono(tcx, def_id)))
        })
        .collect()
}

struct MonoItemsCollector<'tcx> {
    /// The compiler context.
    tcx: TyCtxt<'tcx>,
    /// Set of collected items used to avoid entering recursion loops.
    collected: FxHashSet<MonoItem<'tcx>>,
    /// Items enqueued for visiting.
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
        while let Some(to_visit) = self.queue.pop() {
            if !self.collected.contains(&to_visit) {
                self.collected.insert(to_visit);
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
        let _guard = debug_span!("visit_fn", function=?instance).entered();
        let body = self.tcx.instance_mir(instance.def);
        let mut collector =
            MonoItemsFnCollector { tcx: self.tcx, collected: FxHashSet::default(), instance, body };
        collector.visit_body(body);
        self.queue.extend(collector.collected.iter().filter(|item| !self.collected.contains(item)));
    }

    /// Visit a static object and collect drop / initialization functions.
    fn visit_static(&mut self, def_id: DefId) {
        let _guard = debug_span!("visit_static", ?def_id).entered();
        let instance = Instance::mono(self.tcx, def_id);

        // Collect drop function.
        let static_ty = instance.ty(self.tcx, ParamEnv::reveal_all());
        let instance = Instance::resolve_drop_in_place(self.tcx, static_ty);
        self.queue.push(MonoItem::Fn(instance.polymorphize(self.tcx)));

        // Collect initialization.
        let alloc = self.tcx.eval_static_initializer(def_id).unwrap();
        for &id in alloc.inner().provenance().values() {
            self.queue.extend(collect_alloc_items(self.tcx, id).iter());
        }
    }

    /// Visit global assembly and collect its item.
    fn visit_asm(&mut self, item: MonoItem<'tcx>) {
        debug!(?item, "visit_asm");
        self.collected.insert(item);
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
        trace!(?concrete_ty, ?trait_ty, "collect_vtable_methods");
        assert!(!concrete_ty.is_trait(), "Expected a concrete type, but found: {:?}", concrete_ty);
        assert!(trait_ty.is_trait(), "Expected a trait: {:?}", trait_ty);
        if let TyKind::Dynamic(trait_list, ..) = trait_ty.kind() {
            // A trait object type can have multiple trait bounds but up to one non-auto-trait
            // bound. This non-auto-trait, named principal, is the only one that can have methods.
            // https://doc.rust-lang.org/reference/special-types-and-traits.html#auto-traits
            if let Some(principal) = trait_list.principal() {
                let poly_trait_ref = principal.with_self_ty(self.tcx, concrete_ty);

                // Walk all methods of the trait, including those of its supertraits
                let entries = self.tcx.vtable_entries(poly_trait_ref);
                let methods = entries.iter().filter_map(|entry| match entry {
                    VtblEntry::MetadataAlign
                    | VtblEntry::MetadataDropInPlace
                    | VtblEntry::MetadataSize
                    | VtblEntry::Vacant => None,
                    VtblEntry::TraitVPtr(_) => {
                        // all super trait items already covered, so skip them.
                        None
                    }
                    VtblEntry::Method(instance) if should_codegen_locally(self.tcx, instance) => {
                        Some(MonoItem::Fn(instance.polymorphize(self.tcx)))
                    }
                    VtblEntry::Method(instance) => {
                        warn!("skipping: {:?}", instance);
                        None
                    }
                });
                trace!(methods=?methods.clone().collect::<Vec<_>>(), "collect_vtable_methods");
                self.collected.extend(methods);
            }
        }

        // Add the destructor for the concrete type.
        let instance = Instance::resolve_drop_in_place(self.tcx, concrete_ty);
        self.collect_instance(instance, false, "vtable");
    }

    /// Collect an instance depending on how it is used (invoked directly or via fn_ptr).
    fn collect_instance(&mut self, instance: Instance<'tcx>, is_direct_call: bool, from: &str) {
        trace!(from, ?instance, ?is_direct_call, "collect_instance");
        let should_collect = match instance.def {
            InstanceDef::Virtual(..) | InstanceDef::Intrinsic(_) => {
                // Instance definition has no body.
                assert!(is_direct_call, "Expected direct call {:?}", instance);
                false
            }
            InstanceDef::DropGlue(_, None) => {
                // Only need the glue if we are not calling it directly.
                !is_direct_call
            }
            InstanceDef::CloneShim(..)
            | InstanceDef::ClosureOnceShim { .. }
            | InstanceDef::DropGlue(_, Some(_))
            | InstanceDef::FnPtrShim(..)
            | InstanceDef::Item(..)
            | InstanceDef::ReifyShim(..)
            | InstanceDef::VTableShim(..) => true,
        };
        if should_collect && should_codegen_locally(self.tcx, &instance) {
            trace!(?instance, "collect_instance");
            self.collected.insert(MonoItem::Fn(instance.polymorphize(self.tcx)));
        } else {
            warn!("Ignore {:?} ({})", instance, is_direct_call);
        }
    }

    /// Collect constant values represented by static variables.
    fn collect_const_value(&mut self, value: ConstValue<'tcx>) {
        debug!(?value, "collect_const_value");
        match value {
            ConstValue::Scalar(Scalar::Ptr(ptr, _size)) => {
                self.collected.extend(collect_alloc_items(self.tcx, ptr.provenance).iter());
            }
            ConstValue::Slice { data: alloc, start: _, end: _ }
            | ConstValue::ByRef { alloc, .. } => {
                for &id in alloc.inner().provenance().values() {
                    self.collected.extend(collect_alloc_items(self.tcx, id).iter())
                }
            }
            _ => {}
        }
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
/// 4. Every Static variable that is referenced in the function or constant used in the function.
/// 5. Drop glue.
/// 6. Static Initialization
/// This code has been mostly taken from `rustc_monomorphize::collector::MirNeighborCollector`.
impl<'a, 'tcx> MirVisitor<'tcx> for MonoItemsFnCollector<'a, 'tcx> {
    /// Collect the following:
    /// - Trait implementations when casting from concrete to dyn Trait.
    /// - Functions / Closures that have their address taken.
    /// - Thread Local.
    fn visit_rvalue(&mut self, rvalue: &Rvalue<'tcx>, location: Location) {
        warn!(rvalue=?*rvalue, "visit_rvalue");

        match *rvalue {
            Rvalue::Cast(CastKind::Pointer(PointerCast::Unsize), ref operand, target) => {
                warn!("visit_rvalue cast 1");
                // Check if the conversion include casting a concrete type to a trait type.
                // If so, collect items from the impl `Trait for Concrete {}`.
                let target_ty = self.monomorphize(target);
                let source_ty = self.monomorphize(operand.ty(self.body, self.tcx));
                let (src_inner, dst_inner) = extract_trait_casting(self.tcx, source_ty, target_ty);
                if !src_inner.is_trait() && dst_inner.is_trait() {
                    warn!(concrete_ty=?src_inner, trait_ty=?dst_inner, "collect_vtable_methods");
                    self.collect_vtable_methods(src_inner, dst_inner);
                }
            }
            Rvalue::Cast(CastKind::Pointer(PointerCast::ReifyFnPointer), ref operand, _) => {
                warn!("visit_rvalue cast 2");
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
                    self.collect_instance(instance, false, "rvalue");
                } else {
                    unreachable!("Expected FnDef type, but got: {:?}", fn_ty);
                }
            }
            Rvalue::Cast(CastKind::Pointer(PointerCast::ClosureFnPointer(_)), ref operand, _) => {
                warn!("visit_rvalue cast 3");
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
                        self.collect_instance(instance, false, "closure");
                    }
                    _ => unreachable!("Unexpected type: {:?}", source_ty),
                }
            }
            Rvalue::ThreadLocalRef(def_id) => {
                warn!("visit_rvalue thread local");
                assert!(self.tcx.is_thread_local_static(def_id));
                trace!(?def_id, "visit_rvalue thread_local");
                let instance = Instance::mono(self.tcx, def_id);
                if should_codegen_locally(self.tcx, &instance) {
                    trace!("collecting thread-local static {:?}", def_id);
                    self.collected.insert(MonoItem::Static(def_id));
                }
            }
            _ => {
                /* not interesting */
                warn!("visit_rvalue aff");
            }
        }

        self.super_rvalue(rvalue, location);
    }

    /// Collect constants that are represented as static variables.
    fn visit_constant(&mut self, constant: &Constant<'tcx>, location: Location) {
        let literal = self.monomorphize(constant.literal);
        debug!(?constant, ?location, ?literal, "visit_constant");
        let val = match literal {
            ConstantKind::Val(const_val, _) => const_val,
            ConstantKind::Ty(ct) => match ct.kind() {
                ConstKind::Value(v) => self.tcx.valtree_to_const_val((ct.ty(), v)),
                ConstKind::Unevaluated(un_eval) => {
                    // Thread local fall into this category.
                    match self.tcx.const_eval_resolve(ParamEnv::reveal_all(), un_eval, None) {
                        // The `monomorphize` call should have evaluated that constant already.
                        Ok(const_val) => const_val,
                        Err(ErrorHandled::TooGeneric) => span_bug!(
                            self.body.source_info(location).span,
                            "Unexpected polymorphic constant: {:?}",
                            literal
                        ),
                        Err(error) => {
                            warn!(?error, "Error already reported");
                            return;
                        }
                    }
                }
                // Nothing to do
                ConstKind::Param(..) | ConstKind::Infer(..) | ConstKind::Error(..) => return,

                // Shouldn't happen
                ConstKind::Placeholder(..) | ConstKind::Bound(..) => {
                    unreachable!("Unexpected constant type {:?} ({:?})", ct, ct.kind())
                }
            },
        };
        self.collect_const_value(val);
    }

    fn visit_const(&mut self, constant: Const<'tcx>, location: Location) {
        trace!(?constant, ?location, "visit_const");
        self.super_const(constant);
    }

    /// Collect function calls.
    fn visit_terminator(&mut self, terminator: &Terminator<'tcx>, location: Location) {
        trace!(?terminator, ?location, "visit_terminator");

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
                    self.collect_instance(instance, true, "Call");
                } else {
                    assert!(
                        matches!(fn_ty.kind(), TyKind::FnPtr(..)),
                        "Unexpected type: {:?}",
                        fn_ty
                    );
                }
            }
            TerminatorKind::Drop { ref place, .. }
            | TerminatorKind::DropAndReplace { ref place, .. } => {
                let place_ty = place.ty(self.body, self.tcx).ty;
                let place_mono_ty = self.monomorphize(place_ty);
                let instance = Instance::resolve_drop_in_place(self.tcx, place_mono_ty);
                self.collect_instance(instance, true, "drop/replace");
            }
            TerminatorKind::InlineAsm { .. } => {
                // We don't support inline assembly. This shall be replaced by an unsupported
                // construct during codegen.
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

/// Convert a `MonoItem` into a stable `Fingerprint` which can be used as a stable hash across
/// compilation sessions. This allow us to provide a stable deterministic order to codegen.
fn to_fingerprint(tcx: TyCtxt, item: &MonoItem) -> Fingerprint {
    tcx.with_stable_hashing_context(|mut hcx| {
        let mut hasher = StableHasher::new();
        item.hash_stable(&mut hcx, &mut hasher);
        hasher.finish()
    })
}

/// Function that allow us to easily identify if we are missing any component that might be
/// included by the monomorphizer.
/// To use this, make sure that either the method has a main that only calls all harnesses or
/// that the harnesses are the only roots of the package.
#[allow(dead_code)]
fn check_result<'tcx>(tcx: TyCtxt<'tcx>, result: &FxHashSet<MonoItem<'tcx>>) {
    // Use rustc monomorphizer to retrieve items to codegen.
    tcx.collect_and_partition_mono_items(())
        .1
        .iter()
        .flat_map(|cgu| cgu.items_in_deterministic_order(tcx))
        .filter(|(item, _)| !result.contains(item))
        .for_each(|item| tracing::error!("Missing: {:?}", item));
}

/// Return whether we should include the item into codegen.
/// We don't include foreign items and items that don't have MIR.
fn should_codegen_locally<'tcx>(tcx: TyCtxt<'tcx>, instance: &Instance<'tcx>) -> bool {
    if let Some(def_id) = instance.def.def_id_if_not_guaranteed_local_codegen() {
        if tcx.is_foreign_item(def_id) {
            // We cannot codegen foreign items.
            false
        } else {
            // TODO: This should either be an assert or a warning.
            // Need to compile std with --always-encode-mir first though.
            // https://github.com/model-checking/kani/issues/1605
            // assert!(tcx.is_mir_available(def_id), "no MIR available for {:?}", def_id);
            (!tcx.is_mir_available(def_id)).then(|| warn!(?def_id, "Missing MIR"));
            tcx.is_mir_available(def_id)
        }
    } else {
        // This will include things like VTableShim and other stuff. See the method
        // def_id_if_not_guaranteed_local_codegen for the full list.
        true
    }
}

/// Extract the pair (from_ty, to_ty) for a unsized cast.
///
/// For example, if `&u8` is being converted to `&dyn Debug`, this method would return:
/// `(u8, dyn Debug)`.
///
/// This method also handles nested cases and `std` smart pointers. E.g.:
///
/// Conversion between `Rc<Wrapper<String>>` into `Rc<Wrapper<dyn Debug>>` should return:
/// `(String, dyn Debug)`
///
/// TODO: Do we need to handle &Wrapper<dyn T1> to &dyn T2 or is that taken care of with super
/// trait handling?
/// <https://github.com/model-checking/kani/issues/1692>
fn extract_trait_casting<'tcx>(
    tcx: TyCtxt<'tcx>,
    src_ty: Ty<'tcx>,
    dst_ty: Ty<'tcx>,
) -> (Ty<'tcx>, Ty<'tcx>) {
    trace!(?dst_ty, ?src_ty, "find_trait_conversion");
    let (src_ty_inner, dst_ty_inner) = find_vtable_types_for_unsizing(tcx, src_ty, dst_ty);
    warn!(?dst_ty_inner, ?src_ty_inner, "find_trait_conversion result");
    (src_ty_inner, dst_ty_inner)
}

/// For a given pair of source and target type that occur in an unsizing coercion,
/// this function finds the pair of types that determines the vtable linking
/// them.
///
/// For example, the source type might be `&SomeStruct` and the target type
/// might be `&dyn SomeTrait` in a cast like:
///
/// ```rust,ignore (not real code)
/// let src: &SomeStruct = ...;
/// let target = src as &dyn SomeTrait;
/// ```
///
/// Then the output of this function would be (SomeStruct, SomeTrait) since for
/// constructing the `target` fat-pointer we need the vtable for that pair.
///
/// Things can get more complicated though because there's also the case where
/// the unsized type occurs as a field:
///
/// ```rust
/// struct ComplexStruct<T: ?Sized> {
///    a: u32,
///    b: f64,
///    c: T
/// }
/// ```
///
/// In this case, if `T` is sized, `&ComplexStruct<T>` is a thin pointer. If `T`
/// is unsized, `&SomeStruct` is a fat pointer, and the vtable it points to is
/// for the pair of `T` (which is a trait) and the concrete type that `T` was
/// originally coerced from:
///
/// ```rust,ignore (not real code)
/// let src: &ComplexStruct<SomeStruct> = ...;
/// let target = src as &ComplexStruct<dyn SomeTrait>;
/// ```
///
/// Again, we want this `find_vtable_types_for_unsizing()` to provide the pair
/// `(SomeStruct, SomeTrait)`.
///
/// Finally, there is also the case of custom unsizing coercions, e.g., for
/// smart pointers such as `Rc` and `Arc`.
fn find_vtable_types_for_unsizing<'tcx>(
    tcx: TyCtxt<'tcx>,
    source_ty: Ty<'tcx>,
    target_ty: Ty<'tcx>,
) -> (Ty<'tcx>, Ty<'tcx>) {
    let ptr_vtable = |inner_source: Ty<'tcx>, inner_target: Ty<'tcx>| {
        let param_env = ty::ParamEnv::reveal_all();
        let type_has_metadata = |ty: Ty<'tcx>| -> bool {
            if ty.is_sized(tcx.at(DUMMY_SP), param_env) {
                return false;
            }
            let tail = tcx.struct_tail_erasing_lifetimes(ty, param_env);
            match tail.kind() {
                ty::Foreign(..) => false,
                ty::Str | ty::Slice(..) | ty::Dynamic(..) => true,
                _ => unreachable!("unexpected unsized tail: {:?}", tail),
            }
        };
        if type_has_metadata(inner_source) {
            (inner_source, inner_target)
        } else {
            tcx.struct_lockstep_tails_erasing_lifetimes(inner_source, inner_target, param_env)
        }
    };

    match (&source_ty.kind(), &target_ty.kind()) {
        (&ty::Ref(_, a, _), &ty::Ref(_, b, _) | &ty::RawPtr(ty::TypeAndMut { ty: b, .. }))
        | (&ty::RawPtr(ty::TypeAndMut { ty: a, .. }), &ty::RawPtr(ty::TypeAndMut { ty: b, .. })) => {
            ptr_vtable(*a, *b)
        }
        (&ty::Adt(def_a, _), &ty::Adt(def_b, _)) if def_a.is_box() && def_b.is_box() => {
            ptr_vtable(source_ty.boxed_ty(), target_ty.boxed_ty())
        }

        (&ty::Adt(source_adt_def, source_substs), &ty::Adt(target_adt_def, target_substs)) => {
            assert_eq!(source_adt_def, target_adt_def);

            let CustomCoerceUnsized::Struct(coerce_index) =
                custom_coerce_unsize_info(tcx, source_ty, target_ty);

            let source_fields = &source_adt_def.non_enum_variant().fields;
            let target_fields = &target_adt_def.non_enum_variant().fields;

            assert!(
                coerce_index < source_fields.len() && source_fields.len() == target_fields.len()
            );

            find_vtable_types_for_unsizing(
                tcx,
                source_fields[coerce_index].ty(tcx, source_substs),
                target_fields[coerce_index].ty(tcx, target_substs),
            )
        }
        _ => unreachable!(
            "find_vtable_types_for_unsizing: invalid coercion {:?} -> {:?}",
            source_ty, target_ty
        ),
    }
}

fn custom_coerce_unsize_info<'tcx>(
    tcx: TyCtxt<'tcx>,
    source_ty: Ty<'tcx>,
    target_ty: Ty<'tcx>,
) -> CustomCoerceUnsized {
    let def_id = tcx.require_lang_item(LangItem::CoerceUnsized, None);

    let trait_ref = ty::Binder::dummy(TraitRef {
        def_id,
        substs: tcx.mk_substs_trait(source_ty, &[target_ty.into()]),
    });

    match tcx.codegen_select_candidate((ParamEnv::reveal_all(), trait_ref)) {
        Ok(ImplSource::UserDefined(ImplSourceUserDefinedData { impl_def_id, .. })) => {
            tcx.coerce_unsized_info(impl_def_id).custom_kind.unwrap()
        }
        impl_source => {
            unreachable!("invalid `CoerceUnsized` impl_source: {:?}", impl_source);
        }
    }
}

/// Scans the allocation type and collect static objects.
fn collect_alloc_items<'tcx>(tcx: TyCtxt<'tcx>, alloc_id: AllocId) -> Vec<MonoItem> {
    let mut items = vec![];
    match tcx.global_alloc(alloc_id) {
        GlobalAlloc::Static(def_id) => {
            assert!(!tcx.is_thread_local_static(def_id));
            let instance = Instance::mono(tcx, def_id);
            should_codegen_locally(tcx, &instance).then(|| {
                trace!(?def_id, "global_alloc");
                items.push(MonoItem::Static(def_id))
            });
        }
        GlobalAlloc::Function(instance) => {
            should_codegen_locally(tcx, &instance).then(|| {
                trace!(?alloc_id, ?instance, "global_alloc");
                items.push(MonoItem::Fn(instance.polymorphize(tcx)))
            });
        }
        GlobalAlloc::Memory(alloc) => {
            trace!(?alloc_id, "global_alloc memory");
            items.extend(
                alloc.inner().provenance().values().flat_map(|id| collect_alloc_items(tcx, *id)),
            );
        }
        GlobalAlloc::VTable(ty, trait_ref) => {
            trace!(?alloc_id, "global_alloc vtable");
            let vtable_id = tcx.vtable_allocation((ty, trait_ref));
            items.append(&mut collect_alloc_items(tcx, vtable_id));
        }
    };
    items
}
