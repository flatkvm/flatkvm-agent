Flatkvm is a tool to easily run [flatpak](https://flatpak.org/) apps isolated inside a VM, using QEMU/KVM.

flatkvm-agent provides the agent that runs on the VM and comunicates with the binary on the Host via a vsock port.

The binary produced by this repository must be embedded into the template image used with flatpak.
