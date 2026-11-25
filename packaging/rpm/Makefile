NAME := distro-snapper
REPO_ROOT := $(abspath ../..)
VERSION := $(shell awk '/^Version:/{print $$2}' $(NAME).spec)
TOPDIR := $(REPO_ROOT)/.rpmbuild
SOURCES := $(TOPDIR)/SOURCES
SPECS := $(TOPDIR)/SPECS
SRPMS := $(TOPDIR)/SRPMS

.PHONY: srpm

srpm:
	mkdir -p $(SOURCES) $(SPECS) $(SRPMS)

	git -C $(REPO_ROOT) archive --format=tar.gz \
		--prefix=$(NAME)-$(VERSION)/ \
		-o $(SOURCES)/$(NAME)-$(VERSION).tar.gz \
		HEAD

	mkdir -p $(SOURCES)/packaging/systemd
	cp $(REPO_ROOT)/packaging/systemd/dsnap-auto-upgrade.service $(SOURCES)/packaging/systemd/
	cp $(REPO_ROOT)/packaging/systemd/dsnap-auto-upgrade.timer $(SOURCES)/packaging/systemd/

	cp $(NAME).spec $(SPECS)/

	rpmbuild -bs \
		--define "_topdir $(TOPDIR)" \
		--define "_sourcedir $(SOURCES)" \
		--define "_specdir $(SPECS)" \
		--define "_srcrpmdir $(SRPMS)" \
		$(SPECS)/$(NAME).spec

	mv $(SRPMS)/$(NAME)-$(VERSION)-*.src.rpm ./
