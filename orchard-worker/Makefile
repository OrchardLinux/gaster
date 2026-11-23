.PHONY: all check clean

CARGO ?= cargo
PKG_CONFIG ?= pkg-config
OUT := $(CURDIR)/target/worker
SYSTEM := $(shell uname -s)

all:
	$(CARGO) run --locked --manifest-path Cargo.toml -- "$(OUT)"
ifeq ($(SYSTEM),Darwin)
	$(CC) -O2 -w -D_GNU_SOURCE $(CPPFLAGS) $(CFLAGS) -I"$(OUT)" worker.c \
		$(LDFLAGS) -framework CoreFoundation -framework IOKit -o "$(OUT)/gaster-worker"
else ifeq ($(SYSTEM),Linux)
	usb_flags=$$($(PKG_CONFIG) --cflags --libs libusb-1.0) && \
		$(CC) -O2 -w -D_GNU_SOURCE -DHAVE_LIBUSB $(CPPFLAGS) $(CFLAGS) -I"$(OUT)" worker.c \
		$(LDFLAGS) $$usb_flags -o "$(OUT)/gaster-worker"
else
	@echo 'The Orchard worker supports macOS and Linux.' >&2; exit 1
endif

check: all
	"$(OUT)/gaster-worker" --self-test

clean:
	$(RM) -r "$(OUT)"
