BUNDLE_NAME := ReposeSpike
BUNDLE      := build/$(BUNDLE_NAME).bundle
BINARY      := $(BUNDLE)/Contents/MacOS/$(BUNDLE_NAME)
ARCHS       := -arch arm64 -arch x86_64
CFLAGS      := -Wall -Wextra -Werror -O1 -bundle $(ARCHS)
FRAMEWORKS  := -framework Security -framework CoreFoundation

# Daemon-backed "consume" mode links the permit client into the bundle. The
# client is plain BSD sockets + arc4random_buf on libSystem -- no extra
# frameworks -- so it cross-compiles under $(CFLAGS) with just an include path.
CLIENT_DIR  := ../permit-daemon
CLIENT_SRC  := $(CLIENT_DIR)/repose_permit_client.c
CLIENT_HDRS := $(CLIENT_DIR)/repose_permit_client.h $(CLIENT_DIR)/repose_permit_wire.h

TEST_BIN    := build/plugin_contract_test
# The test dlopens the bundle, so it is built for the host arch only.
TESTFLAGS   := -Wall -Wextra -Werror -O1

.PHONY: all clean verify test

all: $(BUNDLE)

$(BUNDLE): plugin.c $(CLIENT_SRC) $(CLIENT_HDRS) Info.plist
	rm -rf $(BUNDLE)
	mkdir -p $(BUNDLE)/Contents/MacOS
	cp Info.plist $(BUNDLE)/Contents/Info.plist
	clang $(CFLAGS) $(FRAMEWORKS) -I$(CLIENT_DIR) -o $(BINARY) plugin.c $(CLIENT_SRC)
	codesign --force --sign - --timestamp=none $(BUNDLE)

verify: $(BUNDLE)
	codesign --verify --verbose=4 $(BUNDLE)
	codesign -dvvv $(BUNDLE)
	lipo -info $(BINARY)
	nm -gU $(BINARY) | grep AuthorizationPluginCreate

# Proves the plugin's own logic before blaming macOS for a silent milestone A.
# Takes ~10s: one case deliberately waits out the permit timeout.
test: $(BUNDLE)
	clang $(TESTFLAGS) $(FRAMEWORKS) -o $(TEST_BIN) tests/plugin_contract_test.c
	./$(TEST_BIN) $(BINARY)
	@echo
	./tests/authdb_edit_test.sh
	@echo
	./tests/install_invariants_test.sh
	@echo
	./tests/healthcheck_test.sh

clean:
	rm -rf build
