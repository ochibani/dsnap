NAME := distro-snapper
VERSION := $(shell awk '/^Version:/{print $$2}' packaging/rpm/distro-snapper.spec)
TOPDIR := $(CURDIR)/.rpmbuild
SOURCES := $(TOPDIR)/SOURCES
SPECS := $(TOPDIR)/SPECS
SRPMS := $(TOPDIR)/SRPMS

.PHONY: srpm

srpm:
	mkdir -p $(SOURCES) $(SPECS) $(SRPMS)

	git archive --format=tar.gz \
		--prefix=$(NAME)-$(VERSION)/ \
		-o $(SOURCES)/$(NAME)-$(VERSION).tar.gz \
		HEAD

	mkdir -p $(SOURCES)/packaging/systemd
	cp packaging/systemd/dsnap-auto-upgrade.service $(SOURCES)/packaging/systemd/
	cp packaging/systemd/dsnap-auto-upgrade.timer $(SOURCES)/packaging/systemd/

	cp packaging/rpm/$(NAME).spec $(SPECS)/

	rpmbuild -bs \
		--define "_topdir $(TOPDIR)" \
		--define "_sourcedir $(SOURCES)" \
		--define "_specdir $(SPECS)" \
		--define "_srcrpmdir $(SRPMS)" \
		$(SPECS)/$(NAME).spec

	mv $(SRPMS)/$(NAME)-$(VERSION)-*.src.rpm ./
