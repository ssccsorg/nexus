use std::env;
use std::time::Instant;

mod coord;
use coord::TagmaCoord;

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
    if args.len() < 2 {
        print_usage();
        std::process::exit(1);
    }

    match args[1].as_str() {
        "check" => {
            let Some(cp) = args.get(2).and_then(|s| parse_val(s)) else {
                eprintln!("error: provide a character or hex value");
                std::process::exit(1);
            };
            match TagmaCoord::from_code_point(cp) {
                Some(c) => {
                    let (i, m, f) = c.decompose();
                    println!("valid: {} (U+{:04X}, i={i}, m={m}, f={f})", c.to_char(), cp);
                }
                None => println!("invalid: U+{cp:04X}"),
            }
        }
        "compose" => {
            let i: u8 = match args.get(2).and_then(|s| s.parse().ok()) {
                Some(v) => v,
                None => { eprintln!("error: initial must be a number 0-18"); std::process::exit(1); }
            };
            let m: u8 = match args.get(3).and_then(|s| s.parse().ok()) {
                Some(v) => v,
                None => { eprintln!("error: medial must be a number 0-20"); std::process::exit(1); }
            };
            let f: u8 = match args.get(4).and_then(|s| s.parse().ok()) {
                Some(v) => v,
                None => { eprintln!("error: final must be a number 0-27"); std::process::exit(1); }
            };
            match TagmaCoord::new(i, m, f) {
                Some(c) => println!("{} (U+{:04X})", c.to_char(), c.code_point()),
                None => eprintln!("error: invalid axes ({i},{m},{f})"),
            }
        }
        "decompose" => {
            let Some(cp) = args.get(2).and_then(|s| parse_val(s)) else {
                eprintln!("error: provide a character or hex value");
                std::process::exit(1);
            };
            match TagmaCoord::from_code_point(cp) {
                Some(c) => {
                    let (i, m, f) = c.decompose();
                    println!("{}: initial={i}, medial={m}, final={f}", c.to_char());
                }
                None => eprintln!("error: U+{cp:04X} is not valid"),
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
            use std::hint::black_box;
            let n = 100_000usize;
            let n_f = n as f64;

            // Warmup
            for _ in 0..1000 {
                let _ = TagmaCoord::new(0, 0, 0);
                let mut h = Sha256::new();
                h.update([0u8; 3]);
                let _ = h.finalize();
            }

            // Tagma: 1-syllable
            let start = Instant::now();
            for i in 0..n {
                let init = (i % 19) as u8;
                let med = ((i / 19) % 21) as u8;
                let fin = ((i / (19 * 21)) % 28) as u8;
                black_box(TagmaCoord::new(init, med, fin));
            }
            let t1 = start.elapsed();

            // Tagma: 2-syllable
            let start = Instant::now();
            for i in 0..n {
                let a = (i % 19) as u8;
                let b = ((i / 19) % 21) as u8;
                let c = ((i / (19*21)) % 28) as u8;
                let d = ((i / (19*21*28)) % 19) as u8;
                let e = ((i / (19*21*28*19)) % 21) as u8;
                let f = ((i / (19*21*28*19*21)) % 28) as u8;
                black_box(TagmaCoord::new(a, b, c));
                black_box(TagmaCoord::new(d, e, f));
            }
            let t2 = start.elapsed();

            // Tagma: 6-syllable
            let start = Instant::now();
            for i in 0..n {
                for j in 0..6 {
                    let idx = i * 6 + j;
                    let init = (idx % 19) as u8;
                    let med = ((idx / 19) % 21) as u8;
                    let fin = ((idx / (19 * 21)) % 28) as u8;
                    black_box(TagmaCoord::new(init, med, fin));
                }
            }
            let t6 = start.elapsed();

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
            println!("  Tagma 1-syllable: {t1:?} ({:.0} ns/op)", t1.as_nanos() as f64 / n_f);
            println!("  Tagma 2-syllable: {t2:?} ({:.0} ns/op)", t2.as_nanos() as f64 / n_f);
            println!("  Tagma 6-syllable: {t6:?} ({:.0} ns/op)", t6.as_nanos() as f64 / n_f);
            println!("  SHA256:           {sha:?} ({:.0} ns/op)", sha.as_nanos() as f64 / n_f);
            println!("  Speedup 1-syll:   {:.0}x", sha.as_nanos() as f64 / t1.as_nanos() as f64);
            println!("  Speedup 6-syll:   {:.0}x", sha.as_nanos() as f64 / t6.as_nanos() as f64);
        }
        _ => {
            eprintln!("unknown command: {}", args[1]);
            print_usage();
            std::process::exit(1);
        }
    }
}
