# The everyday commands, with the two things that make them slow out of the way.
#
# 1. Every recipe runs in its own cgroup slice with a memory cap. A cargo build inside the desktop's own
#    scope is what once got the whole session killed by systemd-oomd; isolated, the build can be the one
#    that dies instead.
# 2. Plenty of parallelism is then safe: the cap, not a low job count, is what protects the machine.

SLICE = systemd-run --user --scope --slice=lognotes-build.slice -p MemoryHigh=6G -p MemoryMax=10G --
JOBS = CARGO_BUILD_JOBS=6
LIVETEST = QSG_RENDER_LOOP=basic $(JOBS) cargo run -p lognotes-qt --features driver -- --livetest

.PHONY: build release test livetest run

## Build the app (debug).
build:
	$(SLICE) bash -c 'cd $(CURDIR) && $(JOBS) cargo build -p lognotes-qt'

## Build the app (release).
release:
	$(SLICE) bash -c 'cd $(CURDIR) && $(JOBS) cargo build -p lognotes-qt --release'

## Every unit test, core and bridge.
test:
	$(SLICE) bash -c 'cd $(CURDIR) && $(JOBS) cargo test --workspace'

## The whole window, driven: typing, links, search, themes. Screenshots land in $TMPDIR/lognotes-livetest.
livetest:
	$(SLICE) bash -c 'cd $(CURDIR) && $(LIVETEST)'

## Run the app itself (needs a display; not isolated, because it is the thing you are looking at).
run:
	$(JOBS) cargo run -p lognotes-qt
