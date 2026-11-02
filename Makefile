.PHONY: clean clean_all demo refresh_extension_version

PROJ_DIR := $(dir $(abspath $(lastword $(MAKEFILE_LIST))))

EXTENSION_NAME=units

# Mark the extension as C_STRUCT_UNSTABLE: DuckDB 2.x requires the _init_c_api_v2 entrypoint.
# Binaries load only in the exact TARGET_DUCKDB_VERSION, not in newer DuckDB releases.
USE_UNSTABLE_C_API=1
CAPI_VERSION=2

# Target DuckDB version
TARGET_DUCKDB_VERSION=v2.0.0
# The unstable API requires the test runner and binary to match the target.
DUCKDB_TEST_VERSION ?= $(patsubst v%,%,$(TARGET_DUCKDB_VERSION))

all: configure debug

# Include makefiles from DuckDB
include extension-ci-tools/makefiles/c_api_extensions/base.Makefile
include extension-ci-tools/makefiles/c_api_extensions/rust.Makefile

# Build workspace libraries rather than the CI tools' Wasm example target.
IS_EXAMPLE=
ifneq ($(DUCKDB_WASM_PLATFORM),)
	TARGET_INFO=--target $(TARGET) --package units_wasm
else
	TARGET_INFO+=--package units
endif

configure: venv platform extension_version

# CI tools cache configure/extension_version.txt. Refresh it before packaging,
# so local rebuilds follow HEAD and respect changes to EXTENSION_VERSION.
refresh_extension_version: check_configure
	@$(VERSION_COMMAND)

build_extension_with_metadata_debug build_extension_with_metadata_release: refresh_extension_version

debug: build_extension_library_debug build_extension_with_metadata_debug
release: build_extension_library_release build_extension_with_metadata_release

test: test_debug
test_debug: test_extension_debug
test_release: test_extension_release

demo: release
	./scripts/demo.sh

clean: clean_build clean_rust
clean_all: clean_configure clean
