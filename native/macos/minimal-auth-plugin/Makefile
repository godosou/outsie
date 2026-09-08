BUNDLE_NAME := ReposeSpike
BUNDLE      := build/$(BUNDLE_NAME).bundle
BINARY      := $(BUNDLE)/Contents/MacOS/$(BUNDLE_NAME)
ARCHS       := -arch arm64 -arch x86_64
CFLAGS      := -Wall -Wextra -Werror -O1 -bundle $(ARCHS)
FRAMEWORKS  := -framework Security -framework CoreFoundation

TEST_BIN    := build/plugin_contract_test
# The test dlopens the bundle, so it is built for the host arch only.
TESTFLAGS   := -Wall -Wextra -Werror -O1

.PHONY: all clean verify test

all: $(BUNDLE)

$(BUNDLE): plugin.c Info.plist
	rm -rf $(BUNDLE)
	mkdir -p $(BUNDLE)/Contents/MacOS
	cp Info.plist $(BUNDLE)/Contents/Info.plist
	clang $(CFLAGS) $(FRAMEWORKS) -o $(BINARY) plugin.c
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

clean:
	rm -rf build
