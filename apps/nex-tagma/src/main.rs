use std::env;
use std::time::Instant;

mod coord;
use coord::TagmaCoord;

fn print_usage() {
    eprintln!("Usage: tagma-poc <command> [args]");
    eprintln!();
    eprintln!("Commands:");
    eprintln!("  check <char|hex>      Validate a Tagma coordinate");
    eprintln!("  compose <i> <m> <f>   Compose three axis values");
    eprintln!("  decompose <char>      Decompose a coordinate");
    eprintln!("  dist <a> <b>          Field-wise Hamming distance");
    eprintln!("  bench                 SHA256 vs Tagma coordinate speed comparison");
}

fn parse_val(s: &str) -> Option<u16> {
    if s.len() == 1 {
        return s.chars().next().map(|c| c as u16);
    }
    let s = s.strip_prefix("0x").unwrap_or(s);
    u16::from_str_radix(s, 16).ok()
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        print_usage();
        std::process::exit(1);
    }

    match args[1].as_str() {
        "check" => {
            let cp = args.get(2).and_then(|s| parse_val(s)).unwrap_or(0);
            match TagmaCoord::from_code_point(cp) {
                Some(c) => {
                    let (i, m, f) = c.decompose();
                    println!("valid: {} (U+{:04X}, i={i}, m={m}, f={f})", c.to_char(), cp);
                }
                None => println!("invalid: U+{cp:04X}"),
            }
        }
        "compose" => {
            let i: u8 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(99);
            let m: u8 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(99);
            let f: u8 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(99);
            match TagmaCoord::new(i, m, f) {
                Some(c) => println!("{} (U+{:04X})", c.to_char(), c.code_point()),
                None => eprintln!("error: invalid axes ({i},{m},{f})"),
            }
        }
        "decompose" => {
            let cp = args.get(2).and_then(|s| parse_val(s)).unwrap_or(0);
            match TagmaCoord::from_code_point(cp) {
                Some(c) => {
                    let (i, m, f) = c.decompose();
                    println!("{}: initial={i}, medial={m}, final={f}", c.to_char());
                }
                None => eprintln!("error: U+{cp:04X} is not valid"),
            }
        }
        "dist" => {
            let a = args.get(2).and_then(|s| parse_val(s)).unwrap_or(0);
            let b = args.get(3).and_then(|s| parse_val(s)).unwrap_or(0);
            match (TagmaCoord::from_code_point(a), TagmaCoord::from_code_point(b)) {
                (Some(ca), Some(cb)) => {
                    let (di, dm, df) = ca.hamming_distance(&cb);
                    println!("Hamming distance: initial={di}, medial={dm}, final={df}");
                }
                _ => eprintln!("error: one or both values are not valid"),
            }
        }
        "bench" => {
            use sha2::{Digest, Sha256};
            let n = 100_000usize;
            let data: Vec<[u8; 3]> = (0..n).map(|i| {
                let init = (i % 19) as u8;
                let med = ((i / 19) % 21) as u8;
                let fin = ((i / (19 * 21)) % 28) as u8;
                [init, med, fin]
            }).collect();

            let start = Instant::now();
            for &[i, m, f] in &data {
                let _ = TagmaCoord::new(i, m, f);
            }
            let tagma_dur = start.elapsed();

            let start = Instant::now();
            for &bytes in &data {
                let mut hasher = Sha256::new();
                hasher.update(&bytes);
                let _ = hasher.finalize();
            }
            let sha_dur = start.elapsed();

            println!("Benchmark: {n} operations");
            println!("  Tagma coordinate:  {tagma_dur:?} ({:.0} ns/op)",
                tagma_dur.as_nanos() as f64 / n as f64);
            println!("  SHA256:            {sha_dur:?} ({:.0} ns/op)",
                sha_dur.as_nanos() as f64 / n as f64);
            println!("  Speedup:           {:.0}x",
                sha_dur.as_nanos() as f64 / tagma_dur.as_nanos() as f64);
        }
        _ => {
            eprintln!("unknown command: {}", args[1]);
            print_usage();
            std::process::exit(1);
        }
    }
}
