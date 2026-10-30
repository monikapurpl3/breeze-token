//! Talking to the person running it: Breeze Core's `fetch` wording and
//! layout, so the two read alike.

use std::io::{BufRead, IsTerminal, Write};

pub fn heading(title: &str) {
    println!("\n== {title} ==");
}

/// A paragraph, wrapped to the terminal's usual width. Leading newlines
/// become blank lines; leading spaces, the indent (two at least).
pub fn say(text: &str) {
    let after_newlines = text.trim_start_matches('\n');
    for _ in 0..(text.len() - after_newlines.len()) {
        println!();
    }
    let body = after_newlines.trim_start_matches(' ');
    let pad = " ".repeat((after_newlines.len() - body.len()).max(2));
    let mut line = String::new();
    for word in body.split_whitespace() {
        if !line.is_empty() && pad.len() + line.len() + 1 + word.len() > 78 {
            println!("{pad}{line}");
            line.clear();
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        println!("{pad}{line}");
    }
}

/// Whether there is someone to ask.
pub fn interactive() -> bool {
    std::io::stdin().is_terminal()
}

/// A prompt, `None` at end of input, so a script that runs out of answers
/// stops instead of looping.
pub fn ask(message: &str) -> Result<Option<String>, String> {
    print!("{message}");
    std::io::stdout()
        .flush()
        .map_err(|e| format!("cannot write to the terminal: {e}"))?;
    let mut line = String::new();
    match std::io::stdin().lock().read_line(&mut line) {
        Ok(0) => Ok(None),
        Ok(_) => Ok(Some(line.trim().to_string())),
        Err(e) => Err(format!("cannot read from the terminal: {e}")),
    }
}

pub fn yes_no(question: &str, default: bool) -> Result<bool, String> {
    let hint = if default { "[Y/n]" } else { "[y/N]" };
    loop {
        match ask(&format!("{question} {hint} "))?
            .as_deref()
            .map(str::to_ascii_lowercase)
        {
            None => return Ok(default),
            Some(a) if a.is_empty() => return Ok(default),
            Some(a) if a == "y" || a == "yes" => return Ok(true),
            Some(a) if a == "n" || a == "no" => return Ok(false),
            Some(_) => println!("  y or n, please."),
        }
    }
}

/// A secret, read without echo.
pub fn secret(prompt: &str) -> Result<String, String> {
    rpassword::prompt_password(prompt).map_err(|e| format!("cannot read it: {e}"))
}

/// One line of standard input, for scripts.
pub fn stdin_line(what: &str) -> Result<String, String> {
    let mut line = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(|e| format!("cannot read {what} from standard input: {e}"))?;
    Ok(line.trim_end_matches(['\r', '\n']).to_string())
}

/// What this machine is called, for the pairing's label on the server.
pub fn this_host() -> String {
    let named = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .or_else(|| std::fs::read_to_string("/proc/sys/kernel/hostname").ok())
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok());
    match named.map(|n| n.trim().to_string()) {
        Some(n) if !n.is_empty() => n,
        _ => "this machine".into(),
    }
}
