Name:           dsnap
Version:        0.1.0
Release:        1%{?dist}
Summary:        Immutable tree-shaped meta-distribution using snapshots

License:        AGPL-3.0-only
URL:            https://github.com/ochibani/dsnap
Source0:        %{name}-%{version}.tar.gz
Source1:        packaging/systemd/dsnap.service
Source2:        packaging/systemd/dsnap.timer

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
Requires:       python3
Recommends:     systemd

%global _description %{expand:
dsnap manages tree-shaped btrfs snapshot layouts, letting a system be
deployed, restored and exported as immutable snapshot trees. Requires the
python anytree module at runtime (pip install anytree).}

%description %{_description}

%prep
%autosetup -n %{name}-%{version}

%build
export PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1
cargo build --release --locked

%install
install -Dm755 target/release/dsnap %{buildroot}%{_bindir}/dsnap
install -Dm644 %{SOURCE1} %{buildroot}%{_unitdir}/dsnap.service
install -Dm644 %{SOURCE2} %{buildroot}%{_unitdir}/dsnap.timer
install -Dm644 target/release/build/dsnap-*/out/dsnap.bash %{buildroot}%{_datadir}/bash-completion/completions/dsnap
install -Dm644 target/release/build/dsnap-*/out/_dsnap %{buildroot}%{_datadir}/zsh/site-functions/_dsnap
install -Dm644 target/release/build/dsnap-*/out/dsnap.fish %{buildroot}%{_datadir}/fish/vendor_completions.d/dsnap.fish

%files
%license LICENSE
%{_bindir}/dsnap
%{_unitdir}/dsnap.service
%{_unitdir}/dsnap.timer
%{_datadir}/bash-completion/completions/dsnap
%{_datadir}/zsh/site-functions/_dsnap
%{_datadir}/fish/vendor_completions.d/dsnap.fish

%postun
# The timer is shipped disabled by default; never enable it here.
%systemd_postun dsnap.service dsnap.timer

%changelog
* Sun Nov 15 2026 Chibani <11yzyv86j@relay.firefox.com> - 0.1.0-1
- Initial package (renamed from ash to dsnap)
