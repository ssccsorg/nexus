// MCU target runtime verification of the OS-less storage core (issue #181).
//
// This crate is support software: it runs the actual target stack (the
// no_std storage core) on an MCU-class target. It builds as a no_std
// firmware for riscv32imac-unknown-none-elf and runs under QEMU's virt
// machine, verifying runtime behavior that compile gates cannot:
//   - a machine-mode (mstatus.MIE) critical-section implementation
//   - Cell2 borrow semantics under that critical section
//   - EpochClock time flow from a monotonic tick
//   - a full FihStorage submit -> flush -> read round trip on the MCU
//   - the 512 KB SRAM budget (linker-enforced)
//
// On host targets the crate compiles as a stub so workspace builds stay
// green; the firmware body is riscv32-only.

#![cfg_attr(target_arch = "riscv32", no_std)]
#![cfg_attr(target_arch = "riscv32", no_main)]

#[cfg(target_arch = "riscv32")]
extern crate alloc;

#[cfg(target_arch = "riscv32")]
mod firmware {
    use alloc::boxed::Box;
    use alloc::sync::Arc;
    use core::alloc::{GlobalAlloc, Layout};
    use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

    use chton::cell::Cell2;
    use critical_section::RawRestoreState;
    use nex_core::{EpochClock, Monotonic, Now};
    use nex_fih::{AsyncFactCapable, AsyncStorageRead, BoardState, Content, Fact, FihStorage};
    use nexus_verify_support::{FlatIo, block_on};

    // ── Exit: SiFive test device on the virt machine ───────────────────

    const TEST_DEVICE: *mut u32 = 0x100000 as *mut u32;
    const TEST_PASS: u32 = 0x5555;
    const TEST_FAIL: u32 = 0x0001_3333;

    // 16550 UART on the virt machine: THR at +0, LSR at +5.
    const UART: *mut u8 = 0x1000_0000 as *mut u8;

    fn uart_putc(c: u8) {
        unsafe {
            while (UART.add(5).read_volatile() & (1 << 5)) == 0 {}
            UART.write_volatile(c);
        }
    }

    fn uart_puts(s: &str) {
        for b in s.bytes() {
            uart_putc(b);
        }
    }

    fn test_exit(pass: bool) -> ! {
        uart_puts(if pass { "\nPASS\n" } else { "\nFAIL\n" });
        unsafe {
            core::ptr::write_volatile(TEST_DEVICE, if pass { TEST_PASS } else { TEST_FAIL });
        }
        loop {
            core::hint::spin_loop();
        }
    }

    fn require(cond: bool) {
        if !cond {
            test_exit(false);
        }
    }

    #[panic_handler]
    fn panic(info: &core::panic::PanicInfo) -> ! {
        uart_puts("\nPANIC: ");
        uart_puts(&alloc::fmt::format(format_args!("{}", info.message())));
        test_exit(false)
    }

    // ── Machine-mode critical section: mstatus.MIE ─────────────────────
    //
    // Acquire disables interrupts by clearing mstatus.MIE (bit 3) and
    // returns whether they were previously enabled; release restores them.
    // QEMU virt boots in machine mode; a real MCU firmware uses the same
    // bit.
    struct MachineCs;
    critical_section::set_impl!(MachineCs);

    unsafe impl critical_section::Impl for MachineCs {
        unsafe fn acquire() -> RawRestoreState {
            let mstatus: usize;
            unsafe {
                core::arch::asm!(
                    "csrrci {0}, mstatus, 8",
                    out(reg) mstatus,
                    options(nomem, nostack)
                );
            }
            (mstatus & 8) != 0
        }
        unsafe fn release(restore_state: RawRestoreState) {
            if restore_state {
                unsafe {
                    core::arch::asm!("csrsi mstatus, 8", options(nomem, nostack));
                }
            }
        }
    }

    // ── Bump allocator over a static heap in .bss ──────────────────────

    const HEAP_SIZE: usize = 384 * 1024;
    static mut HEAP: [u8; HEAP_SIZE] = [0u8; HEAP_SIZE];

    struct BumpAllocator {
        next: AtomicUsize,
        peak: AtomicUsize,
    }

    unsafe impl Sync for BumpAllocator {}

    #[global_allocator]
    static ALLOC: BumpAllocator = BumpAllocator {
        next: AtomicUsize::new(0),
        peak: AtomicUsize::new(0),
    };

    fn heap_peak_bytes() -> usize {
        ALLOC.peak.load(Ordering::Relaxed)
    }

    unsafe impl GlobalAlloc for BumpAllocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let base = core::ptr::addr_of_mut!(HEAP) as usize;
            let mut next = self.next.load(Ordering::Relaxed);
            loop {
                let aligned = (next + layout.align() - 1) & !(layout.align() - 1);
                let end = aligned.checked_add(layout.size());
                match end {
                    Some(e) if e <= HEAP_SIZE => {
                        if self
                            .next
                            .compare_exchange_weak(next, e, Ordering::Relaxed, Ordering::Relaxed)
                            .is_ok()
                        {
                            // Track peak usage for the memory-budget report.
                            self.peak.fetch_max(e, Ordering::Relaxed);
                            return (base + aligned) as *mut u8;
                        }
                        // Another thread advanced the bump pointer; retry with
                        // the updated value (single-CPU MCU: contention-free).
                        let current = self.next.load(Ordering::Relaxed);
                        next = current;
                    }
                    _ => {
                        uart_puts("OOM: request ");
                        uart_puts(&alloc::fmt::format(format_args!(
                            "{} bytes, used {} bytes\n",
                            layout.size(),
                            self.next.load(Ordering::Relaxed)
                        )));
                        return core::ptr::null_mut();
                    }
                }
            }
        }
        unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
    }

    // ── Minimal block_on: the storage IO backends are synchronous under
    //    the hood, so a no-op waker and a poll loop suffice on the MCU.
    //    (Provided by nexus-verify-support.)

    // ── Entry ──────────────────────────────────────────────────────────

    core::arch::global_asm!(
        ".section .text.init",
        ".globl _start",
        "_start:",
        "  la sp, _stack_top",
        "  call run",
        "1:",
        "  j 1b"
    );

    #[unsafe(no_mangle)]
    pub fn run() -> ! {
        uart_puts("mcu-verify: start\n");

        // 1. Cell2 under the machine-mode critical section.
        let cell = Cell2::new(0u64);
        *cell.borrow_mut() = 41;
        require(*cell.borrow() == 41);
        let a = Cell2::new(1u64);
        let b = Cell2::new(2u64);
        {
            let ga = a.borrow();
            let gb = b.borrow();
            require(*ga + *gb == 3);
        }
        *a.borrow_mut() = 10;
        require(*b.borrow() == 2);
        uart_puts("step1 cell2: ok\n");

        // 2. EpochClock time flow from a monotonic tick.
        #[derive(Clone)]
        struct Tick(Arc<AtomicU32>);
        impl Monotonic for Tick {
            fn elapsed_nanos(&self) -> u64 {
                self.0.load(Ordering::Relaxed) as u64
            }
        }
        let tick = Tick(Arc::new(AtomicU32::new(0)));
        let epoch_secs: u64 = 1_700_000_000;
        let clock = EpochClock::new(epoch_secs, tick.clone());
        let epoch_ns = epoch_secs.saturating_mul(1_000_000_000);
        require(clock.now_nanos() == epoch_ns);
        tick.0.store(2_000_000_000, Ordering::Relaxed); // advance 2 seconds
        require(clock.now_nanos() == epoch_ns + 2_000_000_000);
        require(clock.now_secs() == epoch_secs + 2);
        uart_puts("step2 clock: ok\n");

        // 3. FihStorage submit -> flush -> read round trip on the MCU.
        let io = FlatIo::new();
        let storage = FihStorage::with_clock(io, "mcu", Box::new(clock));

        let fact = Fact::new(
            "mcu".into(),
            Content {
                mime_type: "text/plain".into(),
                data: b"hello mcu".to_vec(),
            },
            "harness".into(),
        );
        let id = match block_on(storage.submit_fact(&fact)) {
            Ok(id) => id,
            Err(_) => test_exit(false),
        };
        // A second fact exercises the multi-record path (distinct blob,
        // distinct record keys, shared structural index).
        let fact2 = Fact::new(
            "mcu".into(),
            Content {
                mime_type: "text/plain".into(),
                data: b"second mcu fact".to_vec(),
            },
            "harness".into(),
        );
        let id2 = match block_on(storage.submit_fact(&fact2)) {
            Ok(id2) => id2,
            Err(_) => test_exit(false),
        };
        if let Err(_) = block_on(storage.flush_pending()) {
            test_exit(false);
        }

        let state: BoardState = block_on(storage.read_state());
        let found = state
            .facts
            .iter()
            .any(|f| f.id == id && f.content.data == b"hello mcu");
        require(found);
        let found2 = state
            .facts
            .iter()
            .any(|f| f.id == id2 && f.content.data == b"second mcu fact");
        require(found2);
        require(state.facts.len() == 2);
        uart_puts("step3 storage round trip (2 facts): ok\n");
        uart_puts("peak heap: ");
        uart_puts(&alloc::fmt::format(format_args!(
            "{} bytes\n",
            heap_peak_bytes()
        )));

        test_exit(true);
    }
}

// Host stub: the firmware body is riscv32-only. Workspace builds on host
// targets compile this stub so `cargo check --workspace` stays green.
#[cfg(not(target_arch = "riscv32"))]
fn main() {
    println!("nexus-mcu-verify: host build; run under the riscv32imac-unknown-none-elf target");
}
