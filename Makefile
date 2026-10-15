APP_NAME   := ExplainIt
VERSION    := $(shell sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
APP        := dist/$(APP_NAME).app
APPS_DIR   ?= $(HOME)/Applications
BIN_DIR    ?= $(HOME)/.local/bin
# A stable signing identity keeps the microphone permission across rebuilds. Picked automatically:
# Developer ID Application, Apple Development, "ExplainIt Dev" (`make cert`), else ad hoc ("-").
SIGN_ID    ?= $(shell scripts/signing-identity.sh)
# Hardened runtime everywhere; the app's entitlements allow the microphone.
CODESIGN   := codesign --force --options runtime --timestamp=none --sign "$(SIGN_ID)"
FEATURES   ?=
LSREGISTER := /System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
CARGO_FEATURES := $(if $(FEATURES),--features explainit-app/$(FEATURES))

FIXTURES   := tests/fixtures/en_bug.wav tests/fixtures/ro_bug.wav tests/fixtures/screenshot.png

.PHONY: build test test-speech bundle install uninstall e2e cert fixtures clean

build:
	cargo build --release --workspace $(CARGO_FEATURES)

test:
	cargo test --workspace

# Transcription and model-download tests; need the speech model (and network for the download test).
test-speech: fixtures
	cargo test --release -p explainit-app -- --ignored --nocapture

bundle: build
	rm -rf $(APP)
	mkdir -p $(APP)/Contents/MacOS $(APP)/Contents/Resources
	sed 's/__VERSION__/$(VERSION)/g' packaging/Info.plist > $(APP)/Contents/Info.plist
	cp target/release/explainit-app $(APP)/Contents/MacOS/$(APP_NAME)
	$(CODESIGN) --entitlements packaging/ExplainIt.entitlements $(APP)
	cp target/release/explainit dist/explainit
	$(CODESIGN) dist/explainit
	@echo "Bundled $(APP) and dist/explainit (signed: $(SIGN_ID))"

install: bundle
	-@test -x $(BIN_DIR)/explainit && $(BIN_DIR)/explainit quit >/dev/null 2>&1; sleep 1
	-@pkill -x $(APP_NAME) 2>/dev/null; true
	mkdir -p $(APPS_DIR) $(BIN_DIR)
	rm -rf $(APPS_DIR)/$(APP_NAME).app
	ditto $(APP) $(APPS_DIR)/$(APP_NAME).app
	-@$(LSREGISTER) -u $(CURDIR)/$(APP) 2>/dev/null
	$(LSREGISTER) -f $(APPS_DIR)/$(APP_NAME).app
	install -m 755 dist/explainit $(BIN_DIR)/explainit
	@echo "Installed $(APPS_DIR)/$(APP_NAME).app and $(BIN_DIR)/explainit"

uninstall:
	-@test -x $(BIN_DIR)/explainit && $(BIN_DIR)/explainit quit >/dev/null 2>&1; sleep 1
	-@pkill -x $(APP_NAME) 2>/dev/null; true
	-$(LSREGISTER) -u $(APPS_DIR)/$(APP_NAME).app
	rm -rf $(APPS_DIR)/$(APP_NAME).app $(BIN_DIR)/explainit
	@echo "Removed. Settings and models remain in ~/Library/Application Support/$(APP_NAME)."

# End-to-end test against an installed build with the dev hooks (feeds WAV fixtures instead of the mic).
e2e: fixtures
	$(MAKE) install FEATURES=dev-hooks
	scripts/e2e.sh

# Without an Apple certificate: creates a self-signed "ExplainIt Dev" identity (asks for your password once).
cert:
	scripts/make-signing-cert.sh

fixtures: $(FIXTURES)

$(FIXTURES):
	scripts/make-fixtures.sh

clean:
	cargo clean
	rm -rf dist
