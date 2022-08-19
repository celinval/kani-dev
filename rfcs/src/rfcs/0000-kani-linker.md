Comparison between the size of the standard library files after executing symtab (`*.out` files) vs generating rlib 
files with `-Z always-encode-mir` (`*.rlib` files) for `x86_64-unknown-linux-gnu` (on a Ubuntu 18.04).

| File Type | Raw size | Compressed size |
|-----------|----------|-----------------|
| `*.out`   |   84M    |     15M         |
| `*.rlib`  |   95M    |     15M         |

These results were obtained running `std-lib-regression.sh` script with and without `-Z always-encode-mir`.