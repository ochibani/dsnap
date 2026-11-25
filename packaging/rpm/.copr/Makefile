NAME := distro-snapper
REPO_ROOT := $(shell git -C . rev-parse --show-toplevel)
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

	git -C $(REPO_ROOT) archive --format=tar.gz \
		--prefix=$(NAME)-$(VERSION)/ \
		-o $(SOURCES)/$(NAME)-$(VERSION).tar.gz \
		HEAD

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
