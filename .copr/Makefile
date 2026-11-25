NAME := distro-snapper
THIS_MAKEFILE := $(abspath $(lastword $(MAKEFILE_LIST)))
REPO_ROOT := $(abspath $(dir $(THIS_MAKEFILE))/..)
SPEC_DEFAULT := $(REPO_ROOT)/packaging/rpm/$(NAME).spec
SPEC := $(if $(spec),$(spec),$(SPEC_DEFAULT))
VERSION := $(shell awk '/^Version:/{print $$2}' $(SPEC))
BUILDROOT := $(shell mktemp -d)
SOURCES := $(BUILDROOT)/SOURCES
SPECS := $(BUILDROOT)/SPECS

ifndef outdir
$(error outdir is required)
endif

.PHONY: srpm

srpm:
	mkdir -p $(outdir) $(SOURCES) $(SPECS)

	tar -C $(REPO_ROOT) \
		--transform='s,^\./,$(NAME)-$(VERSION)/,' \
		--exclude='./.git' \
		--exclude='./.rpmbuild' \
		--exclude='./target' \
		--exclude='*.src.rpm' \
		-czf $(SOURCES)/$(NAME)-$(VERSION).tar.gz \
		.

	mkdir -p $(SOURCES)/packaging/systemd
	cp $(REPO_ROOT)/packaging/systemd/dsnap-auto-upgrade.service $(SOURCES)/packaging/systemd/
	cp $(REPO_ROOT)/packaging/systemd/dsnap-auto-upgrade.timer $(SOURCES)/packaging/systemd/

	cp $(SPEC) $(SPECS)/$(NAME).spec

	rpmbuild -bs \
		--define "_topdir $(BUILDROOT)" \
		--define "_sourcedir $(SOURCES)" \
		--define "_specdir $(SPECS)" \
		--define "_srcrpmdir $(outdir)" \
		$(SPECS)/$(NAME).spec
