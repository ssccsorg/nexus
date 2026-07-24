use std::env;
use std::time::Instant;

use nex_tagma::Coord;

fn print_usage() {
    eprintln!("Usage: nex-tagma <command> [args]");
    eprintln!();
    eprintln!("Commands:");
    eprintln!("  check <char|hex>      Validate a Tagma coordinate");
    eprintln!("  compose <i> <m> <f>   Compose three axis values");
    eprintln!("  decompose <char>      Decompose a coordinate");
    eprintln!("  dist <a> <b>          Field-wise Hamming distance");
    eprintln!("  bench                 SHA256 vs Tagma coordinate speed comparison");
}

fn parse_val(s: &str) -> Option<u16> {
    if s.chars().count() == 1 {
        return s.chars().next().map(|c| c as u16);
    }
    let s = s.strip_prefix("0x").unwrap_or(s);
    u16::from_str_radix(s, 16).ok()
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 || args[1] == "-h" || args[1] == "--help" {
        print_usage();
        std::process::exit(if args.len() < 2 { 1 } else { 0 });
    }

    match args[1].as_str() {
        "check" => {
            let Some(cp) = args.get(2).and_then(|s| parse_val(s)) else {
                eprintln!("error: provide a character or hex value");
                std::process::exit(1);
            };
            match Coord::from_code_point(cp) {
                Some(c) => {
                    let (i, m, f) = c.to_axes();
                    println!("valid: {} (U+{:04X}, i={i}, m={m}, f={f})", c.to_char(), cp);
                }
                None => println!("invalid: U+{cp:04X}"),
            }
        }
        "compose" => {
            let i: u8 = match args.get(2).and_then(|s| s.parse().ok()) {
                Some(v) => v,
                None => {
                    eprintln!("error: initial must be a number 0-18");
                    std::process::exit(1);
                }
            };
            let m: u8 = match args.get(3).and_then(|s| s.parse().ok()) {
                Some(v) => v,
                None => {
                    eprintln!("error: medial must be a number 0-20");
                    std::process::exit(1);
                }
            };
            let f: u8 = match args.get(4).and_then(|s| s.parse().ok()) {
                Some(v) => v,
                None => {
                    eprintln!("error: final must be a number 0-27");
                    std::process::exit(1);
                }
            };
            match Coord::from_axes(i, m, f) {
                Some(c) => println!("{}", c.to_char()),
                None => {
                    eprintln!("error: invalid axes ({i},{m},{f})");
                    std::process::exit(1);
                }
            }
        }
        "decompose" => {
            let Some(cp) = args.get(2).and_then(|s| parse_val(s)) else {
                eprintln!("error: provide a character or hex value");
                std::process::exit(1);
            };
            match Coord::from_code_point(cp) {
                Some(c) => {
                    let (i, m, f) = c.to_axes();
                    println!("{}: initial={i}, medial={m}, final={f}", c.to_char());
                }
                None => {
                    eprintln!("error: U+{cp:04X} is not valid");
                    std::process::exit(1);
                }
            }
        }
        "dist" => {
            let Some(a) = args.get(2).and_then(|s| parse_val(s)) else {
                eprintln!("error: provide two characters or hex values");
                std::process::exit(1);
            };
            let Some(b) = args.get(3).and_then(|s| parse_val(s)) else {
                eprintln!("error: provide two characters or hex values");
                std::process::exit(1);
            };
            match (Coord::from_code_point(a), Coord::from_code_point(b)) {
                (Some(ca), Some(cb)) => {
                    let (di, dm, df) = ca.hamming_distance(cb);
                    println!("Hamming distance: initial={di}, medial={dm}, final={df}");
                }
                _ => {
                    eprintln!("error: one or both values are not valid");
                    std::process::exit(1);
                }
            }
        }
        "bench" => {
            use sha2::{Digest, Sha256};
            use std::hint::black_box;
            let n = 100_000usize;
            let n_f = n as f64;

            // Warmup
            for _ in 0..1000 {
                let _ = Coord::from_axes(0, 0, 0);
                let mut h = Sha256::new();
                h.update([0u8; 3]);
                let _ = h.finalize();
            }

            // Tagma: 1-character
            let start = Instant::now();
            for i in 0..n {
                let init = (i % 19) as u8;
                let med = ((i / 19) % 21) as u8;
                let fin = ((i / (19 * 21)) % 28) as u8;
                black_box(Coord::from_axes(init, med, fin));
            }
            let t1 = start.elapsed();

            // Tagma: 2-character
            let start = Instant::now();
            for i in 0..n {
                let a = (i % 19) as u8;
                let b = ((i / 19) % 21) as u8;
                let c = ((i / (19 * 21)) % 28) as u8;
                let d = ((i / (19 * 21 * 28)) % 19) as u8;
                let e = ((i / (19 * 21 * 28 * 19)) % 21) as u8;
                let f = ((i / (19 * 21 * 28 * 19 * 21)) % 28) as u8;
                black_box(Coord::from_axes(a, b, c));
                black_box(Coord::from_axes(d, e, f));
            }
            let t2 = start.elapsed();

            // Tagma: 6-character
            let start = Instant::now();
            for i in 0..n {
                for j in 0..6 {
                    let idx = i * 6 + j;
                    let init = (idx % 19) as u8;
                    let med = ((idx / 19) % 21) as u8;
                    let fin = ((idx / (19 * 21)) % 28) as u8;
                    black_box(Coord::from_axes(init, med, fin));
                }
            }
            let t6 = start.elapsed();

            // Tagma: 19-character (same address space as SHA256: 11,172^19 ~ 2^256)
            let start = Instant::now();
            for i in 0..n {
                for j in 0..19 {
                    let idx = i * 19 + j;
                    let init = (idx % 19) as u8;
                    let med = ((idx / 19) % 21) as u8;
                    let fin = ((idx / (19 * 21)) % 28) as u8;
                    black_box(Coord::from_axes(init, med, fin));
                }
            }
            let t19 = start.elapsed();

            // SHA256
            let start = Instant::now();
            for i in 0..n {
                let init = (i % 19) as u8;
                let med = ((i / 19) % 21) as u8;
                let fin = ((i / (19 * 21)) % 28) as u8;
                let mut hasher = Sha256::new();
                hasher.update([init, med, fin]);
                black_box(hasher.finalize());
            }
            let sha = start.elapsed();

            println!("Benchmark: {n} operations");
            println!("  {:<20} {:>12} {:>10}", "Method", "Latency", "ns/op");
            println!("  {}", "-".repeat(44));
            println!(
                "  {:<20} {:>8?} {:>8.0} ns",
                "Tagma 1-syll",
                t1,
                t1.as_nanos() as f64 / n_f
            );
            println!(
                "  {:<20} {:>8?} {:>8.0} ns",
                "Tagma 2-syll",
                t2,
                t2.as_nanos() as f64 / n_f
            );
            println!(
                "  {:<20} {:>8?} {:>8.0} ns",
                "Tagma 6-syll",
                t6,
                t6.as_nanos() as f64 / n_f
            );
            println!(
                "  {:<20} {:>8?} {:>8.0} ns",
                "Tagma 19-syll",
                t19,
                t19.as_nanos() as f64 / n_f
            );
            println!(
                "  {:<20} {:>8?} {:>8.0} ns",
                "SHA256",
                sha,
                sha.as_nanos() as f64 / n_f
            );
            println!();
            println!("Speedup (vs SHA256):");
            println!(
                "  1-syll:   {:.0}x  (space: 1.1e4)",
                sha.as_nanos() as f64 / t1.as_nanos() as f64
            );
            println!(
                "  6-syll:   {:.0}x  (space: 1.9e24, UUID-scale)",
                sha.as_nanos() as f64 / t6.as_nanos() as f64
            );
            println!(
                "  19-syll:  {:.0}x  (space: 2^256, SHA256-equivalent)",
                sha.as_nanos() as f64 / t19.as_nanos() as f64
            );
        }
        _ => {
            eprintln!("unknown command: {}", args[1]);
            print_usage();
            std::process::exit(1);
        }
    }
}
