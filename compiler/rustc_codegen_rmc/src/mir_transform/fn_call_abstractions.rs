// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! This transformation pass applies proof abstractions to some structures that are hard to reason
//! about. Today we only replace certain function calls and we only support behaviorally equivalent
//! abstractions.
//!
//! In order to support over-approximations and under-approximations we would need to provide a
//! better mechanism to report proof results to reflect that.
//!
//! The algorithm today is rather simple:
//! 1- Create a map of abstraction types and the def_id for the abstraction implementation. We use
//!    rustc_diagnostic_item to tag them inside rmc crate.
//! 2- Iterate over all basic blocks of the current function looking for function calls that we want
//!    to abstract. Note that function calls are always BB terminators. So we don't need to look at
//!    other instructions inside the BB.
//! 3- Whenever a function of interest is found, we try to replace them with the abstraction.
//!    No changes are needed to the BB structure.
use rustc_hir::def_id::LOCAL_CRATE;
use rustc_index::vec::Idx;
use rustc_middle::bug;
use rustc_middle::mir::*;
use rustc_middle::ty::{self, TyCtxt};
use rustc_span::def_id::DefId;
use rustc_span::Symbol;

use rustc_data_structures::fx::FxHashMap;
use rustc_middle::mir::MirPass;
use rustc_middle::ty::print::with_no_trimmed_paths;
use std::fmt::{Debug, Formatter};
use std::rc::Rc;
use tracing::{debug, error, trace};

const RMC_STR: &'static str = "rmc";

/// A trait that represents an MIR pass that applies function call abstractions.
///
/// We may want to replace some function that is hard to reason about. Today we only support
/// behaviorally equivalent abstractions.
///
/// In order to support over-approximations and under-approximations we would need to provide a
/// better mechanism to report proof results to reflect that.
pub struct FnCallAbstractionPass {
    abstraction_ids: FxHashMap<AbstractionsEnum, DefId>,
    abstractions: Vec<Rc<dyn FnAbstraction>>,
}

impl<'tcx> MirPass<'tcx> for FnCallAbstractionPass {
    /// Pass implementation.
    fn run_pass(&self, tcx: TyCtxt<'tcx>, body: &mut Body<'tcx>) {
        debug!("Run pass {}", self.name());
        if !is_enabled(tcx) {
            debug!("Pass disabled. Not running RMC");
            return;
        }

        if is_rmc_crate(tcx) {
            debug!("Skip pass when compiling RMC crate.");
            return;
        }

        for bb in BasicBlock::new(0)..body.basic_blocks().next_index() {
            self.process_bb(tcx, body, bb);
        }
    }
}

impl FnCallAbstractionPass {
    pub fn new(tcx: TyCtxt<'tcx>) -> FnCallAbstractionPass {
        let abstraction_ids = get_rmc_definitions(tcx);
        Self {
            abstraction_ids: abstraction_ids.clone(),
            abstractions: vec![
                ptr_read(&abstraction_ids),
                ptr_write(&abstraction_ids),
                mem_swap(&abstraction_ids),
                mem_replace(&abstraction_ids),
            ],
        }
    }

    fn process_bb(&self, tcx: TyCtxt<'tcx>, body: &mut Body<'tcx>, bb: BasicBlock) -> bool {
        let bb_data = &body[bb];
        let terminator = bb_data.terminator();
        let mut changed = false;
        if let TerminatorKind::Call { ref func, .. } = terminator.kind {
            trace!(?func, "FnCall");
            if let Some(abs) = self.abstractions.iter().find(|item| item.matches(tcx, body, func)) {
                let terminator = body[bb].terminator.take().unwrap();
                if let Ok(new_terminator) = abs.handle(tcx, &self.abstraction_ids, body, terminator)
                {
                    body[bb].terminator = Some(new_terminator);
                    changed = true;
                } else {
                    bug!("Fail to apply abstraction {}", abs.name());
                }
            }
        }
        changed
    }
}

pub trait FnAbstraction {
    fn name(&self) -> &'static str;
    fn matches(&self, tcx: TyCtxt<'tcx>, body: &Body<'tcx>, func: &Operand<'tcx>) -> bool;
    fn handle(
        &self,
        tcx: TyCtxt<'tcx>,
        abstraction_ids: &FxHashMap<AbstractionsEnum, DefId>,
        body: &mut Body<'tcx>,
        terminator: Terminator<'tcx>,
    ) -> Result<Terminator<'tcx>, String>;
}

impl Debug for dyn FnAbstraction {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

#[derive(Debug)]
pub struct FnReplacement {
    original_fns: Vec<&'static str>,
    abs_id: DefId,
    name: &'static str,
}

impl FnAbstraction for FnReplacement {
    fn name(&self) -> &'static str {
        self.name
    }

    fn matches(&self, tcx: TyCtxt<'tcx>, body: &Body<'tcx>, func: &Operand<'tcx>) -> bool {
        if let ty::FnDef(ref def_id, _) = func.ty(body, tcx).kind() {
            let name = with_no_trimmed_paths(|| tcx.def_path_str(*def_id));
            return self.original_fns.iter().any(|orig| *orig == name.as_str());
        }
        return false;
    }

    fn handle(
        &self,
        tcx: TyCtxt<'tcx>,
        _: &FxHashMap<AbstractionsEnum, DefId>,
        body: &mut Body<'tcx>,
        terminator: Terminator<'tcx>,
    ) -> Result<Terminator<'tcx>, String> {
        if let TerminatorKind::Call {
            ref func,
            args,
            destination,
            cleanup,
            from_hir_call,
            fn_span,
        } = terminator.kind
        {
            if let ty::FnDef(_, subst) = func.ty(body, tcx).kind() {
                let fn_handle = Operand::function_handle(tcx, self.abs_id, subst, fn_span);
                let new_terminator = Terminator {
                    source_info: terminator.source_info,
                    kind: TerminatorKind::Call {
                        func: fn_handle,
                        args,
                        destination,
                        cleanup,
                        from_hir_call,
                        fn_span,
                    },
                };
                debug!(?func, "Replaced call");
                return Ok(new_terminator);
            }
        }
        Err(format!("Failed to replace function. Target abstraction: {:?}", self))
    }
}

#[inline(always)]
fn ptr_read(abstraction_ids: &FxHashMap<AbstractionsEnum, DefId>) -> Rc<dyn FnAbstraction> {
    Rc::new(FnReplacement {
        original_fns: vec![
            "core::ptr::read",
            "core::ptr::read_unaligned",
            "core::ptr::read_volatile",
            "std::ptr::read",
            "std::ptr::read_unaligned",
            "std::ptr::read_volatile",
        ],
        abs_id: *abstraction_ids.get(&AbstractionsEnum::PtrRead).unwrap(),
        name: "PtrRead",
    })
}

#[inline(always)]
fn mem_swap(abstraction_ids: &FxHashMap<AbstractionsEnum, DefId>) -> Rc<dyn FnAbstraction> {
    Rc::new(FnReplacement {
        original_fns: vec![
            "core::mem::swap",
            "std::mem::swap",
            "core::ptr::swap",
            "std::ptr::swap",
        ],
        abs_id: *abstraction_ids.get(&AbstractionsEnum::MemSwap).unwrap(),
        name: "MemSwap",
    })
}

#[inline(always)]
fn mem_replace(abstraction_ids: &FxHashMap<AbstractionsEnum, DefId>) -> Rc<dyn FnAbstraction> {
    Rc::new(FnReplacement {
        original_fns: vec!["core::mem::replace", "std::mem::replace"],
        abs_id: *abstraction_ids.get(&AbstractionsEnum::MemReplace).unwrap(),
        name: "MemReplace",
    })
}

#[inline(always)]
fn ptr_write(abstraction_ids: &FxHashMap<AbstractionsEnum, DefId>) -> Rc<dyn FnAbstraction> {
    Rc::new(FnReplacement {
        original_fns: vec![
            "core::ptr::write",
            "core::ptr::write_unaligned",
            "core::ptr::write_volatile",
            "std::ptr::write",
            "std::ptr::write_unaligned",
            "std::ptr::write_volatile",
        ],
        abs_id: *abstraction_ids.get(&AbstractionsEnum::PtrWrite).unwrap(),
        name: "PtrWrite",
    })
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Copy, Debug)]
pub enum AbstractionsEnum {
    Assumption,
    NonDet,
    PtrRead,
    PtrWrite,
    MemSwap,
    MemReplace,
}

impl AbstractionsEnum {
    /// Returns the symbol relative to the diagnostic item used to tag each method.
    pub fn attribute(self) -> Symbol {
        match self {
            AbstractionsEnum::Assumption => Symbol::intern("RmcAssume"),
            AbstractionsEnum::NonDet => Symbol::intern("RmcNonDet"),
            AbstractionsEnum::PtrRead => Symbol::intern("RmcPtrRead"),
            AbstractionsEnum::PtrWrite => Symbol::intern("RmcPtrWrite"),
            AbstractionsEnum::MemSwap => Symbol::intern("RmcMemSwap"),
            AbstractionsEnum::MemReplace => Symbol::intern("RmcMemReplace"),
        }
    }
}

/// This function extract the `DefId` for all abstractions supported by RMC.
/// It first finds rmc crate and then iterate over its definitions and map to each abstraction type.
fn get_rmc_definitions(tcx: TyCtxt<'tcx>) -> FxHashMap<AbstractionsEnum, DefId> {
    let mut defs = FxHashMap::<AbstractionsEnum, DefId>::default();
    if let Some(krate) = tcx.crates(()).iter().find(|k| tcx.crate_name(**k).to_string() == RMC_STR)
    {
        let diagnostics = tcx.diagnostic_items(*krate);
        let abstractions = [
            AbstractionsEnum::Assumption,
            AbstractionsEnum::NonDet,
            AbstractionsEnum::PtrRead,
            AbstractionsEnum::PtrWrite,
            AbstractionsEnum::MemSwap,
            AbstractionsEnum::MemReplace,
        ];
        for abs in abstractions {
            if let Some(item) = diagnostics.name_to_id.get(&abs.attribute()) {
                defs.insert(abs, *item);
            } else {
                bug!("Missing attribute: {:?}", abs);
            }
        }
    } else {
        bug!("Can not find RMC crate. Is RMC_LIB_PATH configured correctly?")
    }

    debug!(?defs, "Abstractions available");
    return defs;
}

/// Returns true if MIR inlining is enabled in the current compilation session.
#[inline]
fn is_enabled(tcx: TyCtxt<'_>) -> bool {
    tcx.sess.parse_sess.config.iter().any(|(s, _)| s == &Symbol::intern(RMC_STR))
}

/// Check whether the current crate being compiled is the RMC crate.
#[inline]
fn is_rmc_crate(tcx: TyCtxt<'_>) -> bool {
    tcx.crate_name(LOCAL_CRATE).to_string() == RMC_STR
}
