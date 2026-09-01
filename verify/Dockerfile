# Verification runtime for the nexus target verification packages (#181).
#
# Provides what the GitHub Actions host does not ship: the RISC-V system
# emulator (qemu-system-riscv32) and the GNU RISC-V binutils used for the
# firmware size report. The firmware is built on the CI host (dtolnay
# rust-toolchain with the riscv32imac-unknown-none-elf target) and this
# image boots it on the virt machine via the mounted workspace.
#
# Compared with the ev and ssccs/poc images, nothing is compiled from
# source here (no Spike, no riscv-pk): qemu-system-misc and the riscv
# cross binutils are plain apt packages, so the image is quick to build.
# The riscv64-unknown-elf-* names are symlinked to the distro's
# riscv64-linux-gnu-* tools, which are binary-compatible for objcopy,
# objdump, and size on ELF files.

FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y \
    qemu-system-misc \
    gcc-riscv64-linux-gnu \
    binutils-riscv64-linux-gnu \
    && rm -rf /var/lib/apt/lists/*

# Alias so the size report and scripts find the canonical ELF tool names.
RUN ln -sf /usr/bin/riscv64-linux-gnu-size /usr/local/bin/riscv64-unknown-elf-size \
    && ln -sf /usr/bin/riscv64-linux-gnu-objcopy /usr/local/bin/riscv64-unknown-elf-objcopy \
    && ln -sf /usr/bin/riscv64-linux-gnu-objdump /usr/local/bin/riscv64-unknown-elf-objdump \
    && ln -sf /usr/bin/riscv64-linux-gnu-readelf /usr/local/bin/riscv64-unknown-elf-readelf

# Verify the emulator and tools.
RUN qemu-system-riscv32 --version | head -1 \
    && test -x /usr/local/bin/riscv64-unknown-elf-size

WORKDIR /workspace
