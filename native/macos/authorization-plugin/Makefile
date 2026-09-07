SDKROOT ?= $(shell xcrun --sdk macosx --show-sdk-path)
CC := $(shell xcrun --sdk macosx --find clang)
BASE_BUILD_DIR ?= build
empty :=
comma := ,
ifneq ($(strip $(SANITIZE)),)
SANITIZE_TAG := $(subst $(comma),-,$(strip $(SANITIZE)))
BUILD_DIR ?= $(BASE_BUILD_DIR)/sanitize-$(SANITIZE_TAG)
SANITIZE_FLAGS := -fsanitize=$(strip $(SANITIZE)) -fno-omit-frame-pointer
else
BUILD_DIR ?= $(BASE_BUILD_DIR)
SANITIZE_FLAGS :=
endif

ARCH ?= arm64
CONFIGURATION ?= debug
ifeq ($(CONFIGURATION),debug)
BUNDLE_OPT_FLAGS := -O0 -g
else ifeq ($(CONFIGURATION),release)
BUNDLE_OPT_FLAGS := -O2
else
$(error CONFIGURATION must be debug or release)
endif

COMMON_CFLAGS := -std=c11 -Wall -Wextra -Werror -Wpedantic -isysroot $(SDKROOT) \
	-mmacosx-version-min=14.0 -pthread
TEST_CFLAGS := $(COMMON_CFLAGS) -DREPOSE_UNLOCK_TESTING=1 -O1 -g \
	$(SANITIZE_FLAGS)
FRAMEWORKS := -framework Security -framework SystemConfiguration \
	-framework CoreFoundation -lbsm -pthread
TEST_LDFLAGS := $(FRAMEWORKS) $(SANITIZE_FLAGS)
IPC_HEADER_DIR := ../../../src-tauri/crates/repose-unlock-ipc/include
IPC_PROTOCOL_HEADER := $(IPC_HEADER_DIR)/repose_unlock_ipc.h
IPC_TEST_SOURCES := src/ipc_client.c src/deadline.c tests/test_server.c \
	tests/ipc_client_test.c
PLUGIN_TEST_SOURCES := src/plugin.c src/ipc_client.c src/deadline.c \
	tests/test_server.c tests/fake_authorization_engine.c \
	tests/plugin_lifecycle_test.c
RELEASE_SOURCES := src/plugin.c src/ipc_client.c src/deadline.c
RELEASE_OBJECT_DIR := $(BUILD_DIR)/release
RELEASE_OBJECTS := $(RELEASE_OBJECT_DIR)/plugin.o \
	$(RELEASE_OBJECT_DIR)/ipc_client.o $(RELEASE_OBJECT_DIR)/deadline.o

BUNDLE_OUTPUT_DIR ?= ../../../target/macos-auth/$(CONFIGURATION)
BUNDLE_ROOT := $(BUNDLE_OUTPUT_DIR)/ReposeUnlock.bundle
BUNDLE_EXECUTABLE := $(BUNDLE_ROOT)/Contents/MacOS/ReposeUnlock
BUNDLE_INFO := $(BUNDLE_ROOT)/Contents/Info.plist
BUNDLE_CFLAGS := $(COMMON_CFLAGS) $(BUNDLE_OPT_FLAGS) -arch $(ARCH) \
	-fvisibility=hidden
BUNDLE_LDFLAGS := -bundle -arch $(ARCH) \
	-Wl,-exported_symbol,_AuthorizationPluginCreate $(FRAMEWORKS)

.PHONY: all test release-boundary bundle bundle-smoke clean

all: test

test: release-boundary $(BUILD_DIR)/ipc_client_test \
		$(BUILD_DIR)/plugin_lifecycle_test
	$(BUILD_DIR)/ipc_client_test
	$(BUILD_DIR)/plugin_lifecycle_test

release-boundary: $(BUILD_DIR)/release_api_compile.o $(RELEASE_OBJECTS)
	@if nm -gjU $(RELEASE_OBJECTS) | grep -E 'repose_(plugin|ipc)_test' >/dev/null; then \
		echo "production objects expose a test seam" >&2; exit 1; \
	fi

$(BUILD_DIR)/ipc_client_test: $(IPC_TEST_SOURCES) src/ipc_client.h src/deadline.h \
		tests/test_server.h $(IPC_PROTOCOL_HEADER)
	@mkdir -p $(BUILD_DIR)
	$(CC) $(TEST_CFLAGS) -Isrc -I$(IPC_HEADER_DIR) $(IPC_TEST_SOURCES) \
		$(TEST_LDFLAGS) -o $@

$(BUILD_DIR)/release_api_compile.o: tests/release_api_compile.c \
		src/ipc_client.h src/deadline.h $(IPC_PROTOCOL_HEADER)
	@mkdir -p $(BUILD_DIR)
	$(CC) $(COMMON_CFLAGS) -Isrc -I$(IPC_HEADER_DIR) -c $< -o $@

$(RELEASE_OBJECT_DIR)/plugin.o: src/plugin.c src/ipc_client.h src/deadline.h \
		$(IPC_PROTOCOL_HEADER)
	@mkdir -p $(RELEASE_OBJECT_DIR)
	$(CC) $(COMMON_CFLAGS) -fvisibility=hidden -Isrc -I$(IPC_HEADER_DIR) \
		-c $< -o $@

$(RELEASE_OBJECT_DIR)/ipc_client.o: src/ipc_client.c src/ipc_client.h \
		src/deadline.h $(IPC_PROTOCOL_HEADER)
	@mkdir -p $(RELEASE_OBJECT_DIR)
	$(CC) $(COMMON_CFLAGS) -fvisibility=hidden -Isrc -I$(IPC_HEADER_DIR) \
		-c $< -o $@

$(RELEASE_OBJECT_DIR)/deadline.o: src/deadline.c src/deadline.h
	@mkdir -p $(RELEASE_OBJECT_DIR)
	$(CC) $(COMMON_CFLAGS) -fvisibility=hidden -Isrc -I$(IPC_HEADER_DIR) \
		-c $< -o $@

$(BUILD_DIR)/plugin_lifecycle_test: $(PLUGIN_TEST_SOURCES) \
		src/plugin_test_support.h src/ipc_client.h src/deadline.h \
		tests/fake_authorization_engine.h tests/test_server.h \
		$(IPC_PROTOCOL_HEADER)
	@mkdir -p $(BUILD_DIR)
	$(CC) $(TEST_CFLAGS) -Isrc -I$(IPC_HEADER_DIR) $(PLUGIN_TEST_SOURCES) \
		$(TEST_LDFLAGS) -o $@

bundle: $(BUNDLE_EXECUTABLE) $(BUNDLE_INFO)

$(BUNDLE_EXECUTABLE): $(RELEASE_SOURCES) src/ipc_client.h src/deadline.h \
		$(IPC_PROTOCOL_HEADER)
	@mkdir -p $(@D)
	$(CC) $(BUNDLE_CFLAGS) -Isrc -I$(IPC_HEADER_DIR) $(RELEASE_SOURCES) \
		$(BUNDLE_LDFLAGS) -o $@
	@if [ -d "$@.dSYM" ]; then \
		rm -rf "$(BUNDLE_OUTPUT_DIR)/ReposeUnlock.bundle.dSYM"; \
		mv "$@.dSYM" "$(BUNDLE_OUTPUT_DIR)/ReposeUnlock.bundle.dSYM"; \
	fi

$(BUNDLE_INFO): Info.plist
	@mkdir -p $(@D)
	cp Info.plist $@

$(BUILD_DIR)/bundle_smoke: tests/bundle_smoke.c
	@mkdir -p $(BUILD_DIR)
	$(CC) $(COMMON_CFLAGS) -O1 -g $< -framework Security -o $@

bundle-smoke: bundle $(BUILD_DIR)/bundle_smoke
	$(BUILD_DIR)/bundle_smoke $(BUNDLE_EXECUTABLE)

clean:
	rm -rf build
