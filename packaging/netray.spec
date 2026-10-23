Name:           netray
Version:        %{netray_version}
Release:        %{netray_release}%{?dist}
Summary:        Network tunnel client
License:        LicenseRef-Netray AND MPL-2.0
URL:            https://github.com/venujatech-byte/Netray
Source0:        netray-payload.tar.gz
BuildArch:      %{_arch}
%global debug_package %{nil}
# Keep the verified upstream Xray binary byte-for-byte intact.
%global __os_install_post %{nil}

%description
Netray desktop client with the Xray-core proxy engine bundled for Linux.

%prep
mkdir -p payload
tar -xzf %{SOURCE0} -C payload

%build

%install
mkdir -p %{buildroot}
cp -a payload/. %{buildroot}/

%files
%{_bindir}/netray
%{_datadir}/applications/netray.desktop
%{_datadir}/icons/hicolor/scalable/apps/netray.svg
/opt/netray/netray
/opt/netray/xray
/opt/netray/geoip.dat
/opt/netray/geosite.dat
%license /opt/netray/LICENSE
%license /usr/share/licenses/netray/LICENSE-Netray.txt
%doc /opt/netray/README.md
%doc /opt/netray/THIRD-PARTY-NOTICES.txt
