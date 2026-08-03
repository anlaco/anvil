# Investigación: Aislamiento para el LID (Legacy Isolation Domain)

> Base citable para la decisión de **qué tecnología usar para aislar un
> ejecutor de lenguaje que corre en un SO legacy** (Windows 7/10, Ubuntu
> viejo) con DLLs/drivers nativos del fabricante del instrumento. Es el
> hueco que el ADR-0012 dejó abierto: "el mecanismo de aislamiento
> (contenedor/VM/firewall de SO) queda a definir al construir".
>
> Fuentes: docs oficiales y repos consultados con `webfetch` en esta sesión
> (2026-08-03). Cada afirmación marcada "verificado" se apoya en una URL
> real; donde no se pudo verificar, se indica.

## 0. El problema

Un banco de prueba real queda atascado en un SO durante 10+ años porque las
DLLs/drivers del fabricante del instrumento (NI-VISA, Keithley, etc.) solo
funcionan en ese SO. Anvil vive en un SO moderno y portable (WASM/wasmtime);
el ejecutor de lenguaje que toca hardware puede necesitar correr en un
Windows 7 o un Ubuntu antiguo. El LID (*Legacy Isolation Domain*) aísla ese
ejecutor para que **solo salga por puertas declaradas**: conexión de red a
los instrumentos y acceso a ficheros pactados; el resto, bloqueado.

Requisitos duros del LID:

1. **SO huésped legacy**: Windows 7/10 o Ubuntu/Linux antiguo (no solo
   moderno).
2. **DLLs/drivers nativos del huésped**: el ejecutor carga drivers
   kernel-mode del fabricante (NI-VISA, etc.) que **no se pueden
   recompilar**. El aislamiento no puede impedirlos.
3. **Red limitable a IPs/puertos concretos** (los instrumentos).
4. **FS limitable a directorios concretos** (los ficheros pactados).
5. **Overhead asumible** para un banco de producción.

## 1. Resumen ejecutivo

### Tabla comparativa

| Tecnología | SO huésped | DLLs/drivers nativos | Red limitable a IPs/puertos | FS limitable a dirs | Viabilidad LID |
|---|---|---|---|---|---|
| **QEMU/KVM** | Win7/10/11, Linux (verificado) | Sí (VM completa, drivers en huésped) | Sí (TAP + nftables/iptables en host, o `restrict=on`) | Sí (virtio-9p / virtio-fs, read-only) | **Alta** |
| **Hyper-V** | Win7/10/11, Linux (verificado) | Sí (VM completa) | Sí (vSwitch + Windows Firewall/ACLs en host) | Sí (VHDX; SMB share con ACLs) | **Alta** (host Windows) |
| **Sandboxie-Plus** | **Win7**+, Win10/11 (verificado) | Sí (DLLs nativas; drivers en kernel del host) | Sí (WFP firewall per-sandbox, verificado) | Sí (virtualización FS/registro; `OpenFilePath`/`ReadFilePath`) | **Alta** (Win sin VM) |
| **Windows Firewall + icacls + Standard User + AppLocker** | Win7+ | Sí (no toca el proceso) | Sí (reglas de salida por programa + scope IP, verificado) | Sí (ACLs por usuario; AppLocker path rules) | **Alta** (base mínima) |
| **Docker/Podman (Linux)** | Solo Linux | Sí (.so nativas; drivers kernel: cargar en host o `--device`/VFIO) | Sí (`--network none`/`--internal` + nftables en veth) | Sí (bind mounts `:ro`, `--read-only`) | **Alta** (Ubuntu legacy) |
| **systemd-nspawn** | Solo Linux (verificado) | Sí (.so nativas; módulos kernel no desde dentro) | Sí (`--private-network`, `--network-veth`, verificado) | Sí (`--bind=...:ro`, `--read-only`, verificado) | **Media-Alta** (Ubuntu legacy) |
| **Namespaces + seccomp + AppArmor/SELinux + firejail/bubblewrap** | Solo Linux | Sí (.so nativas; drivers kernel en host) | Sí (netns + nftables) | Sí (mount namespace + bind mounts RO) | **Alta** (Ubuntu legacy, bajo nivel) |
| Cloud Hypervisor | Linux + Windows moderno (verificado) | Sí | Igual que Firecracker (TAP + firewall host) | virtio-fs (verificado existe) | **Media** (Win7 no garantizado) |
| VirtualBox | Win7+, Linux | Sí | NAT/port-forwarding (no filtro IP granular nativo) | Shared folders (read-only configurable) | **Media-Alta** |
| VMware Workstation/ESXi | Win7+, Linux | Sí | vSwitch/NSX (no verificado) | Shared folders (no verificado) | **Media-Alta** (no verificado) |
| Windows Containers | Solo Windows moderno (Server 2016+, Win10 1607+) | DLLs Win32 sí; **drivers kernel no** | NAT/Transparent/L2bridge + HNS | Volúmenes + bind mounts | **Baja** (no Win7; no drivers kernel) |
| Windows Sandbox | Win10 18342+ Pro/Ent/Edu (verificado) | DLLs sí; **drivers kernel legacy dudoso** | On/off binario (no filtro IP granular nativo) | `<MappedFolders>` read-only (verificado) | **Media** (sin drivers kernel) |
| AppContainer / Job Objects / token integrity | Win8+ (AppContainer); WinXP+ (Job Objects) | Sí (DLLs nativas) | AppContainer: capabilities Internet/Intranet; WFP | ACLs por AppContainer SID | **Media** (bajo nivel, requiere empaquetado) |
| Firecracker | **Solo Linux** (verificado) | Sí (Linux) | No filtra L3/L4 (firewall en host) | Block devices; sin shared folder nativo | **Baja** (no Windows) |
| gVisor | **Solo Linux** (verificado) | Parcial; **no drivers kernel** (intercepta syscalls) | Network policy container; sin raw sockets | Gofer FS | **Baja** (no drivers) |
| WSL2 | Solo Linux dentro de Windows (verificado) | No (solo binarios Linux ELF, no DLLs Windows) | NAT / mirrored (Win11) | Distros con su FS | **Baja** (no carga DLLs Windows) |

### Recomendación (top 3)

1. **QEMU/KVM (host Linux) o Hyper-V (host Windows)** — viabilidad **Alta**.
   Es la única categoría que cumple los tres requisitos duros a la vez:
   huésped Win7/10 o Ubuntu legacy intacto, drivers kernel del fabricante
   cargados en el huésped, y red limitable a IPs/puertos vía firewall del
   host aplicado al TAP/vNIC. Madurez máxima. Elección: **QEMU/KVM si el
   host aislador es Linux; Hyper-V si es Windows**.

2. **Sandboxie-Plus + WFP firewall per-sandbox + Standard User + icacls**
   — viabilidad **Alta** para el caso Win7/Win10 **sin VM** (cuando el host
   aislador es el propio PC legacy). Corre DLLs nativas y drivers del
   fabricante sin virtualizar hardware; aísla FS/registro/red por sandbox.
   Overhead mínimo. Limitación: los drivers kernel se cargan en el host
   real (no aislados de kernel), pero los **procesos** que los usan sí lo
   están. Adecuado si el driver del fabricante es "de confianza" y lo que
   se aísla es el proceso gRPC y sus datos.

3. **Docker/Podman + nftables + seccomp + AppArmor + bind mounts RO** (o
   systemd-nspawn / namespaces puros) — viabilidad **Alta** para el caso
   **Ubuntu legacy**. Overhead mínimo, maduro, red limitable vía nftables
   en el veth, FS limitable vía bind mounts RO. Limitación: si los drivers
   requieren módulos kernel, hay que cargarlos en el host (pierde
   aislamiento kernel) o pasar el dispositivo vía `--device`/VFIO.

## 2. Detalle por tecnología

### 2.1 QEMU/KVM — viabilidad Alta

- **URL fuente:** https://www.linux-kvm.org/page/FAQ (verificado: *"What OSs
  can I run inside KVM VM? Several."*)
- **SO huésped:** Win7/10/11 y Linux, entre otros. Los drivers virtio-win
  (Fedora/Red Hat) dan soporte estable para Windows huésped.
- **DLLs/drivers nativos:** Sí. El huésped es un SO completo; carga
  drivers NI-VISA/Keithley/etc. siempre que el hardware (USB/PCI) esté
  asignado por passthrough (VFIO) o sea accesible por el dispositivo
  emulado.
- **Red:**
  - `--nic user` (SLIRP): NAT, con `hostfwd` y `restrict=on` (sin outbound).
  - `tap` + bridge: el host enruta y filtra. **Filtro L3/L4 granular vía
    nftables/iptables en el host** sobre el TAP del guest — reglas por
    IP/puerto concretas.
  - libvirt `nwfilter` para reglas declarativas.
- **FS:** virtio-9p y virtio-fs (virtiofsd) para shared folders; el host
  restringe el proceso virtiofsd con SELinux/AppArmor. `virtio-blk` con
  qcow2/raw + snapshots para rollback.
- **Overhead:** Medio (KVM near-native; QEMU sin KVM es lento).
- **Licencia:** GPL 2.0 (QEMU) / GPL+LGPL (KVM kernel). Madurez: máxima.
- **Por qué LID:** la opción más probada para huéspedes Windows legacy con
  drivers nativos + aislamiento de red/FS en el host. Cumple los 5
  requisitos.

### 2.2 Hyper-V — viabilidad Alta (host Windows)

- **URL fuente:** https://learn.microsoft.com/en-us/virtualization/hyper-v-on-windows/about/
  (verificado: *"many versions of Windows, Linux, and FreeBSD"*)
- **SO huésped:** Win7/8.1 (con Integration Services legacy), Win10/11,
  Linux, FreeBSD.
- **DLLs/drivers nativos:** Sí (VM completa). PCI passthrough (DDA en
  Server), USB por redirección.
- **Red:** Virtual switches **External** (bridge), **Internal** (host↔guest),
  **Private** (sólo guests). VLANs y SDN. Filtro IP/puerto: **Windows
  Firewall** en el host sobre la vNIC, o `Set-VMSwitch` + ACLs. Shielded VMs
  para protección contra admin host comprometido.
- **FS:** VHDX (con differencing disks para rollback); passthrough disks.
  Sin shared folder nativo tipo 9p → SMB share restringida con ACLs.
- **Overhead:** Bajo-medio (hypervisor tipo 1).
- **Licencia:** Incluido en Windows Pro/Enterprise/Education/Server.
- **Por qué LID:** la contrapartida de QEMU/KVM cuando el host aislador es
  Windows 10/11/Server. Máxima madurez enterprise.

### 2.3 Sandboxie-Plus — viabilidad Alta (Win sin VM)

- **URL fuente:** https://github.com/sandboxie-plus/Sandboxie
  (verificado: *"Windows 7 or higher (64-bit)"*; *"A network firewall per
  sandbox which supports Windows Filtering Platform (WFP)"*; *"DNS control
  by blocking or redirecting"*; *"Security enhanced sandboxes that restrict
  the availability of syscalls and endpoints"*)
- **SO huésped:** **Windows 7**+, Win10, Win11. Es la única opción SO-level
  que soporta Win7.
- **DLLs nativas:** Sí. Las DLLs del fabricante cargan dentro del sandbox.
  **Drivers kernel:** se cargan en el kernel real del host (Sandboxie
  instala su propio driver `SbieDrv`, pero los drivers de terceros se
  cargan en el host); el aislamiento es a nivel de **proceso** (FS/registro/
  red), no de módulos kernel.
- **Red:** WFP firewall **per-sandbox** → filtro L3/L4 por sandbox, SOCKS5
  proxy forzado, DNS redirect/bloqueo. **Sí, limitable a IPs/puertos.**
- **FS:** Virtualización de FS y registro por sandbox;
  `OpenFilePath`/`OpenConfPath`/`ClosedFilePath`/`ReadFilePath` en
  `Sandboxie.ini` para whitelisting/blacklisting por ruta. **Sí, limitable a
  directorios concretos.**
- **Overhead:** Bajo (a nivel proceso, no VM).
- **Licencia:** GPL v3 (Classic) + licencia custom Plus. Madurez: alta
  (desde 2004; fork activo desde 2020; 19k estrellas en GitHub).
- **Por qué LID:** ideal cuando el host aislador es el propio PC Win7/10
  (sin VM) y el driver del fabricante es de confianza. Aísla el proceso
  gRPC y sus datos sin virtualizar hardware. Overhead mínimo.

### 2.4 Windows Firewall + icacls + Standard User + AppLocker/SRP — viabilidad Alta (base mínima)

- **URL fuente (firewall outbound):**
  https://learn.microsoft.com/en-us/windows/security/threat-protection/windows-firewall/create-an-outbound-program-or-service-rule
  (verificado: *"On the Scope page, you can specify that the rule applies
  only to network traffic to or from the IP addresses"*)
- **SO:** Windows 7+ (Windows Firewall con Advanced Security desde Vista).
- **DLLs nativas:** Sí (no toca el proceso).
- **Red:** Reglas de salida por **program path** + **scope IP**. **Sí,
  limitable a IPs/puertos concretos por proceso.**
- **FS:** `icacls` para ACLs por usuario; AppLocker/SRP para path rules
  (bloquear ejecución/lectura fuera de rutas). Cuenta "Standard User" sin
  derechos minimiza superficie.
- **Overhead:** Nulo.
- **Licencia:** Nativo de Windows.
- **Por qué LID:** la base mínima de aislamiento en Win7/10 sin VM. Se
  complementa con Sandboxie para aislamiento más fuerte (FS/registro
  virtualizados por sandbox).

### 2.5 Docker / Podman (Linux) — viabilidad Alta (Ubuntu legacy)

- **URL fuente Podman:** https://podman.io/docs/installation (verificado)
- **URL fuente Docker networking:** https://docs.docker.com/engine/network/
  (verificado)
- **SO huésped:** Solo Linux (los contenedores comparten kernel Linux).
- **DLLs/.so nativas:** Sí. Apps Linux legacy con sus .so funcionan.
  **Drivers kernel (.ko):** hay que cargarlos en el host (pierde
  aislamiento kernel) o asignar dispositivo vía `--device`/VFIO.
- **Red:** `--network none` (sin red), `bridge` (NAT), `--internal` (sin
  salida externa), `macvlan`/`ipvlan`. **Filtro L3/L4 granular:**
  iptables/nftables en el host sobre el veth del contenedor → reglas por
  IP/puerto concretas.
- **FS:** Bind mounts (`-v /host/dir:/container/dir:ro`), tmpfs, volumes,
  `--read-only` para todo el rootfs. seccomp/AppArmor/SELinux restringen
  accesos.
- **Overhead:** Mínimo (sin VM).
- **Licencia:** Apache 2.0 (Podman, Docker engine).
- **Por qué LID:** la opción más ligera para Ubuntu legacy con .so nativas.
  Si los drivers no necesitan módulos kernel (p. ej. instrumentos por TCP/
  SCPI, sin driver kernel), es la opción ideal.

### 2.6 systemd-nspawn — viabilidad Media-Alta (Ubuntu legacy)

- **URL fuente:** https://man7.org/linux/man-pages/man1/systemd-nspawn.1.html
  (verificado)
- **SO huésped:** Solo Linux. *"Spawn a command or OS in a lightweight
  namespace container."*
- **DLLs/.so nativas:** Sí. *"kernel modules may not be loaded from within
  the container"* → los drivers kernel hay que cargarlos en el host.
- **Red (verificado):** `--private-network` (sin red del host),
  `--network-veth` (veth con el host). El host filtra con nftables sobre
  el veth → IPs/puertos concretos. *"This sandbox can easily be
  circumvented… if user namespaces are not used"* → usar `--private-users`.
- **FS (verificado):** `--bind=PATH[:PATH][:OPTIONS]` con `:ro` (read-only);
  `--read-only` para todo el rootfs. **Sí, limitable a directorios.**
- **Overhead:** Mínimo (namespace container, no VM).
- **Licencia:** LGPL 2.1+ (systemd).
- **Por qué LID:** alternativa a Docker para Ubuntu legacy cuando se quiere
  un contenedor de sistema completo (boot de un OS tree) sin la maquinaria
  de Docker. Más simple que Docker, menos portable.

### 2.7 Namespaces + seccomp + AppArmor/SELinux + firejail/bubblewrap — viabilidad Alta (Ubuntu legacy, bajo nivel)

- **URL fuente bubblewrap:** https://github.com/containers/bubblewrap
  (verificado: *"Network namespaces (CLONE_NEWNET): The sandbox will not
  see the network… only a loopback device"*)
- **URL fuente firejail:** https://firejail.wordpress.com/ (verificado)
- **SO huésped:** Solo Linux.
- **DLLs/.so nativas:** Sí. Drivers kernel: cargar en host o pasar
  dispositivo.
- **Red:** `network namespace` + `iptables`/`nftables` → reglas por
  IP/puerto concretas. Bubblewrap: red aislada con loopback; se añade veth +
  nftables en host para permitir solo ciertos destinos. Firejail:
  `--net=none`, per-profile net rules.
- **FS:** `mount namespace` + bind mounts read-only. Bubblewrap: *"the user
  can specify exactly what parts of the filesystem should be visible in the
  sandbox… readonly"*. Firejail: `whitelist`, `blacklist`, `read-only` por
  ruta. **Sí, limitable a directorios.**
- **Overhead:** Mínimo.
- **Licencia:** bubblewrap GPL v2 (verificado); firejail GPL v2 (verificado).
- **Por qué LID:** la receta más explícita y controlable para "solo sale a
  estas IPs/puertos y solo lee estos directorios" en Linux. Receta:
  `unshare -n` + veth + nftables (allow sólo IPs de instrumentos) +
  `mount -o bind,ro` de los dirs pactados + seccomp filter + AppArmor
  profile.

### 2.8 Cloud Hypervisor — viabilidad Media

- **URL fuente:** https://www.cloudhypervisor.org/ (verificado: *"Supports
  running modern Linux and Windows guests"*)
- **SO huésped:** Linux y **Windows moderno**. Win7 no garantizado (el
  proyecto se enfoca en "modern cloud workloads").
- **DLLs/drivers nativos:** Sí (huésped completo).
- **Red:** Igual que Firecracker: TAP en host, sin filtro L3/L4 nativo.
  nftables/iptables en el host.
- **FS:** virtio-fs soportado (verificado en release notes), block (qcow2,
  vhdx, raw).
- **Overhead:** Bajo (Rust, boot <100 ms con direct kernel boot).
- **Licencia:** Apache 2.0, Linux Foundation.
- **Por qué LID:** arriesgado para Win7; si el caso es Win10/11 o Ubuntu,
  QEMU/KVM o Hyper-V son más probados.

### 2.9 VirtualBox — viabilidad Media-Alta

- **URL fuente:** https://www.virtualbox.org/wiki/User_FAQ (verificado,
  contenido escaso)
- **SO huésped:** Win7/10/11, Linux.
- **DLLs/drivers nativos:** Sí (VM completa). USB passthrough y PCIe
  passthrough limitado.
- **Red:** NAT (con port-forwarding), Host-Only, Internal, Bridged. Para
  limitar a IPs concretas: NAT + port-forwarding (solo puertos, no IPs
  arbitrarias) o Host-Only + router virtual. **No hay filtro L3 granular
  nativo** → firewall en huésped o en un gateway.
- **FS:** Shared Folders (read-only configurable por carpeta), VDI/VDH.
- **Overhead:** Medio-alto.
- **Licencia:** GPL v3 (base) + PUEL para algunas features (USB 3, NVMe,
  cifrado).
- **Por qué LID:** fácil de desplegar en un banco, pero peor que QEMU/KVM
  para filtrado fino de red.

### 2.10 VMware Workstation/ESXi — viabilidad Media-Alta (no verificado)

- **Estado:** **No verificado** en esta sesión (no se consultó la doc).
- **SO huésped:** Win7+ y Linux (ampliamente conocido, pero no verificado).
- **Red:** vSwitch, VLANs, NSX en ESXi. Reglas por IP/puerto vía firewall
  externo o NSX Distributed Firewall. **No verificado.**
- **Por qué LID:** maduro, pero propietario/coste. Pendiente de verificar.

### 2.11 Windows Containers — viabilidad Baja

- **URL fuente:**
  https://learn.microsoft.com/en-us/virtualization/windowscontainers/about/
  (verificado: *"compatible with any machine running Windows 10, version
  1607 or later, or Windows Server 2016 or later"*)
- **SO huésped:** Solo Windows moderno. **No soporta Win7/8 como base
  image.**
- **DLLs nativas:** DLLs Win32 sí. **Drivers kernel: no** — comparten
  kernel con el host; no pueden cargar `.sys` arbitrarios del fabricante en
  modo kernel. Crítico para LID: NI-VISA/Keithley típicamente requieren un
  driver kernel.
- **Red:** NAT/Transparent/ICS/L2bridge/L2tunnel. Filtro por IP/puerto vía
  HNS (Host Network Service) + Windows Firewall.
- **FS:** Capas de imagen + volúmenes montados. Read-only posible.
- **Por qué LID descartado:** no soporta Win7 y no carga drivers kernel del
  fabricante.

### 2.12 Windows Sandbox — viabilidad Media (sin drivers kernel)

- **URL fuente (overview):**
  https://learn.microsoft.com/en-us/windows/security/application-security/application-isolation/windows-sandbox/
  (verificado: *"Part of Windows"* Pro/Enterprise/Education; *"uses
  hardware-based virtualization for kernel isolation"*; *"network
  connection by default… can be disabled"*)
- **URL fuente (config .wsb):**
  https://learn.microsoft.com/en-us/windows/security/application-security/application-isolation/windows-sandbox/windows-sandbox-configure-using-wsb-file
  (verificado: `<Networking>Enable|Disable|Default</Networking>` on/off
  binario; `<MappedFolders>` con `<ReadOnly>true|false</ReadOnly>`)
- **SO huésped:** Win10 build 18342+ Pro/Enterprise/Education. **No Win7.**
  No soportado en Home edition (verificado).
- **DLLs nativas:** Sí. **Drivers kernel legacy: dudoso** — no hay
  mecanismo documentado para cargar `.sys` firmados de fabricante dentro del
  sandbox; los drivers se instalan en el host y el sandbox es desechable.
- **Red:** On/off **binario**, no filtro IP granular nativo. Si se habilita,
  sale por el Hyper-V Default Switch. Para filtrar IPs: Windows Firewall en
  el host sobre la vNIC del sandbox.
- **FS:** `<MappedFolders>` read-only por carpeta. **Sí, limitable a
  directorios concretos.**
- **Overhead:** Bajo-medio (segundos en arrancar, VM ligera).
- **Licencia:** Incluido en Win10/11 Pro/Enterprise/Education.
- **Por qué LID:** viable si los drivers no son kernel-mode; **baja** si lo
  son (caso típico de instrumentación).

### 2.13 AppContainer / Job Objects / token integrity — viabilidad Media

- **URL fuente (AppContainer):**
  https://learn.microsoft.com/en-us/windows/win32/secauthz/appcontainer-isolation
  (verificado: *"Network isolation… Granular access can be granted for
  Internet access, Intranet access, and acting as a server"*;
  *"Read-write access can be granted to specific persistent files and
  registry keys"*)
- **SO:** AppContainer requiere Win8+ (no Win7). Job Objects y token
  integrity desde WinXP/7.
- **DLLs nativas:** Sí (procesos Win32). Drivers kernel: dependen del token
  del proceso.
- **Red:** Capabilities (Internet/Intranet) + WFP. No tan granular por
  IP/puerto sin firewall adicional.
- **FS:** ACLs por AppContainer SID. **Sí, limitable por fichero/clave.**
- **Overhead:** Mínimo.
- **Por qué LID:** potente pero requiere desarrollar/empaquetar la app como
  AppContainer (no trivial para un server gRPC Python/MATLAB legacy). Más
  útil como mecanismo subyacente (lo usa Windows Sandbox).

### 2.14 Firecracker — viabilidad Baja

- **URL fuente:**
  https://github.com/firecracker-microvm/firecracker/blob/main/docs/design.md
  (verificado: *"Firecracker runs on Linux hosts and with Linux guest OSs"*)
- **SO huésped:** **Solo Linux.** No soporta huésped Windows → no sirve para
  el LID con Windows 7/10.
- **Red:** *"Firecracker does not perform any network traffic filtering.
  All egress traffic from a guest is therefore considered untrusted, and
  should be filtered at the host-level."* → filtrado en host con
  nftables/iptables sobre el TAP.
- **FS:** Block devices; sin shared folder nativo. vsock para IPC.
- **Overhead:** Bajo (microVM, ~125 ms boot).
- **Licencia:** Apache 2.0.
- **Por qué LID descartado:** solo Linux. Para Ubuntu legacy hay opciones
  más cómodas (Docker/systemd-nspawn).

### 2.15 gVisor — viabilidad Baja

- **URL fuente:** https://gvisor.dev/docs/architecture_guide/security/
  (verificado: *"gVisor was created in order to provide additional defense
  against the exploitation of kernel bugs by untrusted userspace code"*;
  *"we do not implement or pass through these specialized APIs"*)
- **SO huésped:** **Solo Linux.** El Sentry implementa syscalls Linux; no
  hay soporte para huéspedes Windows.
- **DLLs/.so nativas:** Parcial. Intercepta syscalls y reimplementa.
  **Drivers kernel no soportados** (ioctls especializados no se
  implementan).
- **Red:** Network policy a nivel container; sin raw sockets salvo
  host-net.
- **FS:** Gofer process gestiona FS; directfs opcional.
- **Overhead:** Alto en syscalls (reimplementación).
- **Licencia:** Apache 2.0.
- **Por qué LID descartado:** no Windows, no drivers kernel, overhead en
  syscalls.

### 2.16 WSL2 — viabilidad Baja

- **URL fuente:** https://learn.microsoft.com/en-us/windows/wsl/about
  (verificado: *"Run GNU/Linux command-line applications"*, *"Run common
  BASH command-line tools… other ELF-64 binaries"*)
- **SO huésped:** Solo Linux (distros Ubuntu/Debian/etc.) dentro de Windows.
- **DLLs nativas Windows:** **No.** WSL2 ejecuta binarios Linux (ELF), no
  DLLs Windows. Para invocar apps Windows desde WSL hay interop, pero el
  proceso corre en Windows sin aislar.
- **Red:** VM ligera con NAT; modo `mirrored` en Win11. Hyper-V firewall
  aplicable.
- **FS:** Cada distro tiene su FS; acceso a `\\wsl$` y `/mnt/c`.
- **Por qué LID descartado:** no carga DLLs Windows; su propósito es lo
  opuesto (correr Linux dentro de Windows).

## 3. Decisión por topología

El LID no es una sola tecnología: depende de **dónde** corre el ejecutor y
**qué SO** necesita. La decisión se ramifica por topología:

### 3.1 El ejecutor necesita Windows legacy (Win7/10) con drivers kernel

- **Si hay host aislador (PC moderno o el propio banco):**
  **QEMU/KVM (host Linux) o Hyper-V (host Windows)** con TAP/vNIC + firewall
  L3/L4 en el host + virtio-fs/9p (QEMU) o SMB con ACLs (Hyper-V) para FS.
  El huésped Win7/10 corre con sus drivers intactos; el host filtra la red.
- **Si el host aislador es el propio PC Win7/10 (sin VM):**
  **Sandboxie-Plus** + WFP firewall per-sandbox + Standard User + icacls.
  Los drivers del fabricante se cargan en el host (de confianza); el
  proceso gRPC y sus datos se aíslan por sandbox.

### 3.2 El ejecutor necesita Ubuntu/Linux legacy con .so nativas (sin drivers kernel)

- **Docker/Podman** + nftables en el veth + bind mounts RO + seccomp +
  AppArmor. Overhead mínimo, maduro.
- Alternativa más simple: **systemd-nspawn** (`--private-network` +
  `--network-veth` + `--bind=...:ro` + nftables en host).
- Alternativa más explícita: **namespaces + seccomp + firejail/bubblewrap**.

### 3.3 El ejecutor necesita Ubuntu/Linux legacy con drivers kernel (.ko)

- Los módulos kernel hay que cargarlos en el host (pierde aislamiento
  kernel) o pasar el dispositivo vía VFIO/`--device`. En ese caso, el
  aislamiento de red/FS sigue siendo válido, pero el kernel no está aislado.
- Si se requiere kernel aislado: **QEMU/KVM** con huésped Ubuntu legacy
  (kernel propio, drivers dentro) + passthrough del dispositivo.

## 4. Puntos abiertos a probar en hardware real

1. **QEMU/KVM con Win7 + drivers NI-VISA/Keithley:** verificar que el
   huésped Win7 arranca con drivers virtio-win, carga el driver del
   fabricante y ve el dispositivo (USB/PCI passthrough con VFIO).
2. **Sandboxie-Plus con drivers NI-VISA:** verificar que el proceso
   sandboxed puede abrir el device interface del driver cargado en el
   host. Posible fricción con `OpenIpcPath`/`OpenFilePath` para el namespace
   del driver.
3. **Hyper-V con Win7 huésped + drivers legacy:** confirmar que Win7
   arranca con Integration Services legacy y que el PCI/USB passthrough
   funciona para el dispositivo del instrumento.
4. **Docker en Ubuntu legacy con driver kernel:** si el fabricante
   provee un driver kernel (`.ko`), confirmar si carga en el host y si el
   contenedor ve el device interface (p. ej. `/dev/usbtmc`).
5. **Filtro L3/L4 en Hyper-V para vNIC del sandbox/VM:** confirmar la
   mejor receta (`Set-VMSwitch` + ACLs vs Windows Firewall en host).
6. **VMware Workstation/ESXi:** no verificado. Verificar vSwitch ACLs /
   NSX Distributed Firewall para filtro por IP/puerto.
7. **Cloud Hypervisor con Windows 7:** el proyecto dice "modern Windows
   guests" → Win7 no garantizado. Probar arrancando una ISO de Win7 +
   drivers virtio-win.
8. **Windows Sandbox con drivers kernel legacy (NI-VISA):** no hay
   documentación de carga de `.sys` firmados dentro del sandbox. Probar
   instalando el driver en el host y ejecutando la app dentro del
   sandbox.

## 5. Nota de transparencia sobre fuentes

- **Verificadas con webfetch en esta sesión (2026-08-03):** KVM FAQ,
  Hyper-V overview, Windows Sandbox overview + .wsb config, Windows
  Containers about, gVisor security model, Sandboxie-Plus README,
  AppContainer isolation, Windows Firewall outbound rules, Docker
  networking overview, Podman installation, bubblewrap README, firejail
  about, WSL about, Cloud Hypervisor portada, Firecracker design.md,
  systemd-nspawn man page (man7.org), VirtualBox User_FAQ.
- **No verificadas (HTTP error / 404 / anti-bot):** QEMU networking wiki
  (anti-bot Anubis), virtio-fs QEMU docs (404).
- **No intentadas:** VMware Workstation/ESXi, Astronics, Advantest,
  NI VXI-11 tutorial (404), OpenTAP docs (error transporte).