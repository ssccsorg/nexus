// nex-derive CLI: a goal (work x intent x contract) resolves to a fact by
// intersecting posting bitsets.

use std::io::{self, BufRead, Write};

use nex_derive::{
    AXES, AXIS_CARD, Axis, Board, COORD_SPACE, Condition, Derivation, DeriveOutcome, Names, Query,
    WORDS,
};

const WITNESS_LIMIT: usize = 8;
const SEED: u64 = 0x5EED_1234_5678_9ABC;

fn main() {
    let mut board = Board::new();
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    println!("nex-derive: a goal derives a fact (work x intent x contract)");
    println!("Type 'help' for commands, 'demo' for a scripted run, 'quit' to exit.\n");

    loop {
        print!("> ");
        let _ = stdout.flush();

        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(error) => {
                eprintln!("read error: {error}");
                break;
            }
        }

        if !dispatch(&mut board, line.trim()) {
            break;
        }
    }
}

/// Returns `false` to stop the REPL.
fn dispatch(board: &mut Board, line: &str) -> bool {
    let parts: Vec<&str> = line.split_whitespace().collect();
    let Some(command) = parts.first().map(|token| token.to_lowercase()) else {
        return true;
    };

    match command.as_str() {
        "define" | "def" => cmd_define(board, &parts),
        "values" | "axes" => cmd_values(board),
        "derive" | "d" => cmd_derive(board, &parts),
        "resolve" | "r" => cmd_resolve(board, &parts),
        "explain" | "why" => cmd_explain(board, &parts),
        "scan" | "s" => cmd_scan(board, &parts),
        "bench" | "b" => cmd_bench(&parts),
        "stats" => cmd_stats(board),
        "demo" => cmd_demo(board),
        "help" | "h" | "?" => cmd_help(),
        "quit" | "exit" => {
            println!("bye.");
            return false;
        }
        other => println!("unknown command: {other}. type 'help' for commands."),
    }
    true
}

/// Registers symbolic names so a transcript stays readable and the value
/// indices are pinned before goals are derived.
fn cmd_define(board: &mut Board, parts: &[&str]) {
    if parts.len() < 3 {
        println!("usage: define <axis> <name> [name...]");
        return;
    }
    let Some(axis) = Axis::parse(parts[1]) else {
        println!("unknown axis: {}. use work, intent, or hint.", parts[1]);
        return;
    };
    for name in &parts[2..] {
        match board.names_mut().intern(axis, name) {
            Ok(value) => println!("{}[{value}] = {name}", axis.name()),
            Err(error) => println!("{error}"),
        }
    }
}

fn cmd_values(board: &Board) {
    for axis in Axis::ALL {
        let values = board.names().list(axis);
        if values.is_empty() {
            println!("{}: (none defined)", axis.name());
            continue;
        }
        let items: Vec<String> = values
            .iter()
            .enumerate()
            .map(|(index, name)| format!("{index}:{name}"))
            .collect();
        println!("{}: {}", axis.name(), items.join(" "));
    }
}

fn cmd_derive(board: &mut Board, parts: &[&str]) {
    if parts.len() < 4 {
        println!("usage: derive <work> <intent> <hint> [fact]");
        return;
    }
    let mut values = [0u8; AXES];
    for (offset, axis) in Axis::ALL.iter().enumerate() {
        match resolve_value(board.names_mut(), *axis, parts[1 + offset]) {
            Ok(value) => values[offset] = value,
            Err(error) => {
                println!("{error}");
                return;
            }
        }
    }
    let fact = if parts.len() > 4 {
        parts[4..].join(" ")
    } else {
        String::new()
    };

    let coord = nex_derive::pack(values);
    match board.derive(values[0], values[1], values[2], fact) {
        DeriveOutcome::Derived => {
            let derivation = board
                .derivation(coord)
                .expect("derivation recorded at its goal");
            println!("derived {coord} {}", describe(board, derivation));
        }
        DeriveOutcome::AlreadyDerived => {
            let derivation = board
                .derivation(coord)
                .expect("derivation present at its goal");
            println!("already derived {coord} {}", describe(board, derivation));
        }
    }
}

fn cmd_resolve(board: &Board, parts: &[&str]) {
    let Some(query) = parse_query(board, parts) else {
        return;
    };
    let indices = board.resolve(&query);
    let reference = board.scan(&query);
    println!(
        "resolved: {} derivation(s) over {} goal(s), {} constrained axis/axes",
        indices.len(),
        board.len(),
        query.constrained_axes()
    );
    print_witnesses(board, &indices);
    if indices == reference {
        println!("scan agrees: yes ({})", reference.len());
    } else {
        println!("scan agrees: NO (scan {})", reference.len());
    }
}

fn cmd_explain(board: &Board, parts: &[&str]) {
    let Some(query) = parse_query(board, parts) else {
        return;
    };
    println!("candidate elimination:");
    for (axis, count) in board.funnel(&query) {
        match axis {
            None => println!("  {:<19} {count}", "goals"),
            Some(axis) => println!("  {:<19} {count}", format!("intersect {}", axis.name())),
        }
    }
}

fn cmd_scan(board: &Board, parts: &[&str]) {
    let Some(query) = parse_query(board, parts) else {
        return;
    };
    let indices = board.scan(&query);
    println!(
        "scan: {} derivation(s), linear over {} goal(s)",
        indices.len(),
        board.len()
    );
    print_witnesses(board, &indices);
}

fn cmd_bench(parts: &[&str]) {
    if let Some(token) = parts.get(1) {
        let Ok(n) = token.parse::<usize>() else {
            println!("usage: bench [goal-count]");
            return;
        };
        print_bench_header();
        print_bench_line(Board::bench(n, query_budget(n), SEED));
        return;
    }

    print_bench_header();
    for n in [1_000usize, 10_000, 50_000, 200_000] {
        print_bench_line(Board::bench(n, query_budget(n), SEED));
    }
    println!();
    println!("resolution cost is set by the geometry ({WORDS} words per posting and");
    println!("{AXIS_CARD} values per axis), so it stays flat as goals grow while");
    println!("the scan rises with the goal count.");
}

fn cmd_stats(board: &Board) {
    println!("goal space:       {COORD_SPACE} ({AXIS_CARD} x {AXIS_CARD} x {AXIS_CARD})");
    println!(
        "bitset words:     {WORDS} ({} KiB per bitset)",
        WORDS * 8 / 1024
    );
    println!(
        "postings:         {} bitsets ({} KiB)",
        AXES * AXIS_CARD,
        AXES * AXIS_CARD * WORDS * 8 / 1024
    );
    println!("derivations:      {}", board.len());
    for axis in Axis::ALL {
        println!(
            "{} values:       {}",
            axis.name(),
            board.names().list(axis).len()
        );
    }
}

fn cmd_demo(board: &mut Board) {
    const SCRIPT: &[&str] = &[
        "define work parse scan audit",
        "define intent plan execute",
        "define hint strict lax",
        "derive parse plan strict design the parser",
        "derive parse execute strict run the parser over the corpus",
        "derive scan execute strict scan the corpus for tokens",
        "derive scan execute lax scan the corpus, best effort",
        "derive audit plan lax audit the plan for gaps",
        "resolve parse * *",
        "explain parse * *",
        "resolve * execute strict",
        "bench 50000",
    ];

    println!("-- demo: a goal derives a fact --");
    for line in SCRIPT {
        println!("> {line}");
        dispatch(board, line);
    }
    println!("-- demo end --");
}

fn cmd_help() {
    println!("Commands:");
    println!("  define <axis> <name>...     Pin symbolic values on an axis");
    println!("  values                      List defined values per axis");
    println!("  derive <work> <intent> <hint> [fact]");
    println!("                              Record that a goal derives a fact");
    println!("  resolve <work|*> <intent|*> <hint|*>");
    println!("                              Resolve a conjunctive goal (intersection)");
    println!("  explain <work|*> <intent|*> <hint|*>");
    println!("                              Candidate count after each condition");
    println!("  scan <work|*> <intent|*> <hint|*>");
    println!("                              Same goal by linear scan (reference)");
    println!("  bench [n]                   Intersection vs scan over goal counts");
    println!("  stats                       Geometry and derivation count");
    println!("  demo                        Scripted run over a small board");
    println!("  help                        This text");
    println!("  quit                        Exit");
    println!();
    println!("A goal is the product of the axes: work, intent, hint (the contract).");
    println!("Resolving a goal derives a fact. Use '*' for any value, or #<n> for a");
    println!("raw value index. Unknown names on 'derive' are interned.");
    println!();
    println!("The intersection that resolves a goal is symmetric; the direction lives");
    println!("in the derivation record, which carries goal -> fact.");
}

fn parse_query(board: &Board, parts: &[&str]) -> Option<Query> {
    if parts.len() != 1 + AXES {
        println!("usage: {} <work|*> <intent|*> <hint|*>", parts[0]);
        return None;
    }
    let mut conditions = Vec::with_capacity(AXES);
    for (offset, axis) in Axis::ALL.iter().enumerate() {
        match resolve_condition(board.names(), *axis, parts[1 + offset]) {
            Ok(condition) => conditions.push(condition),
            Err(error) => {
                println!("{error}");
                return None;
            }
        }
    }
    Some(Query::new(conditions))
}

/// A value token for `derive`: `#<n>` is a raw index, anything else is
/// interned.
fn resolve_value(names: &mut Names, axis: Axis, token: &str) -> Result<u8, String> {
    if let Some(rest) = token.strip_prefix('#') {
        let value: u8 = rest
            .parse()
            .map_err(|_| format!("bad value index: {token}"))?;
        if (value as usize) >= AXIS_CARD {
            return Err(format!(
                "value index out of range (0..{AXIS_CARD}): {token}"
            ));
        }
        return Ok(value);
    }
    names.intern(axis, token).map_err(|error| error.to_string())
}

/// A value token for `resolve`, `explain`, and `scan`: `*` allows every value,
/// `#<n>` is a raw index, and a name must already be defined.
fn resolve_condition(names: &Names, axis: Axis, token: &str) -> Result<Condition, String> {
    if token == "*" || token == "any" {
        return Ok(Condition::all(axis));
    }
    if let Some(rest) = token.strip_prefix('#') {
        let value: u8 = rest
            .parse()
            .map_err(|_| format!("bad value index: {token}"))?;
        if (value as usize) >= AXIS_CARD {
            return Err(format!(
                "value index out of range (0..{AXIS_CARD}): {token}"
            ));
        }
        return Ok(Condition::single(axis, value));
    }
    match names.lookup(axis, token) {
        Some(value) => Ok(Condition::single(axis, value)),
        None => Err(format!(
            "unknown {} value: {token} (define it with 'define {} {token}')",
            axis.name(),
            axis.name()
        )),
    }
}

fn describe(board: &Board, derivation: &Derivation) -> String {
    let mut parts = Vec::with_capacity(AXES);
    for (axis, value) in Axis::ALL
        .iter()
        .zip([derivation.work, derivation.intent, derivation.hint])
    {
        let text = match board.names().label(*axis, value) {
            Some(name) => name.to_string(),
            None => format!("#{value}"),
        };
        parts.push(format!("{}={}", axis.name(), text));
    }
    let goal = parts.join(" ");
    if derivation.fact.is_empty() {
        format!("[{goal}]")
    } else {
        format!("[{goal}] -> {}", derivation.fact)
    }
}

fn print_witnesses(board: &Board, indices: &[usize]) {
    for index in indices.iter().take(WITNESS_LIMIT) {
        println!("  {}", describe(board, &board.derivations()[*index]));
    }
    if indices.len() > WITNESS_LIMIT {
        println!("  ... and {} more", indices.len() - WITNESS_LIMIT);
    }
}

/// Targets a bounded total scan workload, so the sweep stays quick in a debug
/// build while still spanning the point where the resolution overtakes the
/// scan.
fn query_budget(n: usize) -> usize {
    (20_000_000 / n.max(1).saturating_mul(AXES)).clamp(20, 500)
}

fn print_bench_header() {
    println!(
        "{:>8} {:>8} {:>10} {:>12} {:>12} {:>9}  agree",
        "goals", "queries", "matched", "intersect/q", "scan/q", "speedup"
    );
}

fn print_bench_line(line: nex_derive::BenchLine) {
    let queries = line.queries as u32;
    let intersection = line.intersection / queries;
    let scan = line.scan / queries;
    let speedup = if line.intersection.as_secs_f64() > 0.0 {
        line.scan.as_secs_f64() / line.intersection.as_secs_f64()
    } else {
        f64::INFINITY
    };
    println!(
        "{:>8} {:>8} {:>10} {:>12?} {:>12?} {:>8.1}x  {}",
        line.n,
        line.queries,
        line.matched,
        intersection,
        scan,
        speedup,
        if line.agree { "yes" } else { "NO" }
    );
}
