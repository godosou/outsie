BUNDLE_NAME := ReposeSpike
BUNDLE      := build/$(BUNDLE_NAME).bundle
BINARY      := $(BUNDLE)/Contents/MacOS/$(BUNDLE_NAME)
ARCHS       := -arch arm64 -arch x86_64
CFLAGS      := -Wall -Wextra -Werror -O1 -bundle $(ARCHS)
FRAMEWORKS  := -framework Security -framework CoreFoundation

.PHONY: all clean verify

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

clean:
	rm -rf build
