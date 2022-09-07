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
//!   - Search boundary via closure (e.g.: should_codegen_locally vs hooks)
//!   - Partition? Parallelism?
//!   - Drop issue: This test is broken due to the reachability analysis:
//!      - unsupported_drop_unsized.rs
#![allow(dead_code)]
use crate::codegen_cprover_gotoc::GotocCtx;
use rustc_data_structures::fx::FxHashSet;
use rustc_middle::mir::interpret::{AllocId, ConstValue, ErrorHandled, GlobalAlloc, Scalar};
use rustc_middle::mir::mono::CodegenUnit;
use rustc_middle::mir::mono::MonoItem;
use rustc_middle::mir::visit::Visitor as MirVisitor;
use rustc_middle::mir::{
    Body, CastKind, Constant, ConstantKind, Location, Rvalue, Terminator, TerminatorKind,
};
use rustc_middle::span_bug;
use rustc_middle::ty::adjustment::PointerCast;
use rustc_middle::ty::layout::{
    HasParamEnv, HasTyCtxt, LayoutError, LayoutOf, LayoutOfHelpers, TyAndLayout,
};
use rustc_middle::ty::{
    Closure, ClosureKind, Const, ConstKind, Instance, InstanceDef, ParamEnv, Ty, TyCtxt, TyKind,
    TypeAndMut, TypeFoldable, VtblEntry,
};
use rustc_span::def_id::DefId;
use rustc_span::source_map::Span;
use rustc_target::abi::{HasDataLayout, TargetDataLayout, VariantIdx};
use tracing::{debug, debug_span, trace, warn};

struct MonoItemsCollector<'tcx> {
    /// The compiler context.
    tcx: TyCtxt<'tcx>,
    /// Set of collected items used to avoid entering recursion loops.
    collected: FxHashSet<MonoItem<'tcx>>,
    /// Keep items collected sorted by visiting order which should be more stable.
    collected_sorted: Vec<MonoItem<'tcx>>,
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
        while !self.queue.is_empty() {
            let to_visit = self.queue.pop().unwrap();
            if !self.collected.contains(&to_visit) {
                self.collected.insert(to_visit);
                self.collected_sorted.push(to_visit);
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
        debug!(?def_id, "visit_static");
        let instance = Instance::mono(self.tcx, def_id);

        // Collect drop function.
        let static_ty = instance.ty(self.tcx, ParamEnv::reveal_all());
        let instance = Instance::resolve_drop_in_place(self.tcx, static_ty);
        self.queue.push(MonoItem::Fn(instance.polymorphize(self.tcx)));

        // Collect initialization.
        if let Ok(alloc) = self.tcx.eval_static_initializer(def_id) {
            for &id in alloc.inner().relocations().values() {
                self.queue.extend(global_alloc(self.tcx, id).iter());
            }
        }
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
        trace!(?concrete_ty, ?trait_ty, "collect_vtable_methods");
        assert!(!concrete_ty.is_trait(), "Already a trait: {:?}", concrete_ty);
        assert!(trait_ty.is_trait(), "Expected a trait: {:?}", trait_ty);
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
                trace!(methods=?methods.clone().collect::<Vec<_>>(), "collect_vtable_methods");
                self.collected.extend(methods);
            }
        }

        // Add the destructor for the concrete type.
        let instance = Instance::resolve_drop_in_place(self.tcx, concrete_ty);
        self.collect_instance(instance, false);
    }

    /// Collect an instance depending on how it is used (invoked directly or via fn_ptr).
    fn collect_instance(&mut self, instance: Instance<'tcx>, is_direct_call: bool) {
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
            InstanceDef::DropGlue(_, Some(_))
            | InstanceDef::VTableShim(..)
            | InstanceDef::ReifyShim(..)
            | InstanceDef::ClosureOnceShim { .. }
            | InstanceDef::Item(..)
            | InstanceDef::FnPtrShim(..)
            | InstanceDef::CloneShim(..) => true,
        };
        if should_collect && should_codegen_locally(self.tcx, &instance) {
            trace!(?instance, "collect_instance");
            self.collected.insert(MonoItem::Fn(instance.polymorphize(self.tcx)));
        }
    }

    /// Collect constant values represented by static variables.
    fn collect_const_value(&mut self, value: ConstValue<'tcx>) {
        debug!(?value, "collect_const_value");
        match value {
            ConstValue::Scalar(Scalar::Ptr(ptr, _size)) => {
                self.collected.extend(global_alloc(self.tcx, ptr.provenance).iter());
            }
            ConstValue::Slice { data: alloc, start: _, end: _ }
            | ConstValue::ByRef { alloc, .. } => {
                for &id in alloc.inner().relocations().values() {
                    self.collected.extend(global_alloc(self.tcx, id).iter())
                }
            }
            _ => {}
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
        //trace!(rvalue=?*rvalue, "visit_rvalue");

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
                trace!(?def_id, "visit_rvalue thread_local");
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
        debug!(?constant, ?location, "visit_const");
        // TODO: Not sure if we need to do anything here.
        self.super_const(constant);
    }

    /// Collect function calls.
    fn visit_terminator(&mut self, terminator: &Terminator<'tcx>, location: Location) {
        debug!(?terminator, ?location, "visit_terminator");

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

/// This is the reachability starting point. We start from every static item and proof harnesses.
/// TODO: Add option to "assess" crate by adding all public items in the crate.
/// TODO: Do we really need all static elements?
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
            MonoItem::Static(_) => Some(item),
            MonoItem::Fn(_) | MonoItem::GlobalAsm(_) => None,
        });
    // For each harness, collect items using the same collector.
    let mut collector = MonoItemsCollector {
        tcx,
        collected: FxHashSet::default(),
        queue: vec![],
        collected_sorted: vec![],
    };
    items.for_each(|item| collector.collect(item));
    collector.collected
}

/// Return whether we should include the item into codegen.
/// We don't include foreign items only.
fn should_codegen_locally<'tcx>(tcx: TyCtxt<'tcx>, instance: &Instance<'tcx>) -> bool {
    if let Some(def_id) = instance.def.def_id_if_not_guaranteed_local_codegen() {
        if tcx.is_foreign_item(def_id) {
            // We cannot codegen foreign items.
            false
        } else {
            // TODO: This should be an assert. Need to compile std with --always-encode-mir.
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

/// Extract the pair (concrete, trait) for a unsized cast.
/// This function will return None if this is not a unsizing coercion from concrete to trait.
///
/// For example, if `&u8` is being converted to `&dyn Debug`, this method would return:
/// `Some(u8, dyn Debug)`.
///
/// This method also handles nested cases and `std` smart pointers. E.g.:
///
/// Conversion between `Rc<Wrapper<T>>` into `Rc<Wrapper<dyn Debug>>` should return:
/// `Some(T, dyn Debug)`
///
/// The first step of this method is to extract the pointee types. Then we need to traverse the
/// pointee types to find the actual trait and the type implementing it.
///
/// TODO: Do we need to handle &Wrapper<dyn T1> to &dyn T2 or is that taken care of with super
/// trait handling? What about CoerceUnsized trait?
fn find_trait_conversion<'tcx>(
    tcx: TyCtxt<'tcx>,
    src_ty: Ty<'tcx>,
    dst_ty: Ty<'tcx>,
) -> Option<(Ty<'tcx>, Ty<'tcx>)> {
    let vtable_metadata = |mir_type: Ty<'tcx>| {
        let (metadata, _) = mir_type.ptr_metadata_ty(tcx, |ty| ty);
        metadata != tcx.types.unit && metadata != tcx.types.usize
    };

    trace!(?dst_ty, ?src_ty, "find_trait_conversion");
    let dst_ty_inner = find_pointer(tcx, dst_ty);
    let src_ty_inner = find_pointer(tcx, src_ty);

    trace!(?dst_ty_inner, ?src_ty_inner, "find_trait_conversion");
    (vtable_metadata(dst_ty_inner) && !vtable_metadata(src_ty_inner)).then(|| {
        let param_env = ParamEnv::reveal_all();
        tcx.struct_lockstep_tails_erasing_lifetimes(src_ty_inner, dst_ty_inner, param_env)
    })
}

/// This function extracts the pointee type of a regular pointer and std smart pointers.
///
/// TODO: Refactor this to use `CustomCoerceUnsized` logic which includes custom smart pointers.
///
/// E.g.: For `Rc<dyn T>` where the Rc definition is:
/// ```
/// pub struct Rc<T: ?Sized> {
///    ptr: NonNull<RcBox<T>>,
///    phantom: PhantomData<RcBox<T>>,
/// }
///
/// pub struct NonNull<T: ?Sized> {
///    pointer: *const T,
/// }
/// ```
///
/// This function will return `pointer: *const T`.
fn find_pointer<'tcx>(tcx: TyCtxt<'tcx>, typ: Ty<'tcx>) -> Ty<'tcx> {
    struct ReceiverIter<'tcx> {
        pub curr: Ty<'tcx>,
        pub tcx: TyCtxt<'tcx>,
    }

    impl<'tcx> LayoutOfHelpers<'tcx> for ReceiverIter<'tcx> {
        type LayoutOfResult = TyAndLayout<'tcx>;

        #[inline]
        fn handle_layout_err(&self, err: LayoutError, span: Span, ty: Ty<'tcx>) -> ! {
            span_bug!(span, "failed to get layout for `{}`: {}", ty, err)
        }
    }

    impl<'tcx> HasParamEnv<'tcx> for ReceiverIter<'tcx> {
        fn param_env(&self) -> ParamEnv<'tcx> {
            ParamEnv::reveal_all()
        }
    }

    impl<'tcx> HasTyCtxt<'tcx> for ReceiverIter<'tcx> {
        fn tcx(&self) -> TyCtxt<'tcx> {
            self.tcx
        }
    }

    impl<'tcx> HasDataLayout for ReceiverIter<'tcx> {
        fn data_layout(&self) -> &TargetDataLayout {
            self.tcx.data_layout()
        }
    }

    impl<'tcx> Iterator for ReceiverIter<'tcx> {
        type Item = (String, Ty<'tcx>);

        fn next(&mut self) -> Option<Self::Item> {
            if let TyKind::Adt(adt_def, adt_substs) = self.curr.kind() {
                assert_eq!(
                    adt_def.variants().len(),
                    1,
                    "Expected a single-variant ADT. Found {:?}",
                    self.curr
                );
                let tcx = self.tcx;
                let fields = &adt_def.variants().get(VariantIdx::from_u32(0)).unwrap().fields;
                let mut non_zsts = fields
                    .iter()
                    .filter(|field| !self.layout_of(field.ty(tcx, adt_substs)).is_zst())
                    .map(|non_zst| (non_zst.name.to_string(), non_zst.ty(tcx, adt_substs)));
                let (name, next) = non_zsts.next().expect("Expected one non-zst field.");
                assert!(non_zsts.next().is_none(), "Expected only one non-zst field.");
                self.curr = next;
                Some((name, self.curr))
            } else {
                None
            }
        }
    }
    if let Some(TypeAndMut { ty, .. }) = typ.builtin_deref(true) {
        ty
    } else {
        ReceiverIter { tcx, curr: typ }.last().unwrap().1
    }
}

/// Scans the allocation type and collect static objects.
/// TODO: Polish this. Do we need VTable and Memory?
fn global_alloc<'tcx>(tcx: TyCtxt<'tcx>, alloc_id: AllocId) -> Vec<MonoItem> {
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
            items
                .extend(alloc.inner().relocations().values().flat_map(|id| global_alloc(tcx, *id)));
        }
        GlobalAlloc::VTable(ty, trait_ref) => {
            trace!(?alloc_id, "global_alloc vtable");
            let vtable_id = tcx.vtable_allocation((ty, trait_ref));
            items.append(&mut global_alloc(tcx, vtable_id));
        }
    };
    items
}
