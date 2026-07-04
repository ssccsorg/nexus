// nex-calc CLI — interactive FIH-based calculator.
//
// Usage:
//   nex-calc                  Interactive mode
//
// Commands:
//   put <n>                   Store a number as a Fact
//   get <hash-prefix>         Read a number from a Fact
//   add|sub|mul|div <a> <b>   Create an operator Intent
//   resolve <hash-prefix>     Resolve an Intent (this IS the computation)
//   constrain <type> [arg]    Add a constraint Hint
//   hints clear               Remove all constraints
//   list                      List all Facts, Intents, and Hints
//   stats                     Show engine statistics
//   help                      Show this help
//   quit                      Exit
//
// Every computation is a traversal of the FIH state space.
// The result Fact persists at its new coordinate forever.

use std::io::{self, BufRead, Write};

use nex_calc::{CalcEngine, Constraint, OpType};

fn main() {
    let mut engine = CalcEngine::new();
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    println!("nex-calc — FIH-based calculator");
    println!("Type 'help' for commands, 'quit' to exit.\n");

    loop {
        print!("> ");
        stdout.flush().unwrap();

        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) => break, // EOF
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

        match cmd.as_str() {
            "put" | "p" => cmd_put(&mut engine, &parts),
            "get" | "g" => cmd_get(&engine, &parts),
            "add" | "+" => cmd_op(&mut engine, OpType::Add, &parts),
            "sub" | "-" => cmd_op(&mut engine, OpType::Sub, &parts),
            "mul" | "*" => cmd_op(&mut engine, OpType::Mul, &parts),
            "div" | "/" => cmd_op(&mut engine, OpType::Div, &parts),
            "resolve" | "r" => cmd_resolve(&mut engine, &parts),
            "constrain" | "c" => cmd_constrain(&mut engine, &parts),
            "list" | "ls" => cmd_list(&engine),
            "stats" => cmd_stats(&engine),
            "help" | "h" | "?" => cmd_help(),
            "quit" | "q" | "exit" => {
                println!("bye.");
                break;
            }
            _ => {
                println!("unknown command: {cmd}. type 'help' for commands.");
            }
        }
    }
}

fn cmd_put(engine: &mut CalcEngine, parts: &[&str]) {
    if parts.len() < 2 {
        println!("usage: put <number>");
        return;
    }
    match parts[1].parse::<i64>() {
        Ok(n) => {
            let id = engine.put(n);
            println!("Fact {} = {n}", short_id(&id));
        }
        Err(_) => println!("invalid number: {}", parts[1]),
    }
}

fn cmd_get(engine: &CalcEngine, parts: &[&str]) {
    if parts.len() < 2 {
        println!("usage: get <hash-prefix>");
        return;
    }
    match engine.find_fact(parts[1]) {
        Some(fact) => match engine.get(&fact.id) {
            Some(n) => println!("Fact {} = {n}", short_id(&fact.id)),
            None => println!("not a number fact: {}", short_id(&fact.id)),
        },
        None => println!("no fact matching: {}", parts[1]),
    }
}

fn cmd_op(engine: &mut CalcEngine, op: OpType, parts: &[&str]) {
    if parts.len() < 3 {
        println!("usage: {} <fact-a> <fact-b>", op);
        return;
    }
    let a = match engine.find_fact(parts[1]) {
        Some(f) => f.id,
        None => {
            println!("no fact matching: {}", parts[1]);
            return;
        }
    };
    let b = match engine.find_fact(parts[2]) {
        Some(f) => f.id,
        None => {
            println!("no fact matching: {}", parts[2]);
            return;
        }
    };
    match engine.op(op, &a, &b) {
        Ok(id) => println!("Intent {} ({} {} {})", short_id(&id), op.symbol(), short_id(&a), short_id(&b)),
        Err(e) => println!("error: {e}"),
    }
}

fn cmd_resolve(engine: &mut CalcEngine, parts: &[&str]) {
    if parts.len() < 2 {
        println!("usage: resolve <intent-hash-prefix>");
        return;
    }
    // Find the intent by prefix.
    let intent_ids: Vec<_> = engine
        .list_intents()
        .iter()
        .filter(|i| {
            i.id.to_string()
                .to_lowercase()
                .starts_with(&parts[1].to_lowercase())
        })
        .map(|i| i.id)
        .collect();

    if intent_ids.is_empty() {
        println!("no intent matching: {}", parts[1]);
        return;
    }
    if intent_ids.len() > 1 {
        println!("ambiguous prefix, {} intents match:", intent_ids.len());
        for id in &intent_ids {
            println!("  {}", short_id(id));
        }
        return;
    }

    match engine.resolve(&intent_ids[0]) {
        Ok(resolved) => {
            println!(
                "{} {} {} = {}  → Fact {} = {}",
                resolved.lhs,
                resolved.op.symbol(),
                resolved.rhs,
                resolved.result_value,
                short_id(&resolved.result_id),
                resolved.result_value,
            );
        }
        Err(e) => println!("error: {e}"),
    }
}

fn cmd_constrain(engine: &mut CalcEngine, parts: &[&str]) {
    if parts.len() < 2 {
        println!("usage: constrain <type> [arg]");
        println!("types: gt <n>, lt <n>, eq <n>, ne <n>, even, pos, double");
        println!("       constrain clear  (remove all constraints)");
        return;
    }

    if parts[1] == "clear" {
        engine.clear_hints();
        println!("all constraints cleared.");
        return;
    }

    let arg = parts.get(2).copied();
    match Constraint::parse(parts[1], arg) {
        Some(c) => {
            let id = engine.constrain(c.clone());
            println!("Hint {} ({})", short_id(&id), c.description());
        }
        None => println!("unknown constraint type: {}. use: gt, lt, eq, ne, even, pos, double", parts[1]),
    }
}

fn cmd_list(engine: &CalcEngine) {
    let facts = engine.list_facts();
    let intents = engine.list_intents();
    let hints = engine.list_hints();

    if facts.is_empty() && intents.is_empty() && hints.is_empty() {
        println!("empty. use 'put' to store a number.");
        return;
    }

    if !facts.is_empty() {
        println!("Facts ({}):", facts.len());
        for f in &facts {
            if let Some(n) = engine.get(&f.id) {
                println!("  {} = {n}", short_id(&f.id));
            } else {
                println!("  {} [non-number]", short_id(&f.id));
            }
        }
    }

    if !intents.is_empty() {
        println!("Intents ({}):", intents.len());
        for i in &intents {
            let status = if i.is_concluded { "✓" } else { "…" };
            print!("  {} {status}", short_id(&i.id));
            if !i.from_facts.is_empty() {
                print!(" from=[");
                for (idx, fh) in i.from_facts.iter().enumerate() {
                    if idx > 0 {
                        print!(", ");
                    }
                    print!("{}", short_id(fh));
                }
                print!("]");
            }
            println!(" op={}", i.description);
        }
    }

    if !hints.is_empty() {
        println!("Hints ({}):", hints.len());
        for (id, c) in &hints {
            println!("  {} {}", short_id(id), c.description());
        }
    }
}

fn cmd_stats(engine: &CalcEngine) {
    println!("steps:     {}", engine.step_count());
    println!("facts:     {}", engine.fact_count());
    println!("pending:   {}", engine.pending_count());
    let hints = engine.list_hints();
    println!("hints:     {}", hints.len());
}

fn cmd_help() {
    println!("nex-calc commands:");
    println!("  put <n>                   Store a number as a Fact");
    println!("  get <hash-prefix>         Read a number from a Fact");
    println!("  add|sub|mul|div <a> <b>   Create an operator Intent");
    println!("  resolve <hash-prefix>     Resolve an Intent (computation)");
    println!("  constrain <type> [arg]    Add a constraint Hint");
    println!("  constrain clear           Remove all constraints");
    println!("  list                      List all Facts, Intents, Hints");
    println!("  stats                     Engine statistics");
    println!("  help                      Show this help");
    println!("  quit                      Exit");
    println!();
    println!("Constraint types:");
    println!("  gt <n>    result > n");
    println!("  lt <n>    result < n");
    println!("  eq <n>    result = n");
    println!("  ne <n>    result != n");
    println!("  even      result is even");
    println!("  pos       result > 0");
    println!("  double    double operands before compute");
    println!();
    println!("Concept:");
    println!("  Every 'put' creates an immutable Fact in the FIH space.");
    println!("  Every operator creates an Intent pointing from operand Facts");
    println!("  toward a new coordinate. Resolution traverses that path.");
    println!("  The traversal IS the computation.");
}

fn short_id(hash: &nexus_model::FihHash) -> String {
    let full = hash.to_string();
    format!("{}..{}", &full[..4], &full[full.len() - 4..])
}
