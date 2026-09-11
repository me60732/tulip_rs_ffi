CC ?= cc
# -march=native/-ffast-math match the flags used to build the vendored
# reference libraries below (tulip_test/build.rs), so the harness itself
# isn't put at an unfair disadvantage relative to C_tulip/talib.
CFLAGS ?= -O3 -march=native -ffast-math -Wall -Wextra -Wno-format-truncation
LIB_DIR := ../../target/release
BIN := tulip_rs_ffi_bench
SRCS ?= bench.c

# tulip_rs_ffi's own C header (regenerate with cbindgen, see repo README).
FFI_INC := ../../include

# Reference implementations for the C_tulip / talib comparisons, built from
# the same git submodules the core Rust criterion benches use
# (../tulip_rs/tulip_test/{tulip_indicators,ta_lib_src}) -- see README.md.
TULIP_TEST_DIR := ../../../tulip_rs/tulip_test
TULIP_C_SRC    := $(TULIP_TEST_DIR)/tulip_indicators/tiamalgamation.c
TULIP_C_INC    := $(TULIP_TEST_DIR)/tulip_indicators
TALIB_DEB      := $(TULIP_TEST_DIR)/ta_lib_src/dist/ta-lib_0.7.1_amd64.deb
VENDOR_DIR     := vendor
TALIB_VENDOR   := $(VENDOR_DIR)/ta-lib
TALIB_INC      := $(TALIB_VENDOR)/usr/include/ta-lib
TALIB_LIB      := $(TALIB_VENDOR)/usr/lib

.PHONY: all clean run vendor

all: $(BIN)

# Tulip Indicators: compiled straight from the single-file amalgamation with
# the same flags as tulip_test/build.rs.
$(VENDOR_DIR)/libtulipc.a: $(TULIP_C_SRC)
	mkdir -p $(VENDOR_DIR)
	$(CC) -O3 -march=native -ffast-math -I$(TULIP_C_INC) -c $(TULIP_C_SRC) -o $(VENDOR_DIR)/tiamalgamation.o
	ar rcs $@ $(VENDOR_DIR)/tiamalgamation.o

# TA-Lib: extracted from the prebuilt .deb already vendored in the ta_lib_src
# submodule (its current upstream layout has no buildable src/ta_func tree in
# this checkout -- see README.md "Reference implementations").
$(TALIB_LIB)/libta-lib.a: $(TALIB_DEB)
	mkdir -p $(TALIB_VENDOR)
	dpkg-deb -x $(TALIB_DEB) $(TALIB_VENDOR)

# libpq: locate the system libpq.so.5 and copy it into vendor/ as libpq.so
# for static linking (no libpq-dev package available in this environment).
$(VENDOR_DIR)/libpq.so:
	mkdir -p $(VENDOR_DIR)
	@LIBPQ_PATH=$$(find /usr/lib -name 'libpq.so.5' 2>/dev/null | head -1); \
	if [ -z "$$LIBPQ_PATH" ]; then LIBPQ_PATH="/usr/lib/x86_64-linux-gnu/libpq.so.5"; fi; \
	if [ -f "$$LIBPQ_PATH" ]; then \
		cp "$$LIBPQ_PATH" $(VENDOR_DIR)/libpq.so; \
		echo "  vendored libpq from $$LIBPQ_PATH"; \
	else \
		echo "[error] libpq.so.5 not found at $$LIBPQ_PATH"; \
		exit 1; \
	fi

vendor: $(VENDOR_DIR)/libtulipc.a $(TALIB_LIB)/libta-lib.a $(VENDOR_DIR)/libpq.so

$(BIN): $(SRCS) vendor $(wildcard bench_indicators/*.c)
	$(CC) $(CFLAGS) -I$(TULIP_C_INC) -I$(TALIB_INC) -I$(FFI_INC) -o $(BIN) $(SRCS) \
		-L$(LIB_DIR) -ltulip_rs_ffi \
		$(VENDOR_DIR)/libtulipc.a \
		$(TALIB_LIB)/libta-lib.a \
		$(VENDOR_DIR)/libpq.so \
		-lm \
		-Wl,-rpath,$(LIB_DIR)

run: $(BIN)
	./$(BIN)

clean:
	rm -f $(BIN)
	rm -rf $(VENDOR_DIR)
