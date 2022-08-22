## User Impact

The main goal of this RFC is to enable Kani users to link against all supported methods from the `std` library.
Currently, Kani will only link to methods that are either generic or have an inline annotation.

Additionally, the approach introduced in this RFC has the following benefits.
- In the verification mode, this will reduce compilation time and memory consumption by only generating goto-program 
  for code that is reachable from the user proof harness.
- In a potential assessment mode, only generate goto-program for that that is reachable from all public methods in the 
 user crate.

## User Experience

I don't expect any change on the user flow while using Kani. User's  

## Release changes

Comparison between the size of the standard library files after executing symtab (`*.out` files) vs generating rlib
files with `-Z always-encode-mir` (`*.rlib` files) for `x86_64-unknown-linux-gnu` (on a Ubuntu 18.04).

| File Type | Raw size | Compressed size |
|-----------|----------|-----------------|
| `*.out`   |   84M    |     15M         |
| `*.rlib`  |   95M    |     15M         |

These results were obtained running `std-lib-regression.sh` script with and without `-Z always-encode-mir`.

## Risks

Failures in the linking stage would not impact the tool soundness. I anticipate the following failure scenarios:
- ICE (Internal compiler error): Some logic is incorrectly implemented and the linking stage crashes.
 Although this is a bad experience for the user, this will not impact the verification result.
- Missing items: This would either result in ICE during code generation or a verification failure if the missing
  item is reachable.
- Extra items: This shouldn't impact the verification results and it should be prunned by CBMC reachability analysis.
  This is already the case today. In extreme cases, this could include a symbol that we cannot compile and cause an ICE.

## Benefits

- We should be able to remove the `#[no_mangle]` from harnesses since we can now control the entry points.