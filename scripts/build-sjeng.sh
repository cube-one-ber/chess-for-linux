#!/bin/sh
# Optional compatibility engine. The application itself builds entirely in Rust.
set -eu
cd "$(dirname "$0")/.."
mkdir -p target/sjeng/include/machine
cat > target/sjeng/include/machine/endian.h <<'HEADER'
#include <endian.h>
HEADER
cc -O2 -fcommon -std=gnu99 -Wno-implicit-function-declaration \
  -I target/sjeng/include -I sjeng \
  sjeng/attacks.c sjeng/crazy.c sjeng/epd.c sjeng/learn.c sjeng/partner.c \
  sjeng/seval.c sjeng/ttable.c sjeng/book.c sjeng/ecache.c sjeng/eval.c \
  sjeng/moves.c sjeng/search.c sjeng/sjeng.c sjeng/utils.c sjeng/newbook.c \
  sjeng/proof.c sjeng/neval.c sjeng/rcfile.c sjeng/leval.c sjeng/draw.c \
  sjeng/see.c sjeng/segtb.c -lm -lgdbm_compat -lgdbm -o target/sjeng/sjeng
printf 'Built optional original engine: %s/target/sjeng/sjeng\n' "$(pwd)"
