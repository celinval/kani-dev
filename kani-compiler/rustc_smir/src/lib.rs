#![feature(rustc_private)]
extern crate rustc_ast;
extern crate rustc_codegen_ssa;
extern crate rustc_data_structures;
extern crate rustc_driver;
extern crate rustc_errors;
extern crate rustc_hir;
extern crate rustc_index;
extern crate rustc_metadata;
extern crate rustc_middle;
extern crate rustc_session;
extern crate rustc_span;
extern crate rustc_target;

// TODO: Expand me.
pub use rustc_ast::ast;
pub use rustc_ast::ast::Mutability;
pub use rustc_ast::{Attribute, LitKind};
pub use rustc_codegen_ssa::back::archive::ArchiveBuilder;
pub use rustc_codegen_ssa::back::link::link_binary;
pub use rustc_codegen_ssa::back::metadata::DefaultMetadataLoader;
pub use rustc_codegen_ssa::traits::CodegenBackend;
pub use rustc_codegen_ssa::{CodegenResults, CrateInfo};
pub use rustc_data_structures::fx::FxHashMap;
pub use rustc_data_structures::owning_ref::OwningRef;
pub use rustc_data_structures::rustc_erase_owner;
pub use rustc_data_structures::sync::MetadataRef;
pub use rustc_data_structures::temp_dir::MaybeTempDir;
pub use rustc_driver::init_rustc_env_logger;
pub use rustc_driver::{Callbacks, RunCompiler};
pub use rustc_errors::ErrorGuaranteed;
pub use rustc_errors::FatalError;
pub use rustc_hir::def::Namespace;
pub use rustc_hir::definitions::DefPathDataName;
pub use rustc_index::vec::IndexVec;
pub use rustc_metadata::EncodedMetadata;
pub use rustc_middle::dep_graph::{WorkProduct, WorkProductId};
// TODO: Expand me.
pub use rustc_middle::mir;
pub use rustc_middle::mir::interpret::{
    read_target_uint, AllocId, Allocation, ConstValue, GlobalAlloc, Scalar,
};
pub use rustc_middle::mir::mono::CodegenUnitNameBuilder;
pub use rustc_middle::mir::mono::{CodegenUnit, MonoItem};
pub use rustc_middle::mir::Body;
pub use rustc_middle::mir::{
    AggregateKind, AssertKind, BasicBlock, BasicBlockData, BinOp, CastKind, Constant, ConstantKind,
    Field, HasLocalDecls, Local, NullOp, Operand, Place, ProjectionElem, Rvalue, Statement,
    StatementKind, SwitchTargets, Terminator, TerminatorKind, UnOp, VarDebugInfo,
    VarDebugInfoContents,
};
pub use rustc_middle::span_bug;
pub use rustc_middle::ty::adjustment::PointerCast;
pub use rustc_middle::ty::layout::LayoutOf;
pub use rustc_middle::ty::layout::{
    HasParamEnv, HasTyCtxt, LayoutError, LayoutOfHelpers, TyAndLayout,
};
pub use rustc_middle::ty::print::with_no_trimmed_paths;
pub use rustc_middle::ty::print::FmtPrinter;
pub use rustc_middle::ty::print::Printer;
pub use rustc_middle::ty::query::Providers;
pub use rustc_middle::ty::subst::InternalSubsts;
// TODO: Expand self.
pub use rustc_middle::ty::{
    self, AdtDef, Const, ConstKind, FloatTy, Instance, InstanceDef, IntTy, List, PolyFnSig, Ty,
    TyCtxt, TypeAndMut, TypeFoldable, Uint, UintTy, VariantDef, VtblEntry,
};
pub use rustc_session::config::{CrateType, OutputFilenames, OutputType};
pub use rustc_session::cstore::DllImport;
pub use rustc_session::cstore::MetadataLoader;
pub use rustc_session::cstore::MetadataLoaderDyn;
pub use rustc_session::Session;
pub use rustc_span::def_id::{DefId, LOCAL_CRATE};
pub use rustc_span::Span;
pub use rustc_span::Symbol;
pub use rustc_span::DUMMY_SP;
pub use rustc_target::abi::{
    Abi, Endian, FieldsShape, HasDataLayout, InitKind, Integer, Layout, Primitive, Size,
    TagEncoding, TargetDataLayout, VariantIdx, Variants,
};
pub use rustc_target::spec::abi::Abi as SpecAbi;
pub use rustc_target::spec::PanicStrategy;
pub use rustc_target::spec::Target;
