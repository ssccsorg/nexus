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
//   - the read path a device uses: a store that keeps no record maps,
//     answering a query by walking the medium one record at a time, with the
//     framing walk reading no payload, agreeing with a state read of the same
//     volume, and a write the medium refused still answered for
//     (issues #203, #209, #213)
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
    use alloc::string::ToString;
    use alloc::sync::Arc;
    use core::alloc::{GlobalAlloc, Layout};
    use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

    use chton::cell::Cell2;
    use chton::io::FileIo;
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
    //
    // The arena is the heap the gate allows, and the peak it reports is read against it,
    // so it is sized to what the storage path needs rather than to the whole part. The
    // rest of the 512 KB SRAM budget is the firmware itself and the stack.

    const HEAP_SIZE: usize = 160 * 1024;
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
        if block_on(storage.flush_pending()).is_err() {
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

        // 4. The read path a device uses: a store that keeps no record maps answers by
        //    walking the medium, and the walk costs a page and a record rather than the
        //    volume. This is the pair a memory-bounded consumer runs with nex-fih's default
        //    features off, so the target gate has to run it.
        let device = FlatIo::new();
        {
            let writer = FihStorage::without_maps(
                device.clone(),
                "mcu",
                Box::new(EpochClock::new(epoch_secs, tick.clone())),
            );
            for payload in ["one", "two", "three"] {
                let fact = Fact::new(
                    "mcu/device".into(),
                    Content {
                        mime_type: "text/plain".into(),
                        data: payload.as_bytes().to_vec(),
                    },
                    "harness".into(),
                );
                if block_on(writer.submit_fact(&fact)).is_err() {
                    test_exit(false);
                }
            }
            if block_on(writer.flush_pending()).is_err() {
                test_exit(false);
            }
            // The mode's defining property: a store without maps holds nothing about the
            // volume, which is what keeps opening a volume independent of how many records
            // it holds.
            require(writer.fact_records.borrow().is_empty());
        }

        let reader = FihStorage::without_maps(
            device.clone(),
            "mcu",
            Box::new(EpochClock::new(epoch_secs, tick.clone())),
        );

        // The records come back with their content, which is what a walk has to hand over for
        // a query to be answered rather than merely counted.
        let mut walked = 0usize;
        let mut payload_bytes = 0usize;
        if block_on(reader.for_each_fact(|_record, content| {
            walked += 1;
            payload_bytes += content.data.len();
            true
        }))
        .is_err()
        {
            test_exit(false);
        }
        require(walked == 3);
        require(payload_bytes == 11); // "one" + "two" + "three"
        // The walk reads the medium; it does not build the maps it exists to avoid.
        require(reader.fact_records.borrow().is_empty());

        // The framing walk is the same visit without the payload reads: one key per record,
        // which is the read a count or an identifier listing spends.
        let before = device.traffic();
        let mut framing = 0usize;
        if block_on(reader.for_each_fact_record(|_record| {
            framing += 1;
            true
        }))
        .is_err()
        {
            test_exit(false);
        }
        let made = device.traffic().since(before);
        require(framing == walked);
        require(made.reads == framing as u32);

        // A question about one record reads one record.
        let before = device.traffic();
        let mut stopped = 0usize;
        if block_on(reader.for_each_fact_record(|_record| {
            stopped += 1;
            false
        }))
        .is_err()
        {
            test_exit(false);
        }
        let made = device.traffic().since(before);
        require(stopped == 1);
        require(made.reads == 1);

        // A host reads the same volume through the maps, and the two have to agree about which
        // records the volume holds. The walk promises no order, so the agreement is about the
        // set rather than about the order.
        let keeper = FihStorage::with_clock(
            device.clone(),
            "mcu",
            Box::new(EpochClock::new(epoch_secs, tick.clone())),
        );
        if block_on(keeper.rebuild_cache()).is_err() {
            test_exit(false);
        }
        let state: BoardState = block_on(keeper.read_state());
        require(state.facts.len() == walked);
        let mut matched = 0usize;
        if block_on(reader.for_each_fact_record(|record| {
            if state
                .facts
                .iter()
                .any(|fact| fact.id.to_string() == record.id)
            {
                matched += 1;
            }
            true
        }))
        .is_err()
        {
            test_exit(false);
        }
        require(matched == walked);
        uart_puts("step4 device read path (no maps, walk, framing): ok\n");

        // 5. A part at the end of its life refuses the write, and the write is not lost with
        //    the medium: the refused writes stay pending and the walk visits them from memory.
        //    Without that, a worn part turns a record the session still holds into one that is
        //    gone.
        let worn = FlatIo::new();
        worn.refuse_writes(true);
        let storage = FihStorage::without_maps(
            worn.clone(),
            "mcu",
            Box::new(EpochClock::new(epoch_secs, tick.clone())),
        );
        let fact = Fact::new(
            "mcu/worn".into(),
            Content {
                mime_type: "text/plain".into(),
                data: b"a write the part refuses".to_vec(),
            },
            "harness".into(),
        );
        if block_on(storage.submit_fact(&fact)).is_err() {
            test_exit(false);
        }
        require(block_on(storage.flush_pending()).is_err());

        let mut survived = 0usize;
        if block_on(storage.for_each_fact(|_record, content| {
            if content.data == b"a write the part refuses" {
                survived += 1;
            }
            true
        }))
        .is_err()
        {
            test_exit(false);
        }
        require(survived == 1);
        let landed = match block_on(worn.list("")) {
            Ok(keys) => keys.len(),
            Err(_) => test_exit(false),
        };
        require(landed == 0);
        uart_puts("step5 refused write survives: ok\n");

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
