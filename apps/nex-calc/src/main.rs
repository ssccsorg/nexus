// nex-calc CLI — interactive FIH-based calculator.
//
// Computation as state space traversal. All methods are async
// through the FihStorage<SimIo> backend. The tokio runtime handles
// IO — all of which is in-memory via SimIo.

use std::io::{self, BufRead, Write};

use nex_calc::{CalcEngine, Constraint, OpType};

#[tokio::main]
async fn main() {
    let engine = CalcEngine::new();
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    println!("nex-calc — FIH-based calculator (async, FihStorage<SimIo>)");
    println!("Type 'help' for commands, 'quit' to exit.\n");

    loop {
        print!("> ");
        stdout.flush().unwrap();

        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(e) => {
                eprintln!("read error: {e}");
                break;
            }
        }

        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        let cmd = parts[0].to_lowercase();

        // Dispatch by operator type. Unary ops get one operand, binary get two.
        // For ergonomics, command aliases (`+`, `-`, `*`, `/`) map to their ops.
        match cmd.as_str() {
            "put" | "p" => cmd_put(&engine, &parts).await,
            "get" | "g" => cmd_get(&engine, &parts).await,
            "resolve" | "r" => cmd_resolve(&engine, &parts).await,
            "constrain" | "c" => cmd_constrain(&engine, &parts).await,
            "list" | "ls" => cmd_list(&engine).await,
            "stats" => cmd_stats(&engine).await,
            "help" | "h" | "?" => cmd_help(),
            "quit" | "q" | "exit" => { println!("bye."); break; }
            _ => {
                if let Some(op) = OpType::parse(cmd.as_str()) {
                    if op.arity() == 1 {
                        cmd_op_unary(&engine, op, &parts).await;
                    } else {
                        cmd_op(&engine, op, &parts).await;
                    }
                } else {
                    println!("unknown command: {cmd}. type 'help' for commands.");
                }
            }
        }
    }
}

async fn cmd_put(engine: &CalcEngine, parts: &[&str]) {
    if parts.len() < 2 {
        println!("usage: put <number>");
        return;
    }
    match parts[1].parse::<i64>() {
        Ok(n) => println!("Fact {} = {n}", short(&engine.put(n).await)),
        Err(_) => println!("invalid number: {}", parts[1]),
    }
}

async fn cmd_get(engine: &CalcEngine, parts: &[&str]) {
    if parts.len() < 2 {
        println!("usage: get <hash-prefix>");
        return;
    }
    match engine.find_fact(parts[1]).await {
        Some(id) => match engine.get(&id).await {
            Some(n) => println!("Fact {} = {n}", short(&id)),
            None => println!("not a number fact: {}", short(&id)),
        },
        None => println!("no fact matching: {}", parts[1]),
    }
}

async fn cmd_op(engine: &CalcEngine, op: OpType, parts: &[&str]) {
    if parts.len() < 3 {
        println!("usage: {} <fact-a> <fact-b>", op);
        return;
    }
    let a = engine.find_fact(parts[1]).await;
    let b = engine.find_fact(parts[2]).await;
    match (a, b) {
        (Some(a), Some(b)) => match engine.op(op, &a, &b).await {
            Ok(id) => println!(
                "Intent {} ({} {} {})",
                short(&id),
                op.symbol(),
                short(&a),
                short(&b)
            ),
            Err(e) => println!("error: {e}"),
        },
        (None, _) => println!("no fact matching: {}", parts[1]),
        (_, None) => println!("no fact matching: {}", parts[2]),
    }
}

/// Unary operator: `neg <fact>`, `abs <fact>`, `sqrt <fact>`, etc.
/// Only one operand Fact; rhs gets a dummy fact (value 0) for the Intent.
async fn cmd_op_unary(engine: &CalcEngine, op: OpType, parts: &[&str]) {
    if parts.len() < 2 {
        println!("usage: {} <fact>", op.symbol());
        return;
    }
    let a = engine.find_fact(parts[1]).await;
    match a {
        Some(a) => {
            // For unary ops, create a zero fact as dummy rhs.
            let zero = engine.put(0).await;
            match engine.op(op, &a, &zero).await {
                Ok(id) => println!("Intent {} ({} {})", short(&id), op.symbol(), short(&a)),
                Err(e) => println!("error: {e}"),
            }
        }
        None => println!("no fact matching: {}", parts[1]),
    }
}

async fn cmd_resolve(engine: &CalcEngine, parts: &[&str]) {
    if parts.len() < 2 {
        println!("usage: resolve <intent-hash-prefix>");
        return;
    }
    let intents = engine.list_intents().await;
    let prefix = parts[1].to_lowercase();
    let matches: Vec<_> = intents
        .iter()
        .filter(|(id, _)| id.to_string().starts_with(&prefix))
        .collect();
    if matches.is_empty() {
        println!("no intent matching: {}", parts[1]);
        return;
    }
    if matches.len() > 1 {
        println!("ambiguous prefix, {} intents:", matches.len());
        for (id, _) in &matches {
            println!("  {}", short(id));
        }
        return;
    }
    match engine.resolve(&matches[0].0).await {
        Ok(r) => println!(
            "{} {} {} = {}  → Fact {} = {}",
            r.lhs,
            r.op.symbol(),
            r.rhs,
            r.result_value,
            short(&r.result_id),
            r.result_value
        ),
        Err(e) => println!("error: {e}"),
    }
}

async fn cmd_constrain(engine: &CalcEngine, parts: &[&str]) {
    if parts.len() < 2 {
        println!(
            "usage: constrain <type> [arg]\ntypes: gt <n>, lt <n>, eq <n>, ne <n>, even, pos, double\n       constrain clear"
        );
        return;
    }
    if parts[1] == "clear" {
        engine.clear_hints().await;
        println!("all constraints cleared.");
        return;
    }
    let arg = parts.get(2).copied();
    match Constraint::parse(parts[1], arg) {
        Some(c) => {
            let id = engine.constrain(c.clone()).await;
            println!("Hint {} ({})", short(&id), c.description());
        }
        None => println!(
            "unknown constraint: {}. use: gt, lt, eq, ne, even, pos, double",
            parts[1]
        ),
    }
}

async fn cmd_list(engine: &CalcEngine) {
    let facts = engine.list_facts().await;
    let intents = engine.list_intents().await;
    let hints = engine.list_hints().await;

    if facts.is_empty() && intents.is_empty() && hints.is_empty() {
        println!("empty. use 'put' to store a number.");
        return;
    }

    if !facts.is_empty() {
        println!("Facts ({}):", facts.len());
        for (id, v) in &facts {
            println!("  {} = {v}", short(id));
        }
    }
    if !intents.is_empty() {
        println!("Intents ({}):", intents.len());
        for (id, concluded) in &intents {
            println!("  {} {}", short(id), if *concluded { "✓" } else { "…" });
        }
    }
    if !hints.is_empty() {
        println!("Hints ({}):", hints.len());
        for (id, c) in &hints {
            println!("  {} {c}", short(id));
        }
    }
}

async fn cmd_stats(engine: &CalcEngine) {
    println!("facts:   {}", engine.fact_count().await);
    println!("pending: {}", engine.pending_count().await);
    println!("hints:   {}", engine.list_hints().await.len());
}

fn cmd_help() {
    println!("Commands:");
    println!("  put <n>                   Store a number as a Fact");
    println!("  get <hash-prefix>         Read a number from a Fact");
    println!("  add|sub|mul|div <a> <b>   Arithmetic (+, -, *, /)");
    println!("  rem <a> <b>               Remainder (%)");
    println!("  pow <a> <b>               Power (^)");
    println!("  min|max <a> <b>           Minimum / maximum");
    println!("  neg|abs|sqrt <a>          Unary: negation, abs, sqrt");
    println!("  fac <a>                   Factorial");
    println!("  and|or|xor <a> <b>        Bitwise AND, OR, XOR");
    println!("  shl|shr <a> <b>           Shift left / right");
    println!("  bnot <a>                  Bitwise NOT");
    println!("  matmul|fft|conv <a> <b>   Vector ops (stage only)");
    println!("  resolve <hash-prefix>     Resolve an Intent (computation)");
    println!("  constrain <type> [arg]    Add a constraint Hint");
    println!("  constrain clear           Remove all constraints");
    println!("  list                      List Facts, Intents, Hints");
    println!("  stats                     Engine statistics");
    println!("  help                      Help");
    println!("  quit                      Exit");
    println!();
    println!("Constraints: gt <n>, lt <n>, eq <n>, ne <n>, even, pos, double");
    println!();
    println!("Concept: computation is FIH state space traversal.");
    println!("  Fact = number at a coordinate (immutable)");
    println!("  Intent = operator with direction (traversal vector)");
    println!("  Hint = constraint or transform (dynamic boundary)");
}

fn short(hash: &nexus_model::FihHash) -> String {
    let full = hash.to_string();
    format!("{}..{}", &full[..4], &full[full.len() - 4..])
}
