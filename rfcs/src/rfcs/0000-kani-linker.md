- **Feature Name:** `MIR` Linker (mir_linker)
- **Feature Request Issue**: https://github.com/model-checking/kani/issues/1213
- **RFC PR:** *TODO*
- **Status:** Draft

-------------------

## User Impact

The main goal of this RFC is to enable Kani users to link against all supported methods from the `std` library.
Currently, Kani will only link to methods that are either generic or have an inline annotation.

The approach introduced in this RFC will have the following secondary benefits.
- Reduce spurious warnings about unsupported features for cases where the feature is not reachable from any harness.
- In the verification mode, we will likely see a reduction on the compilation time and memory consumption
 by pruning the inputs of symtab2gb and goto-instrument.
  - Linking against the standard library goto-models takes more than 5GB which is not feasible with the current github workers.
- In a potential assessment mode, only analyze code that is reachable from all public methods in the target crate.

One downside is that our release bundle will be bigger.
We are going to include `rlib` files for the `std` and `kani` libraries.
This will negatively impact the time taken for Kani setup
(triggered by either the first time a user invokes Kani, or explicit calls to `[kani|cargo-kani] setup`). 

## User Experience

Once this RFC has been stabilized users shall use Kani in the same manner as they have been today.
Until then, we wil add an unstable option `--mir-linker` to enable the cross-crate reachability analysis
and the generation of the `goto-program` model only when compiling the target crate.

Kani setup will likely take a bit longer and more disk space as mentioned in the section above,
independently from opting to use the `MIR` linker via `--mir-linker` option above.

## Detailed Design

In a nutshell, we will no longer generate a `goto-program` model for every crate we compile.
Instead, we will generate the `MIR` for every crate, and we will generate only one `goto-program` model.
This model will only include items reachable from the target crate's harnesses.

The current system flow for a crate verification is the following:

1. `Kani` compiles the user crate as well as all its dependencies.
   For every crate compiled, `kani-compiler` will generate a `goto-program` model.
   This model includes everything reachable from the crate's public methods.
2. After that, `kani` link all models together by invoking `goto-cc`.
   This step will also link against Kani's `C` library.
3. For every harness, `kani` prunes the linked model to only include items reachable from the given harness.
4. Finally, `kani` instruments and verify each harness model via `goto-program` and `cbmc` calls.

After this RFC, the system flow would be slightly different:

1. `Kani` compiles the user crate dependencies up to the `MIR` translation.
   I.e., for every crate compiled, `kani-compiler` will generate an artifact that includes the `MIR` representation
  of all items in the crate.
2. `Kani` will generate the `goto-program` only while compiling the target user crate.
  It will generate one `goto-program` model that includes all items reachable from every harness in the target crate.
3. `goto-cc` will still be invoked to link the generated model against Kani's `C` library.
4. Steps #3 and #4 above will be performed without any change.

This feature will require three main changes to Kani which are detailed in the sub-sections below.

### Kani's Sysroot

Kani currently uses `rustup` sysroot to gather information from the standard library constructs.
The artifacts from this `sysroot` include the `MIR` for generic methods as well as for items that may be included in 
a crate compilation (e.g.: methods marked with `#[inline]` annotation).
The artifacts do not include the `MIR` for items that have already been compiled to the `std` shared library.
This leaves a gap that cannot be filled by the `kani-compiler`;
thus, we are unable to translate these items into `goto-program`.

In order to fulfill this gap, we must compile the standard library from scratch.
This RFC proposes a similar method to what [`MIRI`](https://github.com/rust-lang/miri) implements.
We will generate our own sysroot using the `-Z always-encode-mir` compilation flag.
This sysroot will be pre-compiled and included in our release bundle.

We will compile `kani`'s libraries (`kani` and `std`) also with `-Z always-encode-mir`
and with the new sysroot.


### Cross-Crate Reachability Analysis

`kani-compiler` will include a new `reachability` module to traverse over the local and external `MIR` items.
This module will `monomorphize` all generic code as it's performing the traversal.

The traversal logic will be customizable allowing different starting points to be used.
The two options to be included in this RFC is starting from all local harnesses
(tagged with `#[kani::proof]`) and all public methods in the local crate.

The `kani-compiler` behavior will be customizable via a new flag:

```--reachability=[crate | harnesses | pub_fns |  none]```

where:

 - `crate`: Keep `kani-compiler` current behavior by using 
   `rustc_monomorphizer::collect_and_partition_mono_items()` which respects the crate boundary.
   This will generate a `goto-program` for each crate compiled by `kani-compiler`, and it will still have the same `std` linking issues.
 - `harnesses`: Use the local harnesses as the starting points of the reachability analysis.
 - `pub_fns`: Use the public local functions as the starting points.
 - `none`: This will be the default value if `--reachability` flag is not provided. It will basically skip 
 reachability analysis (and `goto-program` generation).
   `kani-compiler` will still generate artifacts with the crate's `MIR`.


### Dependencies vs Target Crate Compilation




## Rational and Alternatives

### `MIR` Based Sysroot

Comparison between the size of the standard library files after executing symtab (`*.out` files) vs generating rlib
files with `-Z always-encode-mir` (`*.rlib` files) for `x86_64-unknown-linux-gnu` (on a Ubuntu 18.04).

| File Type | Raw size | Compressed size |
|-----------|----------|-----------------|
| `*.out`   |   84M    |     15M         |
| `*.rlib`  |   95M    |     15M         |

These results were obtained running `std-lib-regression.sh` script with and without `-Z always-encode-mir`.

### Risks

Failures in the linking stage would not impact the tool soundness. I anticipate the following failure scenarios:
- ICE (Internal compiler error): Some logic is incorrectly implemented and the linking stage crashes.
 Although this is a bad experience for the user, this will not impact the verification result.
- Missing items: This would either result in ICE during code generation or a verification failure if the missing
  item is reachable.
- Extra items: This shouldn't impact the verification results and it should be prunned by CBMC reachability analysis.
  This is already the case today. In extreme cases, this could include a symbol that we cannot compile and cause an ICE.

### Benefits

- We should be able to remove the `#[no_mangle]` from harnesses since we can now control the entry points.

## Open questions

- Should we build or download the sysroot during `kani setup`?
- What's the best way to enable support to run Kani in the entire `workspace`?
  - One possibility is to run `cargo rustc` per package.

## Future possibilities

- Split `goto-program` models into two or more items to optimize compilation result caching.
  - Dependencies: One model will include items from all the crate dependencies.
    This model will likely be more stable and require fewer updates.
  - Target crate: The model for all items in the target crate.
- Do the analysis per-harness. This might be adequate once we have a mechanism to cache translations.
- Add external methods to the analysis in order to enable verification when calls are made from `C` to `rust`.
- Contribute the reachability analysis code back to upstream.