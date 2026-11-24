Name:           distro-snapper
Version:        0.1.0
Release:        1%{?dist}
Summary:        Manage immutable tree-shaped btrfs snapshots

License:        AGPL-3.0-only
URL:            https://github.com/ochibani/dsnap
Source0:        %{name}-%{version}.tar.gz
Source1:        packaging/systemd/dsnap-auto-upgrade.service
Source2:        packaging/systemd/dsnap-auto-upgrade.timer

BuildRequires:  gcc
BuildRequires:  make
BuildRequires:  pkgconf-pkg-config
BuildRequires:  openssl-devel
BuildRequires:  libcurl-devel
BuildRequires:  python3-devel
BuildRequires:  cargo
BuildRequires:  rust

Requires:       btrfs-progs
Requires:       curl
Requires:       efibootmgr
Requires:       gnupg2
Requires:       python3
Requires:       python3-anytree
Recommends:     systemd

%global _description %{expand:
dsnap manages tree-shaped btrfs snapshot layouts, letting a system be
deployed, restored and exported as immutable snapshot trees.}

%description %{_description}

%prep
%autosetup -n %{name}-%{version}

%build
export PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1
cargo build --release --locked --no-default-features --features "base-import,btrfs,grub,import,lock,dnf"

%install
install -d -m755 %{buildroot}%{_sysconfdir}/dsnap/export-keys
install -d -m755 %{buildroot}%{_sysconfdir}/dsnap/profiles.d
install -d -m755 %{buildroot}%{_sysconfdir}/dsnap/profile-keys
install -Dm755 target/release/dsnap %{buildroot}%{_bindir}/dsnap
install -Dm644 %{SOURCE1} %{buildroot}%{_unitdir}/dsnap-auto-upgrade.service
install -Dm644 %{SOURCE2} %{buildroot}%{_unitdir}/dsnap-auto-upgrade.timer
install -Dm644 target/release/build/dsnap-*/out/dsnap.8 %{buildroot}%{_mandir}/man8/dsnap.8
install -m644 target/release/build/dsnap-*/out/dsnap-*.8 %{buildroot}%{_mandir}/man8/
install -Dm644 target/release/build/dsnap-*/out/dsnap.bash %{buildroot}%{_datadir}/bash-completion/completions/dsnap
install -Dm644 target/release/build/dsnap-*/out/_dsnap %{buildroot}%{_datadir}/zsh/site-functions/_dsnap
install -Dm644 target/release/build/dsnap-*/out/dsnap.fish %{buildroot}%{_datadir}/fish/vendor_completions.d/dsnap.fish

%files
%license LICENSE
%{_bindir}/dsnap
%dir %attr(0755,root,root) %{_sysconfdir}/dsnap/export-keys
%dir %attr(0755,root,root) %{_sysconfdir}/dsnap/profiles.d
%dir %attr(0755,root,root) %{_sysconfdir}/dsnap/profile-keys
%{_mandir}/man8/dsnap*.8*
%{_unitdir}/dsnap-auto-upgrade.service
%{_unitdir}/dsnap-auto-upgrade.timer
%{_datadir}/bash-completion/completions/dsnap
%{_datadir}/zsh/site-functions/_dsnap
%{_datadir}/fish/vendor_completions.d/dsnap.fish

%postun
# The timer is shipped disabled by default; never enable it here.
%systemd_postun dsnap-auto-upgrade.service dsnap-auto-upgrade.timer

%changelog
* Sun Nov 15 2026 Chibani <11yzyv86j@relay.firefox.com> - 0.1.0-1
- Initial package
