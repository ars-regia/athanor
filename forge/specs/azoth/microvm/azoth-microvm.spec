# Guest kernel for the Athanor OS MicroVMs (docs/architecture/doc_kernel_build.md,
# section 9). Same source and same pin as the main kernel, second config: it does not go
# through the Fedora packaging and produces vmlinux (what Firecracker and cloud-hypervisor
# load), bzImage and the config in /usr/lib/athanor/microvm/. build.sh passes the tree
# prepared by `rpmbuild -bp` of kernel.spec and the already configured object directory
# (x86_64_defconfig + kvm_guest.config + microvm/kernel-local): only the compilation with
# O= happens here, so the tree stays clean for the main kernel.
# No automatic strip or debuginfo: vmlinux is stripped of its DWARF by hand, keeping the
# symbols and .BTF.
%global debug_package %{nil}
%global __os_install_post %{nil}
%global microvm_dir /usr/lib/athanor/microvm

Name:           azoth-microvm
Version:        %{kversion}
Release:        %{krelease}
Summary:        Athanor OS guest kernel for the MicroVMs
License:        GPL-2.0-only WITH Linux-syscall-note
URL:            https://github.com/ars-regia/athanor
ExclusiveArch:  x86_64
# The toolchain of the main kernel (builder/Containerfile): clang and lld, pahole for the BTF.
BuildRequires:  clang lld llvm make dwarves

%description
The kernel that hypervisor-daemon boots in the MicroVMs (Firecracker, cloud-hypervisor):
x86_64_defconfig + kvm_guest.config + microvm/kernel-local (virtio, 9p and virtiofs,
EROFS, dm-verity, BPF with BTF; no physical drivers, no modules), compiled with clang
and kCFI from the same source as the main kernel.

%build
make -C %{kernel_tree} O=%{objdir} %{make_opts} -j%{_smp_build_ncpus} vmlinux bzImage

%install
install -d %{buildroot}%{microvm_dir}
llvm-strip --strip-debug -o %{buildroot}%{microvm_dir}/vmlinux %{objdir}/vmlinux
install -m 644 %{objdir}/arch/x86/boot/bzImage %{buildroot}%{microvm_dir}/bzImage
install -m 644 %{objdir}/.config %{buildroot}%{microvm_dir}/config
make -s -C %{kernel_tree} O=%{objdir} %{make_opts} kernelrelease > %{buildroot}%{microvm_dir}/release

%files
%{microvm_dir}/

%changelog
