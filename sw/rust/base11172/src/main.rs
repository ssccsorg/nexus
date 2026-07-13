use std::env;

use base11172::{decode_bytes, encode_bytes};

fn print_usage() {
    eprintln!("Usage: base11172 <command> [args]");
    eprintln!();
    eprintln!("Commands:");
    eprintln!("  encode <text>     Encode text to Hangul Base11172");
    eprintln!("  decode <string>   Decode Hangul Base11172 back to text");
    eprintln!("  bench             Compare density vs Base64");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        print_usage();
        std::process::exit(1);
    }

    match args[1].as_str() {
        "encode" => {
            let text = args[2..].join(" ");
            let encoded = encode_bytes(text.as_bytes());
            println!("{encoded}");
        }
        "decode" => {
            if args.len() < 3 {
                eprintln!("error: provide a Hangul string to decode");
                std::process::exit(1);
            }
            let bytes = decode_bytes(&args[2]).unwrap_or_default();
            println!("{}", String::from_utf8_lossy(&bytes));
        }
        "bench" => {
            let n = 1_000_000u32;
            let data: Vec<u8> = (0..255).cycle().take(n as usize).collect();

            let start = std::time::Instant::now();
            let encoded = encode_bytes(&data);
            let enc_dur = start.elapsed();

            let start = std::time::Instant::now();
            let _decoded = decode_bytes(&encoded).unwrap();
            let dec_dur = start.elapsed();

            let base64_encoded = base64_encoded_len(n as usize);
            let tagma_len = encoded.len();

            println!("Benchmark: {n} bytes input");
            println!("  Encode:  {enc_dur:?}");
            println!("  Decode:  {dec_dur:?}");
            println!("  Base64 output size:  {base64_encoded} chars");
            println!("  Hangul output size:  {tagma_len} chars");
            println!(
                "  Density ratio:       {:.2}x",
                base64_encoded as f64 / tagma_len as f64
            );
        }
        _ => {
            eprintln!("unknown command: {}", args[1]);
            print_usage();
            std::process::exit(1);
        }
    }
}

fn base64_encoded_len(input_len: usize) -> usize {
    input_len.div_ceil(3) * 4
}
